use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;
use tokio::time::{sleep, Duration};

/// Sleep node — delays execution then passes through input unchanged.
///
/// Config:
/// ```json
/// { "duration_ms": 1000, "max_duration_ms": 30000 }
/// ```
///
/// Sleeps for the specified duration (capped by `max_duration_ms` to prevent hanging).
/// Passes through input data unchanged after the delay.
pub struct SleepNode {
    duration_ms: u64,
    max_duration_ms: u64,
}

impl Default for SleepNode {
    fn default() -> Self {
        Self::new()
    }
}

impl SleepNode {
    pub fn new() -> Self {
        Self {
            duration_ms: 1000,
            max_duration_ms: 30000,
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for SleepNode {
    fn node_type(&self) -> &str {
        "sleep"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(dur) = config.get("duration_ms").and_then(|v| v.as_u64()) {
            self.duration_ms = dur;
        }
        if let Some(max) = config.get("max_duration_ms").and_then(|v| v.as_u64()) {
            self.max_duration_ms = max;
        }

        if self.max_duration_ms == 0 {
            return Err(NodeError::InvalidConfig(
                "max_duration_ms must be greater than 0".into(),
            ));
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        // Cap duration at max to prevent hanging
        let actual_duration = self.duration_ms.min(self.max_duration_ms);

        sleep(Duration::from_millis(actual_duration)).await;

        let mut output = NodeOutput::data(input);
        output.logs.push(format!("slept for {}ms", actual_duration));
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sleep_passthrough() {
        let mut node = SleepNode::new();
        node.init(&serde_json::json!({"duration_ms": 10}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let input = serde_json::json!({"key": "value"});
        let output = node.execute(input.clone(), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], input);
    }

    #[tokio::test]
    async fn test_sleep_duration_capped() {
        let mut node = SleepNode::new();
        node.init(&serde_json::json!({"duration_ms": 100000, "max_duration_ms": 50}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();

        let start = std::time::Instant::now();
        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        let elapsed = start.elapsed();

        // Should be capped at 50ms, not 100000ms
        assert!(elapsed.as_millis() < 500);
        assert!(output.logs[0].contains("50ms"));
    }

    #[tokio::test]
    async fn test_sleep_null_input() {
        let mut node = SleepNode::new();
        node.init(&serde_json::json!({"duration_ms": 1}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        assert!(output.ports["default"].is_null());
    }

    #[tokio::test]
    async fn test_zero_max_duration_fails() {
        let mut node = SleepNode::new();
        let result = node
            .init(&serde_json::json!({"max_duration_ms": 0}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_default_config() {
        let mut node = SleepNode::new();
        node.init(&serde_json::json!({})).await.unwrap();
        assert_eq!(node.duration_ms, 1000);
        assert_eq!(node.max_duration_ms, 30000);
    }

    #[tokio::test]
    async fn test_node_type_and_kind() {
        let node = SleepNode::new();
        assert_eq!(node.node_type(), "sleep");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
