use std::collections::HashMap;
use std::net::IpAddr;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// HTTP Client node — makes outbound HTTP requests.
///
/// Config example:
/// ```json
/// {
///   "url": "https://api.example.com/data",
///   "method": "GET",
///   "headers": { "Authorization": "Bearer ..." },
///   "timeout_secs": 30
/// }
/// ```
///
/// Input can override config at runtime:
/// ```json
/// { "url": "...", "method": "POST", "body": { ... }, "headers": { ... } }
/// ```
///
/// Output: `{ "status": 200, "headers": { ... }, "body": ... }`
///
/// Security: blocks private/internal IPs (SSRF protection), enforces 1MB response limit,
/// disables auto-redirects, validates URL scheme (http/https only).
pub struct HttpClientNode {
    url: String,
    method: String,
    headers: HashMap<String, String>,
    timeout_secs: u64,
    retry_count: u32,
    retry_backoff_ms: u64,
    /// Shared HTTP client — reuses TCP/TLS connections across requests
    client: Option<reqwest::Client>,
}

/// Maximum response body size: 1MB
const MAX_RESPONSE_BYTES: usize = 1_048_576;

impl Default for HttpClientNode {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClientNode {
    pub fn new() -> Self {
        Self {
            url: String::new(),
            method: "GET".to_string(),
            headers: HashMap::new(),
            timeout_secs: 30,
            retry_count: 2,
            retry_backoff_ms: 500,
            client: None,
        }
    }
}

/// Check if an IP address is private/internal (SSRF protection).
fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()          // 127.0.0.0/8
                || v4.is_private()     // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
                || v4.is_link_local()  // 169.254.0.0/16
                || v4.is_broadcast()   // 255.255.255.255
                || v4.is_unspecified() // 0.0.0.0
                || v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64 // 100.64.0.0/10 (CGN)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()           // ::1
                || v6.is_unspecified() // ::
                // fc00::/7 (unique local)
                || (v6.segments()[0] & 0xFE00) == 0xFC00
                // fe80::/10 (link-local)
                || (v6.segments()[0] & 0xFFC0) == 0xFE80
        }
    }
}

/// Validate URL: scheme must be http/https, host must not resolve to private IP.
fn validate_url(url: &str) -> Result<reqwest::Url, NodeError> {
    let url = url.trim();
    if url.is_empty() {
        return Err(NodeError::InvalidConfig("URL must not be empty".into()));
    }

    let parsed = reqwest::Url::parse(url)
        .map_err(|e| NodeError::InvalidConfig(format!("invalid URL '{}': {}", url, e)))?;

    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(NodeError::InvalidConfig(format!(
                "unsupported URL scheme: '{}'. Only http and https are allowed",
                other
            )));
        }
    }

    // Block private/internal hostnames
    if let Some(host) = parsed.host_str() {
        let lower = host.to_lowercase();
        if lower == "localhost"
            || lower == "metadata.google.internal"
            || lower.ends_with(".internal")
            || lower.ends_with(".local")
        {
            return Err(NodeError::InvalidConfig(format!(
                "blocked internal hostname: '{}'",
                host
            )));
        }

        // Check if host is a direct IP address
        if let Ok(ip) = host.parse::<IpAddr>() {
            if is_private_ip(&ip) {
                return Err(NodeError::InvalidConfig(format!(
                    "blocked private/internal IP: '{}'",
                    ip
                )));
            }
        }

        // Check IPv6 bracket notation (e.g., [::1])
        let stripped = host.trim_start_matches('[').trim_end_matches(']');
        if let Ok(ip) = stripped.parse::<IpAddr>() {
            if is_private_ip(&ip) {
                return Err(NodeError::InvalidConfig(format!(
                    "blocked private/internal IP: '{}'",
                    ip
                )));
            }
        }
    } else {
        return Err(NodeError::InvalidConfig("URL has no host".into()));
    }

    Ok(parsed)
}

