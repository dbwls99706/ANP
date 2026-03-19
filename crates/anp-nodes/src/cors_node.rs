use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// CORS node — adds CORS headers to HTTP responses.
///
/// Config:
/// ```json
/// {
///   "allowed_origins": ["*"],
///   "allowed_methods": ["GET", "POST", "PUT", "DELETE"],
///   "allowed_headers": ["Content-Type", "Authorization"],
///   "max_age": 86400
/// }
/// ```
///
/// Takes input and merges it with a `_cors_headers` field containing
/// the appropriate CORS response headers.
pub struct CorsNode {
    allowed_origins: Vec<String>,
    allowed_methods: Vec<String>,
    allowed_headers: Vec<String>,
    max_age: u64,
}

impl Default for CorsNode {
    fn default() -> Self {
        Self::new()
    }
}

impl CorsNode {
    pub fn new() -> Self {
        Self {
            allowed_origins: vec!["*".to_string()],
            allowed_methods: vec![
                "GET".to_string(),
                "POST".to_string(),
                "PUT".to_string(),
                "DELETE".to_string(),
            ],
            allowed_headers: vec![
                "Content-Type".to_string(),
                "Authorization".to_string(),
            ],
            max_age: 86400,
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for CorsNode {
    fn node_type(&self) -> &str {
        "cors"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(origins) = config.get("allowed_origins").and_then(|v| v.as_array()) {
            self.allowed_origins = origins
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }
        if let Some(methods) = config.get("allowed_methods").and_then(|v| v.as_array()) {
            self.allowed_methods = methods
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }
        if let Some(headers) = config.get("allowed_headers").and_then(|v| v.as_array()) {
            self.allowed_headers = headers
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }
        if let Some(age) = config.get("max_age").and_then(|v| v.as_u64()) {
            self.max_age = age;
        }
        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let cors_headers = serde_json::json!({
            "Access-Control-Allow-Origin": self.allowed_origins.join(", "),
            "Access-Control-Allow-Methods": self.allowed_methods.join(", "),
            "Access-Control-Allow-Headers": self.allowed_headers.join(", "),
            "Access-Control-Max-Age": self.max_age.to_string()
        });

        let result = if let serde_json::Value::Object(mut map) = input {
            map.insert("_cors_headers".to_string(), cors_headers);
            serde_json::Value::Object(map)
        } else {
            serde_json::json!({
                "data": input,
                "_cors_headers": cors_headers
            })
        };

        Ok(NodeOutput::data(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_default_cors_headers() {
        let mut node = CorsNode::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"message": "hello"}), &ctx)
            .await
            .unwrap();

        let result = &output.ports["default"];
        assert_eq!(result["message"], "hello");
        assert_eq!(
            result["_cors_headers"]["Access-Control-Allow-Origin"],
            "*"
        );
        assert!(result["_cors_headers"]["Access-Control-Allow-Methods"]
            .as_str()
            .unwrap()
            .contains("GET"));
    }

    #[tokio::test]
    async fn test_custom_cors_config() {
        let mut node = CorsNode::new();
        node.init(&serde_json::json!({
            "allowed_origins": ["https://example.com"],
            "allowed_methods": ["GET", "POST"],
            "allowed_headers": ["X-Custom"],
            "max_age": 3600
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({}), &ctx)
            .await
            .unwrap();

        let headers = &output.ports["default"]["_cors_headers"];
        assert_eq!(headers["Access-Control-Allow-Origin"], "https://example.com");
        assert_eq!(headers["Access-Control-Allow-Methods"], "GET, POST");
        assert_eq!(headers["Access-Control-Allow-Headers"], "X-Custom");
        assert_eq!(headers["Access-Control-Max-Age"], "3600");
    }

    #[tokio::test]
    async fn test_non_object_input() {
        let mut node = CorsNode::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!("string input"), &ctx)
            .await
            .unwrap();

        let result = &output.ports["default"];
        assert_eq!(result["data"], "string input");
        assert!(result["_cors_headers"].is_object());
    }

    #[test]
    fn test_node_type_and_kind() {
        let node = CorsNode::new();
        assert_eq!(node.node_type(), "cors");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
