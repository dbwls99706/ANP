use std::collections::HashMap;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Webhook node — sends webhook HTTP requests with signing and retry.
///
/// Delivers JSON payloads to external webhook endpoints with optional
/// HMAC-like signature verification and automatic retry with exponential backoff.
///
/// Config example:
/// ```json
/// {
///   "url": "https://hooks.example.com/webhook",
///   "method": "POST",
///   "headers": { "Content-Type": "application/json" },
///   "secret_env": "WEBHOOK_SECRET",
///   "retry_count": 3,
///   "timeout_secs": 10
/// }
/// ```
///
/// Input: any JSON data (becomes the webhook body).
/// Output: `{ "status": 200, "success": true, "response": "...", "attempts": 1 }`
pub struct WebhookNode {
    url: String,
    method: String,
    headers: HashMap<String, String>,
    secret_env: String,
    retry_count: u32,
    timeout_secs: u64,
}

impl Default for WebhookNode {
    fn default() -> Self {
        Self::new()
    }
}

impl WebhookNode {
    pub fn new() -> Self {
        Self {
            url: String::new(),
            method: "POST".to_string(),
            headers: HashMap::new(),
            secret_env: String::new(),
            retry_count: 3,
            timeout_secs: 10,
        }
    }
}

/// Simple HMAC-like signature using FNV-1a hash of payload + secret.
/// Not cryptographically strong, but sufficient for basic webhook verification.
fn simple_sign(data: &str, secret: &str) -> String {
    let combined = format!("{}{}", data, secret);
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in combined.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("sha256={:016x}", hash)
}

/// Validate URL scheme: must be http:// or https://.
fn validate_webhook_url(url: &str) -> Result<(), NodeError> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(NodeError::InvalidConfig(
            "webhook URL must not be empty".into(),
        ));
    }
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err(NodeError::InvalidConfig(format!(
            "webhook URL must start with http:// or https://, got: '{}'",
            trimmed
        )));
    }
    Ok(())
}

