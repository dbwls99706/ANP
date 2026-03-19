use std::sync::Arc;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeExecutor, NodeOutput, NodeResult, PipelineTrigger};
use anp_core::runtime::context::ExecutionContext;
use tokio::time::{interval, Duration};

/// Cron node — scheduled execution at fixed intervals.
///
/// Config:
/// ```json
/// {
///   "interval_ms": 60000,
///   "max_iterations": 0
/// }
/// ```
///
/// Kind: Service — spawns a background task that triggers the downstream
/// pipeline at fixed intervals. `max_iterations: 0` means infinite.
/// Each tick sends `{ "tick": N, "timestamp_ms": epoch_ms }`.
pub struct CronNode {
    interval_ms: u64,
    max_iterations: u64,
}

impl Default for CronNode {
    fn default() -> Self {
        Self::new()
    }
}

impl CronNode {
    pub fn new() -> Self {
        Self {
            interval_ms: 60000,
            max_iterations: 0,
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for CronNode {
    fn node_type(&self) -> &str {
        "cron"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Service
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(ms) = config.get("interval_ms").and_then(|v| v.as_u64()) {
            self.interval_ms = ms;
        }
        if let Some(max) = config.get("max_iterations").and_then(|v| v.as_u64()) {
            self.max_iterations = max;
        }
        Ok(())
    }

    async fn start_service(
        &self,
        node_id: &str,
        trigger: Arc<dyn PipelineTrigger>,
    ) -> NodeResult<()> {
        let interval_ms = self.interval_ms;
        let max_iter = self.max_iterations;
        let id = node_id.to_string();

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_millis(interval_ms));
            let mut count: u64 = 0;
            loop {
                ticker.tick().await;
                count += 1;
                let data = serde_json::json!({
                    "tick": count,
                    "timestamp_ms": std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64
                });
                let _ = trigger.trigger(&id, "default", data).await;
                if max_iter > 0 && count >= max_iter {
                    break;
                }
            }
        });

        Ok(())
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        // Service nodes should not be executed directly in service mode.
        // In batch mode, return config info as a mock.
        Ok(NodeOutput::data(serde_json::json!({
            "type": "cron",
            "interval_ms": self.interval_ms,
            "max_iterations": self.max_iterations,
            "status": "mock"
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_init_defaults() {
        let mut node = CronNode::new();
        node.init(&serde_json::json!({})).await.unwrap();
        assert_eq!(node.interval_ms, 60000);
        assert_eq!(node.max_iterations, 0);
    }

    #[tokio::test]
    async fn test_init_custom() {
        let mut node = CronNode::new();
        node.init(&serde_json::json!({
            "interval_ms": 5000,
            "max_iterations": 10
        }))
        .await
        .unwrap();
        assert_eq!(node.interval_ms, 5000);
        assert_eq!(node.max_iterations, 10);
    }

    #[tokio::test]
    async fn test_execute_batch_mode() {
        let mut node = CronNode::new();
        node.init(&serde_json::json!({
            "interval_ms": 1000,
            "max_iterations": 5
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::json!({}), &ctx).await.unwrap();
        let result = &output.ports["default"];
        assert_eq!(result["type"], "cron");
        assert_eq!(result["interval_ms"], 1000);
        assert_eq!(result["max_iterations"], 5);
        assert_eq!(result["status"], "mock");
    }

    #[test]
    fn test_node_type_and_kind() {
        let node = CronNode::new();
        assert_eq!(node.node_type(), "cron");
        assert_eq!(node.node_kind(), NodeKind::Service);
    }
}