fn parse_headers(value: &serde_json::Value) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    if let Some(obj) = value.as_object() {
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                // Reject header values containing control characters (header injection)
                if !s.chars().any(|c| c == '\r' || c == '\n' || c == '\0') {
                    headers.insert(k.clone(), s.to_string());
                }
            }
        }
    }
    headers
}

fn validate_method(method: &str) -> Result<String, NodeError> {
    let m = method.to_uppercase();
    match m.as_str() {
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS" => Ok(m),
        other => Err(NodeError::InvalidConfig(format!(
            "unsupported HTTP method: '{}'",
            other
        ))),
    }
}

#[async_trait::async_trait]
impl NodeExecutor for HttpClientNode {
    fn node_type(&self) -> &str {
        "http_client"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(url) = config.get("url").and_then(|v| v.as_str()) {
            validate_url(url)?;
            self.url = url.to_string();
        }

        if let Some(method) = config.get("method").and_then(|v| v.as_str()) {
            self.method = validate_method(method)?;
        }

        if let Some(headers) = config.get("headers") {
            self.headers = parse_headers(headers);
        }

        if let Some(timeout) = config.get("timeout_secs").and_then(|v| v.as_u64()) {
            if timeout == 0 || timeout > 300 {
                return Err(NodeError::InvalidConfig(
                    "timeout_secs must be between 1 and 300".into(),
                ));
            }
            self.timeout_secs = timeout;
        }

        if let Some(retry) = config.get("retry_count").and_then(|v| v.as_u64()) {
            if retry > 10 {
                return Err(NodeError::InvalidConfig(
                    "retry_count must be between 0 and 10".into(),
                ));
            }
            self.retry_count = retry as u32;
        }

        if let Some(backoff) = config.get("retry_backoff_ms").and_then(|v| v.as_u64()) {
            if backoff > 30000 {
                return Err(NodeError::InvalidConfig(
                    "retry_backoff_ms must be between 0 and 30000".into(),
                ));
            }
            self.retry_backoff_ms = backoff;
        }

        // Build shared client once — reuses TCP connections and TLS sessions
        self.client = Some(
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(self.timeout_secs))
                .redirect(reqwest::redirect::Policy::none())
                .pool_max_idle_per_host(16)
                .pool_idle_timeout(std::time::Duration::from_secs(90))
                .build()
                .map_err(|e| {
                    NodeError::InitFailed(format!("failed to build HTTP client: {}", e))
                })?,
        );

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        // Allow input to override URL, method, headers, body at runtime
        let url = input
            .get("url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.url.clone());

        if url.is_empty() {
            return Err(NodeError::MissingInput(
                "no URL configured or provided in input".into(),
            ));
        }

        validate_url(&url)?;

        let method = match input.get("method").and_then(|v| v.as_str()) {
            Some(m) => validate_method(m)?,
            None => self.method.clone(),
        };

        // Merge headers: config defaults + input overrides
        let mut headers = self.headers.clone();
        if let Some(input_headers) = input.get("headers") {
            for (k, v) in parse_headers(input_headers) {
                headers.insert(k, v);
            }
        }

        let body = input.get("body").cloned();

        // Retry loop with exponential backoff
        let max_attempts = self.retry_count + 1;
        let mut last_err = String::new();
        for attempt in 0..max_attempts {
            if attempt > 0 {
                let delay = self.retry_backoff_ms * 2u64.pow(attempt - 1);
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            }
            // Reuse pooled client — avoids TCP/TLS handshake per request
            let client = self.client.as_ref().cloned().unwrap_or_else(|| {
                reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(self.timeout_secs))
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .expect("failed to build fallback HTTP client")
            });

            let mut request = match method.as_str() {
                "GET" => client.get(&url),
                "POST" => client.post(&url),
                "PUT" => client.put(&url),
                "PATCH" => client.patch(&url),
                "DELETE" => client.delete(&url),
                "HEAD" => client.head(&url),
                _ => {
                    return Err(NodeError::ExecutionFailed(format!(
                        "unsupported method: '{}'",
                        method
                    )));
                }
            };

            for (k, v) in &headers {
                request = request.header(k.as_str(), v.as_str());
            }

            if let Some(body_val) = &body {
                request = request
                    .header("Content-Type", "application/json")
                    .json(body_val);
            }

            match request.send().await {
                Ok(response) => {
                    let status_code = response.status();

                    // Retry on 5xx server errors (not 4xx client errors)
                    if status_code.is_server_error() && attempt < max_attempts - 1 {
                        last_err = format!("server error: {}", status_code.as_u16());
                        continue;
                    }

                    // Success path — process response
                    let status = status_code.as_u16();
                    let resp_headers: HashMap<String, String> = response
                        .headers()
                        .iter()
                        .filter_map(|(k, v)| {
                            v.to_str().ok().map(|val| (k.to_string(), val.to_string()))
                        })
                        .collect();

                    let content_type = resp_headers
                        .get("content-type")
                        .cloned()
                        .unwrap_or_default();

                    let body_bytes = response.bytes().await.map_err(|e| {
                        NodeError::ExecutionFailed(format!("failed to read response body: {}", e))
                    })?;

                    if body_bytes.len() > MAX_RESPONSE_BYTES {
                        return Err(NodeError::ExecutionFailed(format!(
                            "response body too large: {} bytes (limit: {} bytes)",
                            body_bytes.len(),
                            MAX_RESPONSE_BYTES
                        )));
                    }

                    let resp_body = if content_type.contains("application/json") {
                        serde_json::from_slice(&body_bytes).unwrap_or(serde_json::Value::Null)
                    } else {
                        let text = String::from_utf8_lossy(&body_bytes).into_owned();
                        serde_json::Value::String(text)
                    };

                    let output = serde_json::json!({
                        "status": status,
                        "headers": resp_headers,
                        "body": resp_body
                    });

                    let mut node_output = NodeOutput::data(output)
                        .with_log(format!("{} {} -> {}", method, url, status));
                    if attempt > 0 {
                        node_output = node_output
                            .with_log(format!("succeeded after {} attempts", attempt + 1));
                    }
                    return Ok(node_output);
                }
                Err(e) => {
                    last_err = e.to_string();
                    if attempt == max_attempts - 1 {
                        return Err(NodeError::ExecutionFailed(format!(
                            "HTTP request failed after {} attempts: {}",
                            max_attempts, last_err
                        )));
                    }
                }
            }
        }

