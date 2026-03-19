use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;
use tokio::sync::RwLock;

/// Queue node — in-memory FIFO queue for buffering and ordering data.
///
/// Provides a thread-safe queue that can buffer items between pipeline runs.
/// Useful for rate-limiting, batching, or ordering asynchronous data flows.
///
/// Config example:
/// ```json
/// {
///   "operation": "enqueue",
///   "max_size": 10000,
///   "queue_name": "default"
/// }
/// ```
///
/// Input operations:
/// ```json
/// { "value": { "task": "process" } }   // enqueue
/// {}                                    // dequeue
/// {}                                    // peek
/// {}                                    // size
/// {}                                    // clear
/// {}                                    // drain
/// ```
pub struct QueueNode {
    operation: String,
    max_size: usize,
    queue_name: String,
    queues: Arc<RwLock<HashMap<String, VecDeque<serde_json::Value>>>>,
}

impl Default for QueueNode {
    fn default() -> Self {
        Self::new()
    }
}

impl QueueNode {
    pub fn new() -> Self {
        Self {
            operation: "enqueue".to_string(),
            max_size: 10_000,
            queue_name: "default".to_string(),
            queues: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for QueueNode {
    fn node_type(&self) -> &str {
        "queue"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "enqueue" | "dequeue" | "peek" | "size" | "clear" | "drain" => {
                    self.operation = op.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown queue operation: '{}'. Use: enqueue, dequeue, peek, size, clear, drain",
                        other
                    )));
                }
            }
        }

        if let Some(max) = config.get("max_size").and_then(|v| v.as_u64()) {
            if max == 0 || max > 1_000_000 {
                return Err(NodeError::InvalidConfig(
                    "max_size must be between 1 and 1000000".into(),
                ));
            }
            self.max_size = max as usize;
        }

        if let Some(name) = config.get("queue_name").and_then(|v| v.as_str()) {
            if name.is_empty() {
                return Err(NodeError::InvalidConfig(
                    "queue_name must not be empty".into(),
                ));
            }
            self.queue_name = name.to_string();
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let op = input
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.operation);

        let queue_name = input
            .get("queue_name")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.queue_name)
            .to_string();

        match op {
            "enqueue" => {
                let value = input.get("value").cloned().unwrap_or_else(|| {
                    // If no explicit "value" field, use the entire input minus control fields
                    let mut data = input.clone();
                    if let Some(obj) = data.as_object_mut() {
                        obj.remove("operation");
                        obj.remove("queue_name");
                    }
                    data
                });

                let mut queues = self.queues.write().await;
                let queue = queues.entry(queue_name.clone()).or_default();

                if queue.len() >= self.max_size {
                    return Err(NodeError::ExecutionFailed(format!(
                        "queue '{}' is full ({} items). Increase max_size or drain the queue",
                        queue_name, self.max_size
                    )));
                }

                queue.push_back(value);
                let size = queue.len();

                Ok(NodeOutput::data(serde_json::json!({
                    "operation": "enqueue",
                    "queue_name": queue_name,
                    "success": true,
                    "size": size
                })))
            }

            "dequeue" => {
                let mut queues = self.queues.write().await;
                let queue = queues.entry(queue_name.clone()).or_default();

                if let Some(value) = queue.pop_front() {
                    Ok(NodeOutput::data(serde_json::json!({
                        "operation": "dequeue",
                        "queue_name": queue_name,
                        "value": value,
                        "size": queue.len()
                    })))
                } else {
                    Ok(NodeOutput::skip())
                }
            }

            "peek" => {
                let queues = self.queues.read().await;

                if let Some(queue) = queues.get(&queue_name) {
                    if let Some(value) = queue.front() {
                        Ok(NodeOutput::data(serde_json::json!({
                            "operation": "peek",
                            "queue_name": queue_name,
                            "value": value,
                            "size": queue.len()
                        })))
                    } else {
                        Ok(NodeOutput::skip())
                    }
                } else {
                    Ok(NodeOutput::skip())
                }
            }

            "size" => {
                let queues = self.queues.read().await;
                let size = queues.get(&queue_name).map_or(0, |q| q.len());

                Ok(NodeOutput::data(serde_json::json!({
                    "operation": "size",
                    "queue_name": queue_name,
                    "size": size
                })))
            }

            "clear" => {
                let mut queues = self.queues.write().await;
                let cleared = if let Some(queue) = queues.get_mut(&queue_name) {
                    let count = queue.len();
                    queue.clear();
                    count
                } else {
                    0
                };

                Ok(NodeOutput::data(serde_json::json!({
                    "operation": "clear",
                    "queue_name": queue_name,
                    "cleared": cleared
                })))
            }

            "drain" => {
                let mut queues = self.queues.write().await;
                let queue = queues.entry(queue_name.clone()).or_default();
                let items: Vec<serde_json::Value> = queue.drain(..).collect();
                let count = items.len();

                Ok(NodeOutput::data(serde_json::json!({
                    "operation": "drain",
                    "queue_name": queue_name,
                    "items": items,
                    "count": count
                })))
            }

            other => Err(NodeError::ExecutionFailed(format!(
                "unknown queue operation: '{}'. Use: enqueue, dequeue, peek, size, clear, drain",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_enqueue_and_dequeue() {
        let mut node = QueueNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({})).await.unwrap();

        // Enqueue items
        node.execute(
            serde_json::json!({"operation": "enqueue", "value": "first"}),
            &ctx,
        )
        .await
        .unwrap();
        node.execute(
            serde_json::json!({"operation": "enqueue", "value": "second"}),
            &ctx,
        )
        .await
        .unwrap();

        // Dequeue — FIFO order
        let result = node
            .execute(serde_json::json!({"operation": "dequeue"}), &ctx)
            .await
            .unwrap();
        assert_eq!(result.ports["default"]["value"], "first");

        let result = node
            .execute(serde_json::json!({"operation": "dequeue"}), &ctx)
            .await
            .unwrap();
        assert_eq!(result.ports["default"]["value"], "second");
    }

    #[tokio::test]
    async fn test_dequeue_empty_returns_skip() {
        let mut node = QueueNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let result = node
            .execute(serde_json::json!({"operation": "dequeue"}), &ctx)
            .await
            .unwrap();
        assert!(result.ports.is_empty()); // Skip has no ports
    }

    #[tokio::test]
    async fn test_peek() {
        let mut node = QueueNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({})).await.unwrap();

        node.execute(
            serde_json::json!({"operation": "enqueue", "value": "item"}),
            &ctx,
        )
        .await
        .unwrap();

        let result = node
            .execute(serde_json::json!({"operation": "peek"}), &ctx)
            .await
            .unwrap();
        assert_eq!(result.ports["default"]["value"], "item");

        // Still there after peek
        let result = node
            .execute(serde_json::json!({"operation": "size"}), &ctx)
            .await
            .unwrap();
        assert_eq!(result.ports["default"]["size"], 1);
    }

    #[tokio::test]
    async fn test_drain() {
        let mut node = QueueNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({})).await.unwrap();

        node.execute(
            serde_json::json!({"operation": "enqueue", "value": 1}),
            &ctx,
        )
        .await
        .unwrap();
        node.execute(
            serde_json::json!({"operation": "enqueue", "value": 2}),
            &ctx,
        )
        .await
        .unwrap();
        node.execute(
            serde_json::json!({"operation": "enqueue", "value": 3}),
            &ctx,
        )
        .await
        .unwrap();

        let result = node
            .execute(serde_json::json!({"operation": "drain"}), &ctx)
            .await
            .unwrap();
        assert_eq!(result.ports["default"]["count"], 3);
        assert_eq!(
            result.ports["default"]["items"],
            serde_json::json!([1, 2, 3])
        );

        // Queue should be empty now
        let result = node
            .execute(serde_json::json!({"operation": "size"}), &ctx)
            .await
            .unwrap();
        assert_eq!(result.ports["default"]["size"], 0);
    }

    #[tokio::test]
    async fn test_max_size() {
        let mut node = QueueNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({"max_size": 2}))
            .await
            .unwrap();

        node.execute(
            serde_json::json!({"operation": "enqueue", "value": 1}),
            &ctx,
        )
        .await
        .unwrap();
        node.execute(
            serde_json::json!({"operation": "enqueue", "value": 2}),
            &ctx,
        )
        .await
        .unwrap();

        let result = node
            .execute(
                serde_json::json!({"operation": "enqueue", "value": 3}),
                &ctx,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let mut node = QueueNode::new();
        let result = node
            .init(&serde_json::json!({"operation": "invalid"}))
            .await;
        assert!(result.is_err());
    }
}
