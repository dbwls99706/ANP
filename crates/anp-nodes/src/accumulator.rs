use std::sync::Arc;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;
use tokio::sync::RwLock;

/// AccumulatorNode — collects multiple inputs into an array.
///
/// Config example:
/// ```json
/// {
///   "target_count": 10,
///   "flush_on": "count"
/// }
/// ```
///
/// Each call to execute() adds the input to an internal buffer.
/// When the buffer reaches `target_count`, it emits the collected array
/// and clears the buffer. If not yet full, returns `NodeOutput::skip()`.
///
/// Output: `{ "items": [...], "count": n }`
pub struct AccumulatorNode {
    target_count: usize,
    buffer: Arc<RwLock<Vec<serde_json::Value>>>,
}

impl Default for AccumulatorNode {
    fn default() -> Self {
        Self::new()
    }
}

impl AccumulatorNode {
    pub fn new() -> Self {
        Self {
            target_count: 10,
            buffer: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for AccumulatorNode {
    fn node_type(&self) -> &str {
        "accumulator"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(count) = config.get("target_count").and_then(|v| v.as_u64()) {
            if count == 0 {
                return Err(anp_core::node::traits::NodeError::InvalidConfig(
                    "target_count must be greater than 0".into(),
                ));
            }
            self.target_count = count as usize;
        }

        // flush_on is accepted for future use (timeout mode), but currently
        // only "count" mode is implemented
        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let mut buf = self.buffer.write().await;
        buf.push(input);

        if buf.len() >= self.target_count {
            let items: Vec<serde_json::Value> = buf.drain(..).collect();
            let count = items.len();
            Ok(NodeOutput::data(serde_json::json!({
                "items": items,
                "count": count
            }))
            .with_log(format!("flushed {} accumulated items", count)))
        } else {
            let current = buf.len();
            Ok(NodeOutput::skip()
                .with_log(format!("buffered {}/{} items", current, self.target_count)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_accumulates_until_target() {
        let mut node = AccumulatorNode::new();
        node.init(&serde_json::json!({"target_count": 3}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();

        // First two should skip
        let out1 = node.execute(serde_json::json!(1), &ctx).await.unwrap();
        assert_eq!(out1.status, anp_core::node::traits::NodeStatus::Skip);

        let out2 = node.execute(serde_json::json!(2), &ctx).await.unwrap();
        assert_eq!(out2.status, anp_core::node::traits::NodeStatus::Skip);

        // Third should flush
        let out3 = node.execute(serde_json::json!(3), &ctx).await.unwrap();
        assert_eq!(out3.status, anp_core::node::traits::NodeStatus::Ok);

        let result = &out3.ports["default"];
        assert_eq!(result["count"], 3);
        assert_eq!(result["items"], serde_json::json!([1, 2, 3]));
    }

    #[tokio::test]
    async fn test_buffer_clears_after_flush() {
        let mut node = AccumulatorNode::new();
        node.init(&serde_json::json!({"target_count": 2}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();

        // Fill and flush first batch
        node.execute(serde_json::json!("a"), &ctx).await.unwrap();
        let out = node.execute(serde_json::json!("b"), &ctx).await.unwrap();
        assert_eq!(out.ports["default"]["count"], 2);

        // Next input should start a new batch and skip
        let out2 = node.execute(serde_json::json!("c"), &ctx).await.unwrap();
        assert_eq!(out2.status, anp_core::node::traits::NodeStatus::Skip);
    }

    #[tokio::test]
    async fn test_target_count_one() {
        let mut node = AccumulatorNode::new();
        node.init(&serde_json::json!({"target_count": 1}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"val": 42}), &ctx)
            .await
            .unwrap();

        assert_eq!(output.status, anp_core::node::traits::NodeStatus::Ok);
        assert_eq!(output.ports["default"]["count"], 1);
    }

    #[tokio::test]
    async fn test_invalid_target_count_zero() {
        let mut node = AccumulatorNode::new();
        let result = node.init(&serde_json::json!({"target_count": 0})).await;
        assert!(result.is_err());
    }
}
