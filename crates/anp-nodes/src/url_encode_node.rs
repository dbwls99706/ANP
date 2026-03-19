use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// UrlEncodeNode — URL encoding/decoding and URL parsing
pub struct UrlEncodeNode {
    operation: String,
    field: String,
}

impl Default for UrlEncodeNode {
    fn default() -> Self {
        Self::new()
    }
}

impl UrlEncodeNode {
    pub fn new() -> Self {
        Self {
            operation: "encode".to_string(),
            field: "text".to_string(),
        }
    }
}

fn percent_encode(input: &str) -> String {
    let mut result = String::with_capacity(input.len() * 3);
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
}

fn percent_decode(input: &str) -> Result<String, String> {
    let mut result = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err("incomplete percent encoding".to_string());
            }
            let hex = &input[i + 1..i + 3];
            let byte = u8::from_str_radix(hex, 16)
                .map_err(|_| format!("invalid percent encoding: %{}", hex))?;
            result.push(byte);
            i += 3;
        } else if bytes[i] == b'+' {
            result.push(b' ');
            i += 1;
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }

    String::from_utf8(result).map_err(|e| format!("invalid UTF-8 after decoding: {}", e))
}

fn parse_url(url: &str) -> Result<serde_json::Value, String> {
    let mut remaining = url;
    let mut result = serde_json::Map::new();

    // Extract fragment
    let fragment = if let Some(hash_pos) = remaining.rfind('#') {
        let frag = &remaining[hash_pos + 1..];
        remaining = &remaining[..hash_pos];
        Some(frag.to_string())
    } else {
        None
    };

    // Extract query string
    let query_str = if let Some(q_pos) = remaining.find('?') {
        let qs = &remaining[q_pos + 1..];
        remaining = &remaining[..q_pos];
        Some(qs.to_string())
    } else {
        None
    };

    // Extract scheme
    if let Some(scheme_end) = remaining.find("://") {
        let scheme = &remaining[..scheme_end];
        result.insert(
            "scheme".to_string(),
            serde_json::Value::String(scheme.to_string()),
        );
        remaining = &remaining[scheme_end + 3..];
    }

    // Extract host and port
    let path_start = remaining.find('/').unwrap_or(remaining.len());
    let authority = &remaining[..path_start];
    let path = if path_start < remaining.len() {
        &remaining[path_start..]
    } else {
        "/"
    };

    // Check for port
    if let Some(colon_pos) = authority.rfind(':') {
        let potential_port = &authority[colon_pos + 1..];
        if let Ok(port) = potential_port.parse::<u16>() {
            result.insert(
                "host".to_string(),
                serde_json::Value::String(authority[..colon_pos].to_string()),
            );
            result.insert("port".to_string(), serde_json::json!(port));
        } else {
            result.insert(
                "host".to_string(),
                serde_json::Value::String(authority.to_string()),
            );
        }
    } else if !authority.is_empty() {
        result.insert(
            "host".to_string(),
            serde_json::Value::String(authority.to_string()),
        );
    }

    result.insert(
        "path".to_string(),
        serde_json::Value::String(path.to_string()),
    );

    // Parse query params
    if let Some(qs) = &query_str {
        let mut params = serde_json::Map::new();
        for pair in qs.split('&') {
            if pair.is_empty() {
                continue;
            }
            let mut parts = pair.splitn(2, '=');
            let key = parts.next().unwrap_or("");
            let value = parts.next().unwrap_or("");
            let decoded_key = percent_decode(key).unwrap_or_else(|_| key.to_string());
            let decoded_value = percent_decode(value).unwrap_or_else(|_| value.to_string());
            params.insert(decoded_key, serde_json::Value::String(decoded_value));
        }
        result.insert(
            "query".to_string(),
            serde_json::Value::Object(params),
        );
        result.insert(
            "query_string".to_string(),
            serde_json::Value::String(qs.clone()),
        );
    }

    if let Some(frag) = fragment {
        result.insert(
            "fragment".to_string(),
            serde_json::Value::String(frag),
        );
    }

    Ok(serde_json::Value::Object(result))
}

fn build_url(input: &serde_json::Value) -> Result<String, String> {
    let mut url = String::new();

    if let Some(scheme) = input.get("scheme").and_then(|v| v.as_str()) {
        url.push_str(scheme);
        url.push_str("://");
    }

    if let Some(host) = input.get("host").and_then(|v| v.as_str()) {
        url.push_str(host);
    }

    if let Some(port) = input.get("port").and_then(|v| v.as_u64()) {
        url.push(':');
        url.push_str(&port.to_string());
    }

    if let Some(path) = input.get("path").and_then(|v| v.as_str()) {
        if !path.starts_with('/') {
            url.push('/');
        }
        url.push_str(path);
    }

    if let Some(query) = input.get("query").and_then(|v| v.as_object()) {
        if !query.is_empty() {
            url.push('?');
            let pairs: Vec<String> = query
                .iter()
                .map(|(k, v)| {
                    let val = v.as_str().map(|s| s.to_string()).unwrap_or_else(|| {
                        if let Some(n) = v.as_f64() {
                            if n == n.floor() && n.abs() < i64::MAX as f64 {
                                format!("{}", n as i64)
                            } else {
                                n.to_string()
                            }
                        } else {
                            v.to_string()
                        }
                    });
                    format!("{}={}", percent_encode(k), percent_encode(&val))
                })
                .collect();
            url.push_str(&pairs.join("&"));
        }
    }

    if let Some(fragment) = input.get("fragment").and_then(|v| v.as_str()) {
        url.push('#');
        url.push_str(fragment);
    }

    Ok(url)
}

