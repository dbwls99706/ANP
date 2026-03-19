use std::sync::Arc;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;
use tokio::sync::RwLock;

/// ThrottleNode — rate-limits execution by enforcing a minimum interval between calls.
///
/// Config example:
/// ```json
/// {
///   "interval_ms": 1000,
///   "drop_mode": "skip"
/// }
/// ```
///
/// If enough time has passed since the last execution, passes the input through.
/// Otherwise, depending on `drop_mode`:
/// - "skip": returns `NodeOutput::skip()`
/// - "queue": returns the input with throttle metadata
///
/// Output when allowed: `{ ...input, "_throttled": false }`
/// Output when queued: `{ ...input, "_throttled": true, "_delay_ms": remaining }`
pub struct ThrottleNode {
    interval_ms: u64,
    drop_mode: String,
    last_execution: Arc<RwLock<Option<std::time::Instant>>>,
}

impl Default for ThrottleNode {
    fn default() -> Self {
        Self::new()
    }
}

impl ThrottleNode {
    pub fn new() -> Self {
        Self {
            interval_ms: 1000,
            drop_mode: "skip".to_string(),
            last_execution: Arc::new(RwLock::new(None)),
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for ThrottleNode {
    fn node_type(&self) -> &str {
        "throttle"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(ms) = config.get("interval_ms").and_then(|v| v.as_u64()) {
            self.interval_ms = ms;
        }

        if let Some(mode) = config.get("drop_mode").and_then(|v| v.as_str()) {
            match mode {
                "skip" | "queue" => {
                    self.drop_mode = mode.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "invalid drop_mode '{}': must be 'skip' or 'queue'",
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
        let now = std::time::Instant::now();
        let interval = std::time::Duration::from_millis(self.interval_ms);

        let mut last = self.last_execution.write().await;

        if let Some(last_time) = *last {
            let elapsed = now.duration_since(last_time);
            if elapsed < interval {
                // Throttled
                let remaining_ms = (interval - elapsed).as_millis() as u64;
                return if self.drop_mode == "skip" {
                    Ok(NodeOutput::skip())
                } else {
                    // queue mode
                    let mut output = match input {
                        serde_json::Value::Object(mut map) => {
                            map.insert("_throttled".to_string(), serde_json::json!(true));
                            map.insert("_delay_ms".to_string(), serde_json::json!(remaining_ms));
                            serde_json::Value::Object(map)
                        }
                        other => {
                            serde_json::json!({
                                "_input": other,
                                "_throttled": true,
                                "_delay_ms": remaining_ms,
                            })
                        }
                    };
                    Ok(NodeOutput::data(output))
                };
            }
        }

        // Allowed through
        *last = Some(now);

        let output = match input {
            serde_json::Value::Object(mut map) => {
                map.insert("_throttled".to_string(), serde_json::json!(false));
                serde_json::Value::Object(map)
            }
            other => {
                serde_json::json!({
                    "_input": other,
                    "_throttled": false,
                })
            }
        };

        Ok(NodeOutput::data(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ExecutionContext {
        ExecutionContext::default()
    }

    #[tokio::test]
    async fn test_first_execution_passes_through() {
        let mut node = ThrottleNode::new();
        node.init(&serde_json::json!({ "interval_ms": 1000 }))
            .await
            .unwrap();

        let result = node
            .execute(serde_json::json!({"msg": "hello"}), &ctx())
            .await
            .unwrap();
        let out = result.ports.get("default").unwrap();
        assert_eq!(out["_throttled"], false);
        assert_eq!(out["msg"], "hello");
    }

    #[tokio::test]
    async fn test_throttled_skip_mode() {
        let mut node = ThrottleNode::new();
        node.init(&serde_json::json!({ "interval_ms": 10000, "drop_mode": "skip" }))
            .await
            .unwrap();

        // First call passes
        let _ = node
            .execute(serde_json::json!({"a": 1}), &ctx())
            .await
            .unwrap();

        // Second call should be throttled (skip)
        let r2 = node
            .execute(serde_json::json!({"a": 2}), &ctx())
            .await
            .unwrap();
        assert!(r2.ports.is_empty()); // skip
    }

    #[tokio::test]
    async fn test_throttled_queue_mode() {
        let mut node = ThrottleNode::new();
        node.init(&serde_json::json!({ "interval_ms": 10000, "drop_mode": "queue" }))
            .await
            .unwrap();

        let _ = node
            .execute(serde_json::json!({"a": 1}), &ctx())
            .await
            .unwrap();

        let r2 = node
            .execute(serde_json::json!({"a": 2}), &ctx())
            .await
            .unwrap();
        let out = r2.ports.get("default").unwrap();
        assert_eq!(out["_throttled"], true);
        assert!(out["_delay_ms"].as_u64().unwrap() > 0);
    }

    #[tokio::test]
    async fn test_invalid_drop_mode() {
        let mut node = ThrottleNode::new();
        let result = node
            .init(&serde_json::json!({ "drop_mode": "invalid" }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_node_type_and_kind() {
        let node = ThrottleNode::new();
        assert_eq!(node.node_type(), "throttle");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }

    #[tokio::test]
    async fn test_allows_after_interval_passes() {
        let mut node = ThrottleNode::new();
        node.init(&serde_json::json!({ "interval_ms": 10 }))
            .await
            .unwrap();

        let _ = node
            .execute(serde_json::json!({"a": 1}), &ctx())
            .await
            .unwrap();

        // Wait for interval to pass
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let r2 = node
            .execute(serde_json::json!({"a": 2}), &ctx())
            .await
            .unwrap();
        let out = r2.ports.get("default").unwrap();
        assert_eq!(out["_throttled"], false);
    }
}