        Err(NodeError::ExecutionFailed(format!(
            "HTTP request failed after {} attempts: {}",
            max_attempts, last_err
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- URL validation ---

    #[test]
    fn test_validate_url_valid() {
        assert!(validate_url("https://api.example.com/data").is_ok());
        assert!(validate_url("http://example.com").is_ok());
    }

    #[test]
    fn test_validate_url_invalid_scheme() {
        assert!(validate_url("ftp://example.com").is_err());
        assert!(validate_url("file:///etc/passwd").is_err());
    }

    #[test]
    fn test_validate_url_malformed() {
        assert!(validate_url("not a url").is_err());
    }

    #[test]
    fn test_validate_url_empty() {
        assert!(validate_url("").is_err());
        assert!(validate_url("   ").is_err());
    }

    // --- SSRF protection ---

    #[test]
    fn test_validate_url_blocks_localhost() {
        assert!(validate_url("http://localhost:8080").is_err());
        assert!(validate_url("http://localhost/secret").is_err());
    }

    #[test]
    fn test_validate_url_blocks_loopback_ip() {
        assert!(validate_url("http://127.0.0.1:8080").is_err());
        assert!(validate_url("http://127.0.0.1").is_err());
    }

    #[test]
    fn test_validate_url_blocks_private_ips() {
        assert!(validate_url("http://10.0.0.1").is_err());
        assert!(validate_url("http://172.16.0.1").is_err());
        assert!(validate_url("http://192.168.1.1").is_err());
    }

    #[test]
    fn test_validate_url_blocks_ipv6_loopback() {
        assert!(validate_url("http://[::1]:8080").is_err());
    }

    #[test]
    fn test_validate_url_blocks_internal_hostnames() {
        assert!(validate_url("http://metadata.google.internal/computeMetadata").is_err());
        assert!(validate_url("http://service.internal").is_err());
        assert!(validate_url("http://something.local").is_err());
    }

    #[test]
    fn test_validate_url_blocks_link_local() {
        assert!(validate_url("http://169.254.169.254/latest/meta-data").is_err());
    }

    #[test]
    fn test_validate_url_blocks_unspecified() {
        assert!(validate_url("http://0.0.0.0").is_err());
    }

    // --- Private IP checks ---

    #[test]
    fn test_is_private_ip() {
        assert!(is_private_ip(&"127.0.0.1".parse().unwrap()));
        assert!(is_private_ip(&"10.0.0.1".parse().unwrap()));
        assert!(is_private_ip(&"172.16.0.1".parse().unwrap()));
        assert!(is_private_ip(&"192.168.0.1".parse().unwrap()));
        assert!(is_private_ip(&"169.254.1.1".parse().unwrap()));
        assert!(is_private_ip(&"0.0.0.0".parse().unwrap()));
        assert!(is_private_ip(&"::1".parse().unwrap()));

        assert!(!is_private_ip(&"8.8.8.8".parse().unwrap()));
        assert!(!is_private_ip(&"1.1.1.1".parse().unwrap()));
    }

    // --- Header injection ---

    #[test]
    fn test_parse_headers_rejects_injection() {
        let val = serde_json::json!({
            "Good": "value",
            "Bad": "value\r\nX-Injected: true"
        });
        let h = parse_headers(&val);
        assert_eq!(h.get("Good").unwrap(), "value");
        assert!(!h.contains_key("Bad"));
    }

    // --- Init tests ---

    #[tokio::test]
    async fn test_init_defaults() {
        let mut node = HttpClientNode::new();
        node.init(&serde_json::json!({})).await.unwrap();
        assert_eq!(node.method, "GET");
        assert_eq!(node.timeout_secs, 30);
    }

    #[tokio::test]
    async fn test_init_with_config() {
        let mut node = HttpClientNode::new();
        node.init(&serde_json::json!({
            "url": "https://example.com/api",
            "method": "POST",
            "headers": { "X-Custom": "value" },
            "timeout_secs": 60
        }))
        .await
        .unwrap();
        assert_eq!(node.url, "https://example.com/api");
        assert_eq!(node.method, "POST");
        assert_eq!(node.headers.get("X-Custom").unwrap(), "value");
        assert_eq!(node.timeout_secs, 60);
    }

    #[tokio::test]
    async fn test_init_invalid_method() {
        let mut node = HttpClientNode::new();
        let result = node.init(&serde_json::json!({ "method": "INVALID" })).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_init_invalid_timeout() {
        let mut node = HttpClientNode::new();
        let result = node.init(&serde_json::json!({ "timeout_secs": 0 })).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_init_rejects_private_url() {
        let mut node = HttpClientNode::new();
        let result = node
            .init(&serde_json::json!({ "url": "http://127.0.0.1:9200" }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_init_retry_config() {
        let mut node = HttpClientNode::new();
        node.init(&serde_json::json!({
            "retry_count": 5,
            "retry_backoff_ms": 1000
        }))
        .await
        .unwrap();
        assert_eq!(node.retry_count, 5);
        assert_eq!(node.retry_backoff_ms, 1000);
    }

    #[tokio::test]
    async fn test_init_retry_count_too_high() {
        let mut node = HttpClientNode::new();
        let result = node.init(&serde_json::json!({"retry_count": 99})).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_headers() {
        let val = serde_json::json!({
            "Authorization": "Bearer token",
            "X-Custom": "value"
        });
        let h = parse_headers(&val);
        assert_eq!(h.get("Authorization").unwrap(), "Bearer token");
        assert_eq!(h.get("X-Custom").unwrap(), "value");
    }
}
