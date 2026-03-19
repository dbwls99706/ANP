use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// SemaphoreNode — Concurrency limiter using atomic permits.
///
/// Config:
/// ```json
/// {
///   "max_permits": 5,
///   "operation": "acquire"
/// }
/// ```
pub struct SemaphoreNode {
    max_permits: u64,
    operation: String,
    available: AtomicU64,
    permit_counter: AtomicU64,
}

impl Default for SemaphoreNode {
    fn default() -> Self {
        Self::new()
    }
}

impl SemaphoreNode {
    pub fn new() -> Self {
        Self {
            max_permits: 5,
            operation: "acquire".to_string(),
            available: AtomicU64::new(5),
            permit_counter: AtomicU64::new(0),
        }
    }

    fn generate_permit_id(&self) -> String {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let count = self.permit_counter.fetch_add(1, Ordering::SeqCst);
        format!("permit_{:x}_{:x}", ts, count)
    }
}

#[async_trait::async_trait]
impl NodeExecutor for SemaphoreNode {
    fn node_type(&self) -> &str {
        "semaphore"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(max) = config.get("max_permits").and_then(|v| v.as_u64()) {
            if max == 0 {
                return Err(NodeError::InvalidConfig(
                    "max_permits must be greater than 0".to_string(),
                ));
            }
            self.max_permits = max;
            self.available.store(max, Ordering::SeqCst);
        }

        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "acquire" | "release" | "status" => {
                    self.operation = op.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown operation: '{}'. Use: acquire, release, status",
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
        let op = input
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.operation);

        match op {
            "acquire" => {
                // Try to decrement available permits atomically
                loop {
                    let current = self.available.load(Ordering::SeqCst);
                    if current == 0 {
                        return Ok(NodeOutput::data(serde_json::json!({
                            "acquired": false,
                            "queued": true,
                            "available": 0,
                            "max_permits": self.max_permits,
                            "permit_id": serde_json::Value::Null,
                        })));
                    }
                    if self
                        .available
                        .compare_exchange(current, current - 1, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok()
                    {
                        let permit_id = self.generate_permit_id();
                        let available = current - 1;
                        return Ok(NodeOutput::data(serde_json::json!({
                            "acquired": true,
                            "available": available,
                            "max_permits": self.max_permits,
                            "permit_id": permit_id,
                        })));
                    }
                    // CAS failed, retry
                }
            }
            "release" => {
                loop {
                    let current = self.available.load(Ordering::SeqCst);
                    if current >= self.max_permits {
                        // Already at max
                        return Ok(NodeOutput::data(serde_json::json!({
                            "released": false,
                            "available": self.max_permits,
                            "max_permits": self.max_permits,
                            "in_use": 0,
                        })));
                    }
                    if self
                        .available
                        .compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok()
                    {
                        let available = current + 1;
                        let in_use = self.max_permits - available;
                        return Ok(NodeOutput::data(serde_json::json!({
                            "released": true,
                            "available": available,
                            "max_permits": self.max_permits,
                            "in_use": in_use,
                        })));
                    }
                }
            }
            "status" => {
                let available = self.available.load(Ordering::SeqCst);
                let in_use = self.max_permits - available;

                Ok(NodeOutput::data(serde_json::json!({
                    "available": available,
                    "max_permits": self.max_permits,
                    "in_use": in_use,
                })))
            }
            other => Err(NodeError::ExecutionFailed(format!(
                "unknown operation: '{}'",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_acquire_success() {
        let mut node = SemaphoreNode::new();
        node.init(&serde_json::json!({"max_permits": 3}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({}), &ctx)
            .await
            .unwrap();

        let data = &output.ports["default"];
        assert_eq!(data["acquired"], true);
        assert_eq!(data["available"], 2);
        assert_eq!(data["max_permits"], 3);
        assert!(data["permit_id"].as_str().unwrap().starts_with("permit_"));
    }

    #[tokio::test]
    async fn test_acquire_exhausted() {
        let mut node = SemaphoreNode::new();
        node.init(&serde_json::json!({"max_permits": 1}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();

        // First acquire succeeds
        let r1 = node.execute(serde_json::json!({}), &ctx).await.unwrap();
        assert_eq!(r1.ports["default"]["acquired"], true);

        // Second acquire fails
        let r2 = node.execute(serde_json::json!({}), &ctx).await.unwrap();
        assert_eq!(r2.ports["default"]["acquired"], false);
        assert_eq!(r2.ports["default"]["queued"], true);
    }

    #[tokio::test]
    async fn test_release() {
        let mut node = SemaphoreNode::new();
        node.init(&serde_json::json!({"max_permits": 2, "operation": "acquire"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();

        // Acquire one
        node.execute(serde_json::json!({}), &ctx).await.unwrap();

        // Release it
        let output = node
            .execute(serde_json::json!({"operation": "release"}), &ctx)
            .await
            .unwrap();

        let data = &output.ports["default"];
        assert_eq!(data["released"], true);
        assert_eq!(data["available"], 2);
    }

    #[tokio::test]
    async fn test_release_at_max() {
        let mut node = SemaphoreNode::new();
        node.init(&serde_json::json!({"max_permits": 2, "operation": "release"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({}), &ctx)
            .await
            .unwrap();

        let data = &output.ports["default"];
        assert_eq!(data["released"], false);
        assert_eq!(data["available"], 2);
    }

    #[tokio::test]
    async fn test_status() {
        let mut node = SemaphoreNode::new();
        node.init(&serde_json::json!({"max_permits": 5, "operation": "acquire"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();

        // Acquire 2 permits
        node.execute(serde_json::json!({}), &ctx).await.unwrap();
        node.execute(serde_json::json!({}), &ctx).await.unwrap();

        // Check status
        let output = node
            .execute(serde_json::json!({"operation": "status"}), &ctx)
            .await
            .unwrap();

        let data = &output.ports["default"];
        assert_eq!(data["available"], 3);
        assert_eq!(data["max_permits"], 5);
        assert_eq!(data["in_use"], 2);
    }

    #[tokio::test]
    async fn test_invalid_config() {
        let mut node = SemaphoreNode::new();
        let result = node
            .init(&serde_json::json!({"max_permits": 0}))
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_node_type_and_kind() {
        let node = SemaphoreNode::new();
        assert_eq!(node.node_type(), "semaphore");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
