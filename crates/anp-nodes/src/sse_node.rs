use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// SSE node — Server-Sent Events formatter.
///
/// Config:
/// ```json
/// {
///   "event_type": "message",
///   "retry_ms": 3000
/// }
/// ```
///
/// Kind: Processor — takes input data and formats it as an SSE event string.
/// Works with `http_server` to create SSE endpoints by setting the
/// `_content_type` field to `text/event-stream`.
///
/// Output:
/// ```json
/// {
///   "sse": "event: message\ndata: {json}\nretry: 3000\n\n",
///   "_content_type": "text/event-stream",
///   "data": <original_input>
/// }
/// ```
pub struct SseNode {
    event_type: String,
    retry_ms: u64,
}

impl Default for SseNode {
    fn default() -> Self {
        Self::new()
    }
}

impl SseNode {
    pub fn new() -> Self {
        Self {
            event_type: "message".to_string(),
            retry_ms: 3000,
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for SseNode {
    fn node_type(&self) -> &str {
        "sse"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(et) = config.get("event_type").and_then(|v| v.as_str()) {
            self.event_type = et.to_string();
        }
        if let Some(r) = config.get("retry_ms").and_then(|v| v.as_u64()) {
            self.retry_ms = r;
        }
        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        // Serialize the input data as a JSON string for the SSE data field
        let data_str = match &input {
            serde_json::Value::String(s) => s.clone(),
            other => serde_json::to_string(other).unwrap_or_default(),
        };

        // Format as SSE event
        let sse = format!(
            "event: {}\ndata: {}\nretry: {}\n\n",
            self.event_type, data_str, self.retry_ms
        );

        Ok(NodeOutput::data(serde_json::json!({
            "sse": sse,
            "_content_type": "text/event-stream",
            "data": input
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_default_sse_format() {
        let mut node = SseNode::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"message": "hello"}), &ctx)
            .await
            .unwrap();

        let result = &output.ports["default"];
        let sse = result["sse"].as_str().unwrap();
        assert!(sse.starts_with("event: message\n"));
        assert!(sse.contains("data: "));
        assert!(sse.contains("retry: 3000\n"));
        assert!(sse.ends_with("\n\n"));
        assert_eq!(result["_content_type"], "text/event-stream");
        assert_eq!(result["data"]["message"], "hello");
    }

    #[tokio::test]
    async fn test_custom_event_type() {
        let mut node = SseNode::new();
        node.init(&serde_json::json!({
            "event_type": "update",
            "retry_ms": 5000
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"count": 42}), &ctx)
            .await
            .unwrap();

        let result = &output.ports["default"];
        let sse = result["sse"].as_str().unwrap();
        assert!(sse.starts_with("event: update\n"));
        assert!(sse.contains("retry: 5000\n"));
    }

    #[tokio::test]
    async fn test_string_input() {
        let mut node = SseNode::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!("plain text"), &ctx)
            .await
            .unwrap();

        let result = &output.ports["default"];
        let sse = result["sse"].as_str().unwrap();
        assert!(sse.contains("data: plain text\n"));
        assert_eq!(result["data"], "plain text");
    }

    #[tokio::test]
    async fn test_numeric_input() {
        let mut node = SseNode::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::json!(42), &ctx).await.unwrap();

        let result = &output.ports["default"];
        let sse = result["sse"].as_str().unwrap();
        assert!(sse.contains("data: 42\n"));
    }

    #[test]
    fn test_node_type_and_kind() {
        let node = SseNode::new();
        assert_eq!(node.node_type(), "sse");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
