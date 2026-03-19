use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Counter node — thread-safe atomic counter.
///
/// Config:
/// ```json
/// { "operation": "increment", "step": 1, "initial": 0 }
/// ```
///
/// Operations:
/// - `increment`: adds `step` to counter
/// - `decrement`: subtracts `step` from counter
/// - `get`: returns current value without modification
/// - `reset`: resets counter to `initial` value
/// - `set`: sets counter to value from input `value` field
///
/// Output: `{ "count": current_value, "previous": previous_value }`
pub struct CounterNode {
    counter: Arc<AtomicI64>,
    operation: String,
    step: i64,
    initial: i64,
}

impl Default for CounterNode {
    fn default() -> Self {
        Self::new()
    }
}

impl CounterNode {
    pub fn new() -> Self {
        Self {
            counter: Arc::new(AtomicI64::new(0)),
            operation: "increment".to_string(),
            step: 1,
            initial: 0,
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for CounterNode {
    fn node_type(&self) -> &str {
        "counter"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "increment" | "decrement" | "get" | "reset" | "set" => {
                    self.operation = op.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown counter operation: '{}'. Use: increment, decrement, get, reset, set",
                        other
                    )));
                }
            }
        }
        if let Some(step) = config.get("step").and_then(|v| v.as_i64()) {
            self.step = step;
        }
        if let Some(initial) = config.get("initial").and_then(|v| v.as_i64()) {
            self.initial = initial;
            self.counter.store(initial, Ordering::SeqCst);
        }
        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let (previous, current) = match self.operation.as_str() {
            "increment" => {
                let prev = self.counter.fetch_add(self.step, Ordering::SeqCst);
                (prev, prev + self.step)
            }
            "decrement" => {
                let prev = self.counter.fetch_sub(self.step, Ordering::SeqCst);
                (prev, prev - self.step)
            }
            "get" => {
                let val = self.counter.load(Ordering::SeqCst);
                (val, val)
            }
            "reset" => {
                let prev = self.counter.swap(self.initial, Ordering::SeqCst);
                (prev, self.initial)
            }
            "set" => {
                let new_val = input
                    .get("value")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| {
                        NodeError::ExecutionFailed(
                            "set operation requires 'value' field in input".into(),
                        )
                    })?;
                let prev = self.counter.swap(new_val, Ordering::SeqCst);
                (prev, new_val)
            }
            _ => {
                return Err(NodeError::ExecutionFailed(format!(
                    "unknown operation: '{}'",
                    self.operation
                )));
            }
        };

        let result = serde_json::json!({
            "count": current,
            "previous": previous
        });
        Ok(NodeOutput::data(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_increment() {
        let mut node = CounterNode::new();
        node.init(&serde_json::json!({"operation": "increment", "step": 1}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();

        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        assert_eq!(output.ports["default"]["count"], 1);
        assert_eq!(output.ports["default"]["previous"], 0);

        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        assert_eq!(output.ports["default"]["count"], 2);
        assert_eq!(output.ports["default"]["previous"], 1);
    }

    #[tokio::test]
    async fn test_decrement() {
        let mut node = CounterNode::new();
        node.init(&serde_json::json!({"operation": "decrement", "initial": 10, "step": 2}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();

        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        assert_eq!(output.ports["default"]["count"], 8);
        assert_eq!(output.ports["default"]["previous"], 10);
    }

    #[tokio::test]
    async fn test_get() {
        let mut node = CounterNode::new();
        node.init(&serde_json::json!({"operation": "get", "initial": 42}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();

        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        assert_eq!(output.ports["default"]["count"], 42);
        assert_eq!(output.ports["default"]["previous"], 42);
    }

    #[tokio::test]
    async fn test_reset() {
        let mut node = CounterNode::new();
        node.init(&serde_json::json!({"operation": "increment", "initial": 0}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();

        // Increment a few times
        node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        node.execute(serde_json::Value::Null, &ctx).await.unwrap();

        // Now reset
        node.operation = "reset".to_string();
        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        assert_eq!(output.ports["default"]["count"], 0);
        assert_eq!(output.ports["default"]["previous"], 2);
    }

    #[tokio::test]
    async fn test_set() {
        let mut node = CounterNode::new();
        node.init(&serde_json::json!({"operation": "set"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();

        let input = serde_json::json!({"value": 99});
        let output = node.execute(input, &ctx).await.unwrap();
        assert_eq!(output.ports["default"]["count"], 99);
        assert_eq!(output.ports["default"]["previous"], 0);
    }

    #[tokio::test]
    async fn test_set_without_value_fails() {
        let mut node = CounterNode::new();
        node.init(&serde_json::json!({"operation": "set"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();

        let result = node.execute(serde_json::json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_custom_step() {
        let mut node = CounterNode::new();
        node.init(&serde_json::json!({"operation": "increment", "step": 5}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();

        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        assert_eq!(output.ports["default"]["count"], 5);

        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        assert_eq!(output.ports["default"]["count"], 10);
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let mut node = CounterNode::new();
        let result = node
            .init(&serde_json::json!({"operation": "multiply"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_node_type_and_kind() {
        let node = CounterNode::new();
        assert_eq!(node.node_type(), "counter");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