fn extract_text(input: &serde_json::Value, field: &str) -> Option<String> {
    if let Some(s) = input.as_str() {
        Some(s.to_string())
    } else if let Some(s) = input.get(field).and_then(|v| v.as_str()) {
        Some(s.to_string())
    } else if let Some(s) = input.get("url").and_then(|v| v.as_str()) {
        Some(s.to_string())
    } else if let Some(s) = input.get("text").and_then(|v| v.as_str()) {
        Some(s.to_string())
    } else {
        None
    }
}

#[async_trait::async_trait]
impl NodeExecutor for UrlEncodeNode {
    fn node_type(&self) -> &str {
        "url_encode"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "encode" | "decode" | "parse" | "build" => self.operation = op.to_string(),
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown operation '{}'. Use: encode, decode, parse, build",
                        other
                    )));
                }
            }
        }
        if let Some(f) = config.get("field").and_then(|v| v.as_str()) {
            self.field = f.to_string();
        }
        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let result = match self.operation.as_str() {
            "encode" => {
                let text = extract_text(&input, &self.field).ok_or_else(|| {
                    NodeError::ExecutionFailed("encode requires a string input".into())
                })?;
                let encoded = percent_encode(&text);
                serde_json::json!({ "encoded": encoded })
            }
            "decode" => {
                let text = extract_text(&input, &self.field).ok_or_else(|| {
                    NodeError::ExecutionFailed("decode requires a string input".into())
                })?;
                let decoded = percent_decode(&text).map_err(|e| {
                    NodeError::ExecutionFailed(format!("decode failed: {}", e))
                })?;
                serde_json::json!({ "decoded": decoded })
            }
            "parse" => {
                let url = extract_text(&input, &self.field).ok_or_else(|| {
                    NodeError::ExecutionFailed("parse requires a URL string input".into())
                })?;
                parse_url(&url).map_err(|e| {
                    NodeError::ExecutionFailed(format!("URL parse failed: {}", e))
                })?
            }
            "build" => {
                let url = build_url(&input).map_err(|e| {
                    NodeError::ExecutionFailed(format!("URL build failed: {}", e))
                })?;
                serde_json::json!({ "url": url })
            }
            _ => {
                return Err(NodeError::ExecutionFailed(format!(
                    "unknown operation: {}",
                    self.operation
                )));
            }
        };

        Ok(NodeOutput::data(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> ExecutionContext {
        ExecutionContext::new()
    }

    #[tokio::test]
    async fn test_encode_basic() {
        let mut node = UrlEncodeNode::new();
        node.init(&json!({"operation": "encode"})).await.unwrap();
        let input = json!({"text": "hello world"});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["encoded"], "hello%20world");
    }

    #[tokio::test]
    async fn test_decode_basic() {
        let mut node = UrlEncodeNode::new();
        node.init(&json!({"operation": "decode"})).await.unwrap();
        let input = json!({"text": "hello%20world"});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["decoded"], "hello world");
    }

    #[tokio::test]
    async fn test_parse_url() {
        let mut node = UrlEncodeNode::new();
        node.init(&json!({"operation": "parse"})).await.unwrap();
        let input = json!({"url": "https://example.com:8080/api/users?name=john&age=30#section"});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["scheme"], "https");
        assert_eq!(data["host"], "example.com");
        assert_eq!(data["port"], 8080);
        assert_eq!(data["path"], "/api/users");
        assert_eq!(data["query"]["name"], "john");
        assert_eq!(data["query"]["age"], "30");
        assert_eq!(data["fragment"], "section");
    }

    #[tokio::test]
    async fn test_build_url() {
        let mut node = UrlEncodeNode::new();
        node.init(&json!({"operation": "build"})).await.unwrap();
        let input = json!({
            "scheme": "https",
            "host": "example.com",
            "path": "/api",
            "query": {"key": "val"}
        });
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        let url = data["url"].as_str().unwrap();
        assert!(url.starts_with("https://example.com/api"));
        assert!(url.contains("key=val"));
    }

    #[tokio::test]
    async fn test_encode_special_chars() {
        let mut node = UrlEncodeNode::new();
        node.init(&json!({"operation": "encode"})).await.unwrap();
        let input = json!({"text": "a=1&b=2"});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        let encoded = data["encoded"].as_str().unwrap();
        assert!(encoded.contains("%3D"));
        assert!(encoded.contains("%26"));
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let mut node = UrlEncodeNode::new();
        let result = node.init(&json!({"operation": "invalid"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_roundtrip_encode_decode() {
        let original = "hello world! foo@bar.com";
        let encoded = percent_encode(original);
        let decoded = percent_decode(&encoded).unwrap();
        assert_eq!(decoded, original);
    }
}
