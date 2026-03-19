use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Request ID / correlation ID node for distributed tracing.
/// Generates unique request IDs and propagates them through the pipeline.
/// Supports custom prefixes, header extraction, and ID formats.
pub struct RequestIdNode {
    prefix: String,
    header_name: String,
    counter: AtomicU64,
}

impl RequestIdNode {
    pub fn new() -> Self {
        Self {
            prefix: "req".to_string(),
            header_name: "x-request-id".to_string(),
            counter: AtomicU64::new(0),
        }
    }

    fn generate_id(&self) -> String {
        let count = self.counter.fetch_add(1, Ordering::Relaxed);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let ts = now.as_micros() as u64;
        // Mix timestamp and counter for uniqueness
        let mixed = ts ^ (count.wrapping_mul(2654435769));
        format!("{}-{:016x}", self.prefix, mixed)
    }
}

impl Default for RequestIdNode {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NodeExecutor for RequestIdNode {
    fn node_type(&self) -> &str {
        "request_id"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &Value) -> NodeResult<()> {
        if let Some(prefix) = config.get("prefix").and_then(|v| v.as_str()) {
            self.prefix = prefix.to_string();
        }
        if let Some(header) = config.get("header_name").and_then(|v| v.as_str()) {
            self.header_name = header.to_string();
        }
        Ok(())
    }

    async fn execute(&self, input: Value, _ctx: &ExecutionContext) -> NodeResult<NodeOutput> {
        // Check if request already has an ID (propagated from upstream)
        let request_id = input
            .get("_request_id")
            .or_else(|| input.get(&self.header_name))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.generate_id());

        // Merge request ID into the input data
        let mut output = if input.is_object() {
            input.clone()
        } else {
            json!({"_input": input})
        };

        if let Some(obj) = output.as_object_mut() {
            obj.insert("_request_id".to_string(), json!(&request_id));
            obj.insert(
                "_response_headers".to_string(),
                json!({
                    &self.header_name: &request_id,
                }),
            );
        }

        Ok(NodeOutput::data(output)
            .with_log(format!("request_id={}", request_id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ExecutionContext {
        ExecutionContext::new()
    }

    #[tokio::test]
    async fn test_generate_id() {
        let mut node = RequestIdNode::new();
        node.init(&json!({})).await.unwrap();

        let out1 = node.execute(json!({"path": "/api"}), &ctx()).await.unwrap();
        let out2 = node.execute(json!({"path": "/api"}), &ctx()).await.unwrap();

        let id1 = out1.ports["default"]["_request_id"].as_str().unwrap();
        let id2 = out2.ports["default"]["_request_id"].as_str().unwrap();

        assert!(id1.starts_with("req-"));
        assert!(id2.starts_with("req-"));
        assert_ne!(id1, id2); // Unique
    }

    #[tokio::test]
    async fn test_propagate_existing_id() {
        let mut node = RequestIdNode::new();
        node.init(&json!({})).await.unwrap();

        let output = node
            .execute(json!({"_request_id": "existing-id-123"}), &ctx())
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["_request_id"], "existing-id-123");
    }

    #[tokio::test]
    async fn test_custom_prefix() {
        let mut node = RequestIdNode::new();
        node.init(&json!({"prefix": "trace"})).await.unwrap();

        let output = node.execute(json!({}), &ctx()).await.unwrap();
        let id = output.ports["default"]["_request_id"].as_str().unwrap();
        assert!(id.starts_with("trace-"));
    }

    #[tokio::test]
    async fn test_response_headers() {
        let mut node = RequestIdNode::new();
        node.init(&json!({"header_name": "x-correlation-id"}))
            .await
            .unwrap();

        let output = node.execute(json!({}), &ctx()).await.unwrap();
        let headers = &output.ports["default"]["_response_headers"];
        assert!(headers["x-correlation-id"].as_str().is_some());
    }
}
