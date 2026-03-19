use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;
use tokio::sync::RwLock;

/// BatchNode — collects inputs into batches and emits when full or flushed.
///
/// Config example:
/// ```json
/// {
///   "batch_size": 5,
///   "flush_field": "_flush"
/// }
/// ```
///
/// Collects inputs into a buffer. When the buffer reaches `batch_size`,
/// emits the batch and clears the buffer. If the input contains a field
/// matching `flush_field` set to true, emits the current partial batch.
///
/// Output: `{ "batch": [...], "batch_size": N, "batch_number": M, "is_full": true/false }`
pub struct BatchNode {
    batch_size: usize,
    flush_field: Option<String>,
    buffer: Arc<RwLock<Vec<serde_json::Value>>>,
    batch_number: Arc<AtomicU64>,
}

impl Default for BatchNode {
    fn default() -> Self {
        Self::new()
    }
}

impl BatchNode {
    pub fn new() -> Self {
        Self {
            batch_size: 10,
            flush_field: None,
            buffer: Arc::new(RwLock::new(Vec::new())),
            batch_number: Arc::new(AtomicU64::new(0)),
        }
    }

    fn make_output(&self, items: Vec<serde_json::Value>, is_full: bool) -> NodeOutput {
        let batch_num = self.batch_number.fetch_add(1, Ordering::SeqCst) + 1;
        let size = items.len();
        NodeOutput::data(serde_json::json!({
            "batch": items,
            "batch_size": size,
            "batch_number": batch_num,
            "is_full": is_full,
        }))
    }
}

#[async_trait::async_trait]
impl NodeExecutor for BatchNode {
    fn node_type(&self) -> &str {
        "batch"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(size) = config.get("batch_size").and_then(|v| v.as_u64()) {
            if size == 0 {
                return Err(NodeError::InvalidConfig(
                    "batch_size must be greater than 0".into(),
                ));
            }
            self.batch_size = size as usize;
        } else {
            return Err(NodeError::InvalidConfig("batch_size is required".into()));
        }

        if let Some(field) = config.get("flush_field").and_then(|v| v.as_str()) {
            self.flush_field = Some(field.to_string());
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        // Check if flush is triggered
        let flush = if let Some(ref field) = self.flush_field {
            input.get(field).and_then(|v| v.as_bool()).unwrap_or(false)
        } else {
            false
        };

        let mut buffer = self.buffer.write().await;
        buffer.push(input);

        if flush {
            let items: Vec<serde_json::Value> = buffer.drain(..).collect();
            return Ok(self.make_output(items, false));
        }

        if buffer.len() >= self.batch_size {
            let items: Vec<serde_json::Value> = buffer.drain(..).collect();
            return Ok(self.make_output(items, true));
        }

        Ok(NodeOutput::skip())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ExecutionContext {
        ExecutionContext::default()
    }

    #[tokio::test]
    async fn test_batch_collects_and_emits() {
        let mut node = BatchNode::new();
        node.init(&serde_json::json!({ "batch_size": 3 }))
            .await
            .unwrap();

        // First two should skip
        let r1 = node
            .execute(serde_json::json!({"a": 1}), &ctx())
            .await
            .unwrap();
        assert!(r1.ports.is_empty()); // skip

        let r2 = node
            .execute(serde_json::json!({"a": 2}), &ctx())
            .await
            .unwrap();
        assert!(r2.ports.is_empty());

        // Third should emit
        let r3 = node
            .execute(serde_json::json!({"a": 3}), &ctx())
            .await
            .unwrap();
        let out = r3.ports.get("default").unwrap();
        assert_eq!(out["batch_size"], 3);
        assert_eq!(out["is_full"], true);
        assert_eq!(out["batch_number"], 1);
        assert_eq!(out["batch"].as_array().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn test_batch_size_zero_invalid() {
        let mut node = BatchNode::new();
        let result = node.init(&serde_json::json!({ "batch_size": 0 })).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_batch_size_required() {
        let mut node = BatchNode::new();
        let result = node.init(&serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_flush_field_emits_partial() {
        let mut node = BatchNode::new();
        node.init(&serde_json::json!({ "batch_size": 10, "flush_field": "_flush" }))
            .await
            .unwrap();

        let r1 = node
            .execute(serde_json::json!({"x": 1}), &ctx())
            .await
            .unwrap();
        assert!(r1.ports.is_empty());

        let r2 = node
            .execute(serde_json::json!({"x": 2, "_flush": true}), &ctx())
            .await
            .unwrap();
        let out = r2.ports.get("default").unwrap();
        assert_eq!(out["batch_size"], 2);
        assert_eq!(out["is_full"], false);
    }

    #[tokio::test]
    async fn test_batch_number_increments() {
        let mut node = BatchNode::new();
        node.init(&serde_json::json!({ "batch_size": 1 }))
            .await
            .unwrap();

        let r1 = node
            .execute(serde_json::json!({"v": 1}), &ctx())
            .await
            .unwrap();
        assert_eq!(r1.ports.get("default").unwrap()["batch_number"], 1);

        let r2 = node
            .execute(serde_json::json!({"v": 2}), &ctx())
            .await
            .unwrap();
        assert_eq!(r2.ports.get("default").unwrap()["batch_number"], 2);
    }

    #[tokio::test]
    async fn test_node_type_and_kind() {
        let node = BatchNode::new();
        assert_eq!(node.node_type(), "batch");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
