use std::sync::Arc;
use std::time::Instant;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;
use tokio::sync::RwLock;

/// DebounceNode — debounces rapid inputs.
///
/// Config example:
/// ```json
/// {
///   "delay_ms": 500
/// }
/// ```
///
/// Stores the last input and timestamp. Only emits if `delay_ms` has passed
/// since the last input was received.
/// - If called within the delay window: stores input, returns Skip.
/// - If called after the delay window: emits the stored input.
pub struct DebounceNode {
    delay_ms: u64,
    state: Arc<RwLock<DebounceState>>,
}

struct DebounceState {
    last_input: Option<serde_json::Value>,
    last_time: Option<Instant>,
}

impl Default for DebounceNode {
    fn default() -> Self {
        Self::new()
    }
}

impl DebounceNode {
    pub fn new() -> Self {
        Self {
            delay_ms: 500,
            state: Arc::new(RwLock::new(DebounceState {
                last_input: None,
                last_time: None,
            })),
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for DebounceNode {
    fn node_type(&self) -> &str {
        "debounce"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(delay) = config.get("delay_ms").and_then(|v| v.as_u64()) {
            self.delay_ms = delay;
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let now = Instant::now();
        let mut state = self.state.write().await;

        match state.last_time {
            Some(last_time)
                if now.duration_since(last_time).as_millis() < self.delay_ms as u128 =>
            {
                // Within delay window — store and skip
                state.last_input = Some(input);
                state.last_time = Some(now);
                Ok(NodeOutput::skip()
                    .with_log(format!("debounced (within {}ms window)", self.delay_ms)))
            }
            _ => {
                // Outside delay window (or first call) — emit the input
                // If there was a stored input from a previous debounce, emit that instead
                let output = if let Some(stored) = state.last_input.take() {
                    stored
                } else {
                    input
                };

                state.last_time = Some(now);
                state.last_input = None;

                Ok(NodeOutput::data(output)
                    .with_log(format!("emitted after {}ms delay window", self.delay_ms)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_first_call_emits() {
        let mut node = DebounceNode::new();
        node.init(&serde_json::json!({"delay_ms": 100}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"data": "first"}), &ctx)
            .await
            .unwrap();

        assert_eq!(output.status, anp_core::node::traits::NodeStatus::Ok);
        assert_eq!(output.ports["default"]["data"], "first");
    }

    #[tokio::test]
    async fn test_rapid_calls_skip() {
        let mut node = DebounceNode::new();
        node.init(&serde_json::json!({"delay_ms": 1000}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();

        // First call emits
        let out1 = node
            .execute(serde_json::json!({"n": 1}), &ctx)
            .await
            .unwrap();
        assert_eq!(out1.status, anp_core::node::traits::NodeStatus::Ok);

        // Immediate second call should skip (within 1000ms window)
        let out2 = node
            .execute(serde_json::json!({"n": 2}), &ctx)
            .await
            .unwrap();
        assert_eq!(out2.status, anp_core::node::traits::NodeStatus::Skip);
    }

    #[tokio::test]
    async fn test_emits_after_delay() {
        let mut node = DebounceNode::new();
        node.init(&serde_json::json!({"delay_ms": 50}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();

        // First call
        let out1 = node
            .execute(serde_json::json!({"n": 1}), &ctx)
            .await
            .unwrap();
        assert_eq!(out1.status, anp_core::node::traits::NodeStatus::Ok);

        // Wait for delay to pass
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;

        // Should emit now (stored input from debounce, or new input)
        let out2 = node
            .execute(serde_json::json!({"n": 2}), &ctx)
            .await
            .unwrap();
        assert_eq!(out2.status, anp_core::node::traits::NodeStatus::Ok);
    }

    #[tokio::test]
    async fn test_default_delay() {
        let node = DebounceNode::new();
        assert_eq!(node.delay_ms, 500);
    }
}
