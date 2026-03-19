use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;
use tokio::sync::RwLock;

/// Rate limit node — sliding window rate limiter.
///
/// Config:
/// ```json
/// {
///   "max_requests": 100,
///   "window_ms": 60000,
///   "key_field": "ip"
/// }
/// ```
///
/// Multi-port output:
/// - "allowed": input passes through when under the rate limit
/// - "rejected": routes here with error info when over the limit
pub struct RateLimitNode {
    max_requests: u64,
    window_ms: u64,
    key_field: Option<String>,
    windows: Arc<RwLock<HashMap<String, Vec<Instant>>>>,
}

impl Default for RateLimitNode {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimitNode {
    pub fn new() -> Self {
        Self {
            max_requests: 100,
            window_ms: 60000,
            key_field: None,
            windows: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

/// Resolve a dot-path field from a JSON value
fn resolve_field(value: &serde_json::Value, field: &str) -> Option<String> {
    let mut current = value;
    for part in field.split('.') {
        current = current.get(part)?;
    }
    current.as_str().map(|s| s.to_string()).or_else(|| {
        // For non-string values, use their JSON representation
        Some(current.to_string())
    })
}

#[async_trait::async_trait]
impl NodeExecutor for RateLimitNode {
    fn node_type(&self) -> &str {
        "rate_limit"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(max) = config.get("max_requests").and_then(|v| v.as_u64()) {
            self.max_requests = max;
        }
        if let Some(window) = config.get("window_ms").and_then(|v| v.as_u64()) {
            self.window_ms = window;
        }
        if let Some(key) = config.get("key_field").and_then(|v| v.as_str()) {
            self.key_field = Some(key.to_string());
        }
        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let key = self
            .key_field
            .as_ref()
            .and_then(|field| resolve_field(&input, field))
            .unwrap_or_else(|| "default".to_string());

        let now = Instant::now();
        let window_duration = std::time::Duration::from_millis(self.window_ms);

        let mut windows = self.windows.write().await;
        let timestamps = windows.entry(key.clone()).or_default();

        // Remove expired timestamps (outside the sliding window)
        timestamps.retain(|t| now.duration_since(*t) < window_duration);

        if (timestamps.len() as u64) < self.max_requests {
            // Under limit — allow the request
            timestamps.push(now);
            let count = timestamps.len() as u64;
            drop(windows);

            let mut ports = HashMap::new();
            ports.insert("allowed".to_string(), input);
            Ok(NodeOutput::multi(ports).with_log(format!(
                "allowed: {}/{} requests for key '{}'",
                count, self.max_requests, key
            )))
        } else {
            // Over limit — reject
            // Calculate retry_after: time until the oldest request in the window expires
            let oldest = timestamps.first().copied().unwrap_or(now);
            let elapsed = now.duration_since(oldest).as_millis() as u64;
            let retry_after_ms = self.window_ms.saturating_sub(elapsed);
            drop(windows);

            let mut ports = HashMap::new();
            ports.insert(
                "rejected".to_string(),
                serde_json::json!({
                    "error": "rate_limited",
                    "retry_after_ms": retry_after_ms,
                    "max_requests": self.max_requests,
                    "window_ms": self.window_ms,
                    "key": key
                }),
            );
            Ok(NodeOutput::multi(ports)
                .with_log(format!("rejected: rate limit exceeded for key '{}'", key)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_allows_under_limit() {
        let mut node = RateLimitNode::new();
        node.init(&serde_json::json!({"max_requests": 5, "window_ms": 60000}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"data": "test"}), &ctx)
            .await
            .unwrap();

        assert!(output.ports.contains_key("allowed"));
        assert_eq!(output.ports["allowed"]["data"], "test");
    }

    #[tokio::test]
    async fn test_rejects_over_limit() {
        let mut node = RateLimitNode::new();
        node.init(&serde_json::json!({"max_requests": 2, "window_ms": 60000}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();

        // First two should pass
        let r1 = node
            .execute(serde_json::json!({"n": 1}), &ctx)
            .await
            .unwrap();
        assert!(r1.ports.contains_key("allowed"));

        let r2 = node
            .execute(serde_json::json!({"n": 2}), &ctx)
            .await
            .unwrap();
        assert!(r2.ports.contains_key("allowed"));

        // Third should be rejected
        let r3 = node
            .execute(serde_json::json!({"n": 3}), &ctx)
            .await
            .unwrap();
        assert!(r3.ports.contains_key("rejected"));
        assert_eq!(r3.ports["rejected"]["error"], "rate_limited");
    }

    #[tokio::test]
    async fn test_key_field_isolation() {
        let mut node = RateLimitNode::new();
        node.init(&serde_json::json!({
            "max_requests": 1,
            "window_ms": 60000,
            "key_field": "ip"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();

        // First IP allowed
        let r1 = node
            .execute(serde_json::json!({"ip": "1.1.1.1"}), &ctx)
            .await
            .unwrap();
        assert!(r1.ports.contains_key("allowed"));

        // Same IP rejected
        let r2 = node
            .execute(serde_json::json!({"ip": "1.1.1.1"}), &ctx)
            .await
            .unwrap();
        assert!(r2.ports.contains_key("rejected"));

        // Different IP allowed
        let r3 = node
            .execute(serde_json::json!({"ip": "2.2.2.2"}), &ctx)
            .await
            .unwrap();
        assert!(r3.ports.contains_key("allowed"));
    }

    #[test]
    fn test_node_type_and_kind() {
        let node = RateLimitNode::new();
        assert_eq!(node.node_type(), "rate_limit");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