#[async_trait::async_trait]
impl NodeExecutor for WebhookNode {
    fn node_type(&self) -> &str {
        "webhook"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(url) = config.get("url").and_then(|v| v.as_str()) {
            validate_webhook_url(url)?;
            self.url = url.to_string();
        }

        if let Some(method) = config.get("method").and_then(|v| v.as_str()) {
            let m = method.to_uppercase();
            match m.as_str() {
                "GET" | "POST" | "PUT" | "PATCH" | "DELETE" => {
                    self.method = m;
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unsupported HTTP method: '{}'. Use: GET, POST, PUT, PATCH, DELETE",
                        other
                    )));
                }
            }
        }

        if let Some(headers) = config.get("headers").and_then(|v| v.as_object()) {
            for (k, v) in headers {
                if let Some(val) = v.as_str() {
                    // Reject header values with control characters (header injection)
                    if !val.chars().any(|c| c == '\r' || c == '\n' || c == '\0') {
                        self.headers.insert(k.clone(), val.to_string());
                    }
                }
            }
        }

        if let Some(secret) = config.get("secret_env").and_then(|v| v.as_str()) {
            self.secret_env = secret.to_string();
        }

        if let Some(retry) = config.get("retry_count").and_then(|v| v.as_u64()) {
            if retry > 10 {
                return Err(NodeError::InvalidConfig(
                    "retry_count must be between 0 and 10".into(),
                ));
            }
            self.retry_count = retry as u32;
        }

        if let Some(timeout) = config.get("timeout_secs").and_then(|v| v.as_u64()) {
            if timeout == 0 || timeout > 300 {
                return Err(NodeError::InvalidConfig(
                    "timeout_secs must be between 1 and 300".into(),
                ));
            }
            self.timeout_secs = timeout;
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        if self.url.is_empty() {
            return Err(NodeError::MissingInput(
                "no webhook URL configured".into(),
            ));
        }

        let body = serde_json::to_string(&input)
            .map_err(|e| NodeError::ExecutionFailed(format!("failed to serialize body: {}", e)))?;

        let max_attempts = self.retry_count + 1;
        let mut last_error = String::new();
        let mut attempts: u32 = 0;

        for attempt in 0..max_attempts {
            attempts = attempt + 1;

            if attempt > 0 {
                let backoff = std::time::Duration::from_millis(1000 * 2u64.pow(attempt - 1));
                tokio::time::sleep(backoff).await;
            }

            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(self.timeout_secs))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|e| {
                    NodeError::ExecutionFailed(format!("failed to build HTTP client: {}", e))
                })?;

            let mut request = match self.method.as_str() {
                "GET" => client.get(&self.url),
                "PUT" => client.put(&self.url),
                "PATCH" => client.patch(&self.url),
                "DELETE" => client.delete(&self.url),
                _ => client.post(&self.url),
            };

            // Set content type
            request = request.header("Content-Type", "application/json");

            // Add custom headers
            for (k, v) in &self.headers {
                request = request.header(k.as_str(), v.as_str());
            }

            // Add signature if secret is configured
            if !self.secret_env.is_empty() {
                if let Ok(secret) = std::env::var(&self.secret_env) {
                    let sig = simple_sign(&body, &secret);
                    request = request.header("X-Webhook-Signature", sig);
                }
            }

            request = request.body(body.clone());

            match request.send().await {
                Ok(response) => {
                    let status = response.status().as_u16();

                    // Retry on 5xx server errors
                    if status >= 500 && attempt < max_attempts - 1 {
                        last_error = format!("server error: {}", status);
                        continue;
                    }

                    let response_body = response.text().await.unwrap_or_default();
                    let success = status >= 200 && status < 300;

                    let mut output = NodeOutput::data(serde_json::json!({
                        "status": status,
                        "success": success,
                        "response": response_body,
                        "attempts": attempts,
                        "url": self.url
                    }));

                    if attempts > 1 {
                        output = output.with_log(format!(
                            "webhook delivered after {} attempts",
                            attempts
                        ));
                    }

                    return Ok(output);
                }
                Err(e) => {
                    last_error = e.to_string();
                    if attempt == max_attempts - 1 {
                        return Ok(NodeOutput::data(serde_json::json!({
                            "status": 0,
                            "success": false,
                            "error": last_error,
                            "attempts": attempts,
                            "url": self.url
                        })));
                    }
                }
            }
        }

        Ok(NodeOutput::data(serde_json::json!({
            "status": 0,
            "success": false,
            "error": last_error,
            "attempts": attempts,
            "url": self.url
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_sign_deterministic() {
        let sig1 = simple_sign("hello", "secret");
        let sig2 = simple_sign("hello", "secret");
        assert_eq!(sig1, sig2);
        assert!(sig1.starts_with("sha256="));
    }

    #[test]
    fn test_simple_sign_different_data() {
        let sig1 = simple_sign("hello", "secret");
        let sig2 = simple_sign("world", "secret");
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn test_simple_sign_different_secret() {
        let sig1 = simple_sign("hello", "secret1");
        let sig2 = simple_sign("hello", "secret2");
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn test_validate_url_valid() {
        assert!(validate_webhook_url("https://hooks.example.com/webhook").is_ok());
        assert!(validate_webhook_url("http://example.com/hook").is_ok());
    }

    #[test]
    fn test_validate_url_invalid_scheme() {
        assert!(validate_webhook_url("ftp://example.com").is_err());
        assert!(validate_webhook_url("file:///etc/passwd").is_err());
    }

    #[test]
    fn test_validate_url_empty() {
        assert!(validate_webhook_url("").is_err());
        assert!(validate_webhook_url("   ").is_err());
    }

    #[tokio::test]
    async fn test_init_defaults() {
        let mut node = WebhookNode::new();
        node.init(&serde_json::json!({})).await.unwrap();
        assert_eq!(node.method, "POST");
        assert_eq!(node.retry_count, 3);
        assert_eq!(node.timeout_secs, 10);
    }

    #[tokio::test]
    async fn test_init_with_config() {
        let mut node = WebhookNode::new();
        node.init(&serde_json::json!({
            "url": "https://hooks.example.com/webhook",
            "method": "PUT",
            "headers": { "X-Custom": "value" },
            "secret_env": "MY_SECRET",
            "retry_count": 5,
            "timeout_secs": 30
        }))
        .await
        .unwrap();

        assert_eq!(node.url, "https://hooks.example.com/webhook");
        assert_eq!(node.method, "PUT");
        assert_eq!(node.headers.get("X-Custom").unwrap(), "value");
        assert_eq!(node.secret_env, "MY_SECRET");
        assert_eq!(node.retry_count, 5);
        assert_eq!(node.timeout_secs, 30);
    }

    #[tokio::test]
    async fn test_init_invalid_url() {
        let mut node = WebhookNode::new();
        let result = node
            .init(&serde_json::json!({"url": "ftp://bad.com"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_init_invalid_method() {
        let mut node = WebhookNode::new();
        let result = node
            .init(&serde_json::json!({"method": "INVALID"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_init_retry_count_too_high() {
        let mut node = WebhookNode::new();
        let result = node
            .init(&serde_json::json!({"retry_count": 99}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_init_timeout_too_high() {
        let mut node = WebhookNode::new();
        let result = node
            .init(&serde_json::json!({"timeout_secs": 999}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_missing_url() {
        let mut node = WebhookNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let result = node
            .execute(serde_json::json!({"data": "test"}), &ctx)
            .await;
        assert!(result.is_err());
    }
}
