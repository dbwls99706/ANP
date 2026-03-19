use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// AgentLoop node — implements an agentic loop pattern with state tracking.
///
/// Config:
/// ```json
/// { "max_iterations": 10, "termination_field": "done", "operation": "iterate" }
/// ```
///
/// Operations:
/// - `start`: begin loop with initial state, counter = 0
/// - `iterate`: increment counter, check max_iterations
/// - `check`: check termination_field + max_iterations
/// - `reset`: reset counter to 0
pub struct AgentLoopNode {
    counter: Arc<AtomicU64>,
    max_iterations: u64,
    termination_field: String,
    operation: String,
}

impl Default for AgentLoopNode {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentLoopNode {
    pub fn new() -> Self {
        Self {
            counter: Arc::new(AtomicU64::new(0)),
            max_iterations: 10,
            termination_field: "done".to_string(),
            operation: "iterate".to_string(),
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for AgentLoopNode {
    fn node_type(&self) -> &str {
        "agent_loop"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(max) = config.get("max_iterations").and_then(|v| v.as_u64()) {
            if max == 0 {
                return Err(NodeError::InvalidConfig(
                    "max_iterations must be greater than 0".into(),
                ));
            }
            self.max_iterations = max;
        }
        if let Some(field) = config.get("termination_field").and_then(|v| v.as_str()) {
            self.termination_field = field.to_string();
        }
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "start" | "iterate" | "check" | "reset" => {
                    self.operation = op.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown agent_loop operation: '{}'. Use: start, iterate, check, reset",
                        other
                    )));
                }
            }
        }
        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        match self.operation.as_str() {
            "start" => {
                self.counter.store(0, Ordering::SeqCst);
                Ok(NodeOutput::data(serde_json::json!({
                    "iteration": 0,
                    "state": "running",
                    "should_continue": true,
                    "max_iterations": self.max_iterations,
                })))
            }
            "iterate" => {
                let prev = self.counter.fetch_add(1, Ordering::SeqCst);
                let current = prev + 1;
                let should_continue = current < self.max_iterations;
                let state = if should_continue {
                    "running"
                } else {
                    "completed"
                };

                Ok(NodeOutput::data(serde_json::json!({
                    "iteration": current,
                    "state": state,
                    "should_continue": should_continue,
                    "max_iterations": self.max_iterations,
                })))
            }
            "check" => {
                let current = self.counter.load(Ordering::SeqCst);
                let at_max = current >= self.max_iterations;

                // Check termination field in input
                let terminated = input
                    .get(&self.termination_field)
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let should_continue = !at_max && !terminated;
                let state = if terminated {
                    "terminated"
                } else if at_max {
                    "completed"
                } else {
                    "running"
                };

                Ok(NodeOutput::data(serde_json::json!({
                    "iteration": current,
                    "state": state,
                    "should_continue": should_continue,
                    "max_iterations": self.max_iterations,
                })))
            }
            "reset" => {
                self.counter.store(0, Ordering::SeqCst);
                Ok(NodeOutput::data(serde_json::json!({
                    "iteration": 0,
                    "state": "reset",
                    "should_continue": true,
                    "max_iterations": self.max_iterations,
                })))
            }
            _ => Err(NodeError::ExecutionFailed(format!(
                "unknown operation: '{}'",
                self.operation
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_start() {
        let mut node = AgentLoopNode::new();
        node.init(&serde_json::json!({"operation": "start"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::Value::Null, &ctx)
            .await
            .unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["iteration"], 0);
        assert_eq!(data["state"], "running");
        assert_eq!(data["should_continue"], true);
    }

    #[tokio::test]
    async fn test_iterate_until_max() {
        let mut node = AgentLoopNode::new();
        node.init(&serde_json::json!({"operation": "iterate", "max_iterations": 3}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();

        // Iteration 1
        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        assert_eq!(output.ports["default"]["iteration"], 1);
        assert_eq!(output.ports["default"]["should_continue"], true);

        // Iteration 2
        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        assert_eq!(output.ports["default"]["iteration"], 2);
        assert_eq!(output.ports["default"]["should_continue"], true);

        // Iteration 3 — max reached
        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        assert_eq!(output.ports["default"]["iteration"], 3);
        assert_eq!(output.ports["default"]["should_continue"], false);
        assert_eq!(output.ports["default"]["state"], "completed");
    }

    #[tokio::test]
    async fn test_check_termination_field() {
        let mut node = AgentLoopNode::new();
        node.init(&serde_json::json!({
            "operation": "check",
            "termination_field": "done"
        }))
        .await
        .unwrap();
        let ctx = ExecutionContext::new();

        // Not terminated
        let output = node
            .execute(serde_json::json!({"done": false}), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["should_continue"], true);
        assert_eq!(output.ports["default"]["state"], "running");

        // Terminated
        let output = node
            .execute(serde_json::json!({"done": true}), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["should_continue"], false);
        assert_eq!(output.ports["default"]["state"], "terminated");
    }

    #[tokio::test]
    async fn test_reset() {
        let mut node = AgentLoopNode::new();
        node.init(&serde_json::json!({"operation": "iterate"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();

        // Iterate a few times
        node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        node.execute(serde_json::Value::Null, &ctx).await.unwrap();

        // Reset
        node.operation = "reset".to_string();
        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        assert_eq!(output.ports["default"]["iteration"], 0);
        assert_eq!(output.ports["default"]["state"], "reset");
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let mut node = AgentLoopNode::new();
        let result = node
            .init(&serde_json::json!({"operation": "run"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_zero_max_iterations() {
        let mut node = AgentLoopNode::new();
        let result = node
            .init(&serde_json::json!({"max_iterations": 0}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_node_type_and_kind() {
        let node = AgentLoopNode::new();
        assert_eq!(node.node_type(), "agent_loop");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
