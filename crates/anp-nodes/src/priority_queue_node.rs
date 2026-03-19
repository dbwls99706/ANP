use std::sync::Arc;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;
use tokio::sync::RwLock;

/// PriorityQueueNode — a priority queue where higher priority items dequeue first.
///
/// Config example:
/// ```json
/// {
///   "operation": "enqueue",
///   "priority_field": "priority",
///   "max_size": 100
/// }
/// ```
///
/// Operations:
/// - `enqueue`: adds item sorted by priority
/// - `dequeue`: removes and returns highest priority item
/// - `peek`: returns highest priority item without removing
/// - `size`: returns current queue size
/// - `clear`: clears the queue
pub struct PriorityQueueNode {
    operation: String,
    priority_field: String,
    max_size: usize,
    queue: Arc<RwLock<Vec<(i64, serde_json::Value)>>>,
}

impl Default for PriorityQueueNode {
    fn default() -> Self {
        Self::new()
    }
}

impl PriorityQueueNode {
    pub fn new() -> Self {
        Self {
            operation: "enqueue".to_string(),
            priority_field: "priority".to_string(),
            max_size: 100,
            queue: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

/// Resolve a dot-path on a JSON value.
fn resolve_field(value: &serde_json::Value, path: &str) -> Option<serde_json::Value> {
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
    Some(current.clone())
}

#[async_trait::async_trait]
impl NodeExecutor for PriorityQueueNode {
    fn node_type(&self) -> &str {
        "priority_queue"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "enqueue" | "dequeue" | "peek" | "size" | "clear" => {
                    self.operation = op.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "invalid operation '{}': must be enqueue, dequeue, peek, size, or clear",
                        other
                    )));
                }
            }
        }

        if let Some(field) = config.get("priority_field").and_then(|v| v.as_str()) {
            self.priority_field = field.to_string();
        }

        if let Some(max) = config.get("max_size").and_then(|v| v.as_u64()) {
            if max == 0 {
                return Err(NodeError::InvalidConfig(
                    "max_size must be greater than 0".into(),
                ));
            }
            self.max_size = max as usize;
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        match self.operation.as_str() {
            "enqueue" => {
                let priority = resolve_field(&input, &self.priority_field)
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);

                let mut queue = self.queue.write().await;

                if queue.len() >= self.max_size {
                    return Err(NodeError::ExecutionFailed(format!(
                        "priority queue is full (max_size: {})",
                        self.max_size
                    )));
                }

                // Insert in sorted order (ascending), so highest priority is at the end
                let pos = queue
                    .binary_search_by(|(p, _)| p.cmp(&priority))
                    .unwrap_or_else(|e| e);
                queue.insert(pos, (priority, input));

                let size = queue.len();
                Ok(NodeOutput::data(serde_json::json!({
                    "queued": true,
                    "size": size,
                    "priority": priority,
                })))
            }
            "dequeue" => {
                let mut queue = self.queue.write().await;
                if queue.is_empty() {
                    return Ok(NodeOutput::skip());
                }
                // Pop highest priority (last element since sorted ascending)
                let (priority, item) = queue.pop().unwrap();
                let size = queue.len();
                Ok(NodeOutput::data(serde_json::json!({
                    "item": item,
                    "priority": priority,
                    "remaining": size,
                })))
            }
            "peek" => {
                let queue = self.queue.read().await;
                if queue.is_empty() {
                    return Ok(NodeOutput::skip());
                }
                let (priority, item) = queue.last().unwrap();
                let size = queue.len();
                Ok(NodeOutput::data(serde_json::json!({
                    "item": item,
                    "priority": priority,
                    "size": size,
                })))
            }
            "size" => {
                let queue = self.queue.read().await;
                Ok(NodeOutput::data(serde_json::json!({
                    "size": queue.len(),
                    "max_size": self.max_size,
                })))
            }
            "clear" => {
                let mut queue = self.queue.write().await;
                let cleared = queue.len();
                queue.clear();
                Ok(NodeOutput::data(serde_json::json!({
                    "cleared": cleared,
                })))
            }
            _ => Err(NodeError::ExecutionFailed(format!(
                "unknown operation: {}",
                self.operation
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ExecutionContext {
        ExecutionContext::default()
    }

    #[tokio::test]
    async fn test_enqueue_and_dequeue() {
        let enqueue_node = {
            let mut n = PriorityQueueNode::new();
            n.queue = Arc::new(RwLock::new(Vec::new()));
            n.init(&serde_json::json!({ "operation": "enqueue", "priority_field": "p" }))
                .await
                .unwrap();
            n
        };

        // Share the queue across operations
        let shared_queue = enqueue_node.queue.clone();

        enqueue_node
            .execute(serde_json::json!({"p": 1, "data": "low"}), &ctx())
            .await
            .unwrap();
        enqueue_node
            .execute(serde_json::json!({"p": 10, "data": "high"}), &ctx())
            .await
            .unwrap();
        enqueue_node
            .execute(serde_json::json!({"p": 5, "data": "mid"}), &ctx())
            .await
            .unwrap();

        // Dequeue should return highest priority first
        let mut dequeue_node = PriorityQueueNode::new();
        dequeue_node.queue = shared_queue;
        dequeue_node
            .init(&serde_json::json!({ "operation": "dequeue" }))
            .await
            .unwrap();

        let r = dequeue_node
            .execute(serde_json::json!({}), &ctx())
            .await
            .unwrap();
        let out = r.ports.get("default").unwrap();
        assert_eq!(out["priority"], 10);
        assert_eq!(out["item"]["data"], "high");
    }

    #[tokio::test]
    async fn test_dequeue_empty_skips() {
        let mut node = PriorityQueueNode::new();
        node.init(&serde_json::json!({ "operation": "dequeue" }))
            .await
            .unwrap();

        let r = node.execute(serde_json::json!({}), &ctx()).await.unwrap();
        assert!(r.ports.is_empty());
    }

    #[tokio::test]
    async fn test_peek_does_not_remove() {
        let mut node = PriorityQueueNode::new();
        node.init(&serde_json::json!({ "operation": "enqueue" }))
            .await
            .unwrap();
        node.execute(serde_json::json!({"priority": 5, "val": "x"}), &ctx())
            .await
            .unwrap();

        let shared = node.queue.clone();

        let mut peek_node = PriorityQueueNode::new();
        peek_node.queue = shared;
        peek_node
            .init(&serde_json::json!({ "operation": "peek" }))
            .await
            .unwrap();

        let r = peek_node
            .execute(serde_json::json!({}), &ctx())
            .await
            .unwrap();
        assert_eq!(r.ports.get("default").unwrap()["size"], 1);

        // Still size 1 after peek
        let r2 = peek_node
            .execute(serde_json::json!({}), &ctx())
            .await
            .unwrap();
        assert_eq!(r2.ports.get("default").unwrap()["size"], 1);
    }

    #[tokio::test]
    async fn test_size_and_clear() {
        let mut node = PriorityQueueNode::new();
        node.init(&serde_json::json!({ "operation": "enqueue" }))
            .await
            .unwrap();
        node.execute(serde_json::json!({"priority": 1}), &ctx())
            .await
            .unwrap();
        node.execute(serde_json::json!({"priority": 2}), &ctx())
            .await
            .unwrap();

        let shared = node.queue.clone();

        let mut size_node = PriorityQueueNode::new();
        size_node.queue = shared.clone();
        size_node
            .init(&serde_json::json!({ "operation": "size" }))
            .await
            .unwrap();
        let r = size_node
            .execute(serde_json::json!({}), &ctx())
            .await
            .unwrap();
        assert_eq!(r.ports.get("default").unwrap()["size"], 2);

        let mut clear_node = PriorityQueueNode::new();
        clear_node.queue = shared;
        clear_node
            .init(&serde_json::json!({ "operation": "clear" }))
            .await
            .unwrap();
        let r = clear_node
            .execute(serde_json::json!({}), &ctx())
            .await
            .unwrap();
        assert_eq!(r.ports.get("default").unwrap()["cleared"], 2);
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let mut node = PriorityQueueNode::new();
        let result = node
            .init(&serde_json::json!({ "operation": "invalid" }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_max_size_enforced() {
        let mut node = PriorityQueueNode::new();
        node.init(&serde_json::json!({ "operation": "enqueue", "max_size": 2 }))
            .await
            .unwrap();

        node.execute(serde_json::json!({"priority": 1}), &ctx())
            .await
            .unwrap();
        node.execute(serde_json::json!({"priority": 2}), &ctx())
            .await
            .unwrap();

        let result = node
            .execute(serde_json::json!({"priority": 3}), &ctx())
            .await;
        assert!(result.is_err());
    }
}
