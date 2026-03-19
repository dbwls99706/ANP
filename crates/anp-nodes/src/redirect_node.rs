use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Redirect node — generates HTTP redirect responses for use with http_server.
///
/// Produces a structured response that http_server can interpret as a redirect,
/// including the Location header and appropriate status code.
///
/// Config example:
/// ```json
/// {
///   "url": "https://example.com/{{path}}",
///   "status": 302,
///   "preserve_query": true
/// }
/// ```
///
/// Input: JSON data used to resolve `{{placeholders}}` in the URL.
/// Output: `{ "_redirect": "url", "_status": 302, "_headers": { "Location": "url" } }`
pub struct RedirectNode {
    url: String,
    status: u16,
    preserve_query: bool,
}

impl Default for RedirectNode {
    fn default() -> Self {
        Self::new()
    }
}

impl RedirectNode {
    pub fn new() -> Self {
        Self {
            url: String::new(),
            status: 302,
            preserve_query: true,
        }
    }
}

/// Resolve a dot-path like "user.name" on a JSON value.
fn resolve_path(value: &serde_json::Value, path: &str) -> Option<String> {
    let mut current = value;

    for part in path.split('.') {
        match current {
            serde_json::Value::Object(map) => {
                current = map.get(part)?;
            }
            serde_json::Value::Array(arr) => {
                let idx: usize = part.parse().ok()?;
                current = arr.get(idx)?;
            }
            _ => return None,
        }
    }

    match current {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Null => Some("null".to_string()),
        other => Some(other.to_string()),
    }
}

/// Replace `{{placeholder}}` tokens in a string with values from JSON data.
fn resolve_placeholders(template: &str, data: &serde_json::Value) -> String {
    let mut result = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '{' && chars.peek() == Some(&'{') {
            chars.next(); // consume second '{'

            let mut key = String::new();
            let mut found_close = false;
            while let Some(c) = chars.next() {
                if c == '}' && chars.peek() == Some(&'}') {
                    chars.next(); // consume second '}'
                    found_close = true;
                    break;
                }
                key.push(c);
            }

            let key = key.trim();
            if found_close && !key.is_empty() {
                match resolve_path(data, key) {
                    Some(val) => result.push_str(&val),
                    None => {
                        result.push_str("{{");
                        result.push_str(key);
                        result.push_str("}}");
                    }
                }
            } else {
                result.push_str("{{");
                result.push_str(key);
            }
        } else {
            result.push(ch);
        }
    }

    result
}

fn is_valid_redirect_status(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

#[async_trait::async_trait]
impl NodeExecutor for RedirectNode {
    fn node_type(&self) -> &str {
        "redirect"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(url) = config.get("url").and_then(|v| v.as_str()) {
            if url.is_empty() {
                return Err(NodeError::InvalidConfig(
                    "url must not be empty".into(),
                ));
            }
            self.url = url.to_string();
        }

        if let Some(status) = config.get("status").and_then(|v| v.as_u64()) {
            let status = status as u16;
            if !is_valid_redirect_status(status) {
                return Err(NodeError::InvalidConfig(format!(
                    "invalid redirect status: {}. Use: 301, 302, 303, 307, 308",
                    status
                )));
            }
            self.status = status;
        }

        if let Some(preserve) = config.get("preserve_query").and_then(|v| v.as_bool()) {
            self.preserve_query = preserve;
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        // Allow input to override URL at runtime
        let url_template = input
            .get("url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.url.clone());

        if url_template.is_empty() {
            return Err(NodeError::MissingInput(
                "no redirect URL configured or provided in input".into(),
            ));
        }

        // Resolve placeholders in the URL
        let mut resolved_url = resolve_placeholders(&url_template, &input);

        // Allow input to override status
        let status = input
            .get("status")
            .and_then(|v| v.as_u64())
            .map(|s| s as u16)
            .unwrap_or(self.status);

        // Append query parameters if preserve_query is true
        if self.preserve_query {
            if let Some(query) = input.get("_query").and_then(|v| v.as_object()) {
                if !query.is_empty() {
                    let query_string: Vec<String> = query
                        .iter()
                        .filter_map(|(k, v)| {
                            v.as_str().map(|val| {
                                format!("{}={}", k, val)
                            })
                        })
                        .collect();

                    if !query_string.is_empty() {
                        let separator = if resolved_url.contains('?') { "&" } else { "?" };
                        resolved_url = format!("{}{}{}", resolved_url, separator, query_string.join("&"));
                    }
                }
            }
        }

        let mut headers = serde_json::Map::new();
        headers.insert(
            "Location".to_string(),
            serde_json::Value::String(resolved_url.clone()),
        );

        Ok(NodeOutput::data(serde_json::json!({
            "_redirect": resolved_url,
            "_status": status,
            "_headers": headers,
            "_content_type": "text/html"
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_redirect() {
        let mut node = RedirectNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({
            "url": "https://example.com",
            "status": 302
        }))
        .await
        .unwrap();

        let result = node
            .execute(serde_json::json!({}), &ctx)
            .await
            .unwrap();

        let data = &result.ports["default"];
        assert_eq!(data["_redirect"], "https://example.com");
        assert_eq!(data["_status"], 302);
        assert_eq!(data["_headers"]["Location"], "https://example.com");
    }

    #[tokio::test]
    async fn test_placeholder_resolution() {
        let mut node = RedirectNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({
            "url": "https://example.com/users/{{user_id}}/profile"
        }))
        .await
        .unwrap();

        let result = node
            .execute(serde_json::json!({"user_id": "42"}), &ctx)
            .await
            .unwrap();

        assert_eq!(
            result.ports["default"]["_redirect"],
            "https://example.com/users/42/profile"
        );
    }

    #[tokio::test]
    async fn test_preserve_query() {
        let mut node = RedirectNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({
            "url": "https://example.com/search",
            "preserve_query": true
        }))
        .await
        .unwrap();

        let result = node
            .execute(
                serde_json::json!({
                    "_query": { "q": "hello" }
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert_eq!(
            result.ports["default"]["_redirect"],
            "https://example.com/search?q=hello"
        );
    }

    #[tokio::test]
    async fn test_permanent_redirect() {
        let mut node = RedirectNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({
            "url": "https://new-site.com",
            "status": 301
        }))
        .await
        .unwrap();

        let result = node
            .execute(serde_json::json!({}), &ctx)
            .await
            .unwrap();

        assert_eq!(result.ports["default"]["_status"], 301);
    }

    #[tokio::test]
    async fn test_invalid_status() {
        let mut node = RedirectNode::new();
        let result = node
            .init(&serde_json::json!({"url": "https://example.com", "status": 200}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_missing_url() {
        let mut node = RedirectNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let result = node.execute(serde_json::json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_input_override_url() {
        let mut node = RedirectNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({
            "url": "https://default.com"
        }))
        .await
        .unwrap();

        let result = node
            .execute(serde_json::json!({"url": "https://override.com"}), &ctx)
            .await
            .unwrap();

        assert_eq!(
            result.ports["default"]["_redirect"],
            "https://override.com"
        );
    }
}
