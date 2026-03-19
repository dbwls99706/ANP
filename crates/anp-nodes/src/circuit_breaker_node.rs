use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// CircuitBreakerNode — Circuit breaker pattern for fault tolerance.
///
/// States: closed (normal), open (blocking), half_open (testing).
///
/// Config:
/// ```json
/// {
///   "failure_threshold": 5,
///   "reset_timeout_ms": 30000,
///   "half_open_max": 1
/// }
/// ```
pub struct CircuitBreakerNode {
    failure_threshold: u64,
    reset_timeout_ms: u64,
    half_open_max: u64,
    failures: AtomicU64,
    total_requests: AtomicU64,
    half_open_allowed: AtomicU64,
    state: Arc<RwLock<String>>,
    last_failure_time: Arc<RwLock<u64>>,
}

impl Default for CircuitBreakerNode {
    fn default() -> Self {
        Self::new()
    }
}

impl CircuitBreakerNode {
    pub fn new() -> Self {
        Self {
            failure_threshold: 5,
            reset_timeout_ms: 30000,
            half_open_max: 1,
            failures: AtomicU64::new(0),
            total_requests: AtomicU64::new(0),
            half_open_allowed: AtomicU64::new(0),
            state: Arc::new(RwLock::new("closed".to_string())),
            last_failure_time: Arc::new(RwLock::new(0)),
        }
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

#[async_trait::async_trait]
impl NodeExecutor for CircuitBreakerNode {
    fn node_type(&self) -> &str {
        "circuit_breaker"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(threshold) = config.get("failure_threshold").and_then(|v| v.as_u64()) {
            if threshold == 0 {
                return Err(NodeError::InvalidConfig(
                    "failure_threshold must be greater than 0".to_string(),
                ));
            }
            self.failure_threshold = threshold;
        }

        if let Some(timeout) = config.get("reset_timeout_ms").and_then(|v| v.as_u64()) {
            self.reset_timeout_ms = timeout;
        }

        if let Some(max) = config.get("half_open_max").and_then(|v| v.as_u64()) {
            if max == 0 {
                return Err(NodeError::InvalidConfig(
                    "half_open_max must be greater than 0".to_string(),
                ));
            }
            self.half_open_max = max;
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        self.total_requests.fetch_add(1, Ordering::SeqCst);
        let now = Self::now_ms();

        let current_state = {
            let s = self.state.read().unwrap();
            s.clone()
        };

        match current_state.as_str() {
            "closed" => {
                let has_error = input.get("_error").is_some();
                if has_error {
                    let failures = self.failures.fetch_add(1, Ordering::SeqCst) + 1;
                    {
                        let mut lft = self.last_failure_time.write().unwrap();
                        *lft = now;
                    }
                    if failures >= self.failure_threshold {
                        let mut s = self.state.write().unwrap();
                        *s = "open".to_string();
                    }
                }

                let state = self.state.read().unwrap().clone();
                let failures = self.failures.load(Ordering::SeqCst);
                let total = self.total_requests.load(Ordering::SeqCst);

                let mut result = input.clone();
                if let Some(obj) = result.as_object_mut() {
                    obj.insert(
                        "_circuit_breaker".to_string(),
                        serde_json::json!({
                            "state": state,
                            "failures": failures,
                            "total_requests": total,
                        }),
                    );
                }

                Ok(NodeOutput::data(result))
            }
            "open" => {
                let last_failure = {
                    let lft = self.last_failure_time.read().unwrap();
                    *lft
                };

                if now.saturating_sub(last_failure) >= self.reset_timeout_ms {
                    // Transition to half_open
                    {
                        let mut s = self.state.write().unwrap();
                        *s = "half_open".to_string();
                    }
                    self.half_open_allowed.store(1, Ordering::SeqCst);

                    // Process this request as half_open: check success/failure
                    let has_error = input.get("_error").is_some();
                    if has_error {
                        // Back to open
                        {
                            let mut s = self.state.write().unwrap();
                            *s = "open".to_string();
                        }
                        {
                            let mut lft = self.last_failure_time.write().unwrap();
                            *lft = now;
                        }
                        self.failures.fetch_add(1, Ordering::SeqCst);
                    } else {
                        // Success — reset to closed
                        {
                            let mut s = self.state.write().unwrap();
                            *s = "closed".to_string();
                        }
                        self.failures.store(0, Ordering::SeqCst);
                    }

                    let state = self.state.read().unwrap().clone();
                    let failures = self.failures.load(Ordering::SeqCst);
                    let total = self.total_requests.load(Ordering::SeqCst);

                    let mut result = input.clone();
                    if let Some(obj) = result.as_object_mut() {
                        obj.insert(
                            "_circuit_breaker".to_string(),
                            serde_json::json!({
                                "state": state,
                                "failures": failures,
                                "total_requests": total,
                            }),
                        );
                    }

                    Ok(NodeOutput::data(result))
                } else {
                    let total = self.total_requests.load(Ordering::SeqCst);
                    let failures = self.failures.load(Ordering::SeqCst);

                    Ok(NodeOutput::data(serde_json::json!({
                        "state": "open",
                        "blocked": true,
                        "_circuit_breaker": {
                            "state": "open",
                            "failures": failures,
                            "total_requests": total,
                        }
                    })))
                }
            }
            "half_open" => {
                let allowed = self.half_open_allowed.load(Ordering::SeqCst);
                if allowed >= self.half_open_max {
                    // Too many half_open requests, block
                    let total = self.total_requests.load(Ordering::SeqCst);
                    let failures = self.failures.load(Ordering::SeqCst);

                    return Ok(NodeOutput::data(serde_json::json!({
                        "state": "half_open",
                        "blocked": true,
                        "_circuit_breaker": {
                            "state": "half_open",
                            "failures": failures,
                            "total_requests": total,
                        }
                    })));
                }

                self.half_open_allowed.fetch_add(1, Ordering::SeqCst);
                let has_error = input.get("_error").is_some();

                if has_error {
                    // Back to open
                    {
                        let mut s = self.state.write().unwrap();
                        *s = "open".to_string();
                    }
                    {
                        let mut lft = self.last_failure_time.write().unwrap();
                        *lft = now;
                    }
                    self.failures.fetch_add(1, Ordering::SeqCst);
                } else {
                    // Success — reset to closed
                    {
                        let mut s = self.state.write().unwrap();
                        *s = "closed".to_string();
                    }
                    self.failures.store(0, Ordering::SeqCst);
                }

                let state = self.state.read().unwrap().clone();
                let failures = self.failures.load(Ordering::SeqCst);
                let total = self.total_requests.load(Ordering::SeqCst);

                let mut result = input.clone();
                if let Some(obj) = result.as_object_mut() {
                    obj.insert(
                        "_circuit_breaker".to_string(),
                        serde_json::json!({
                            "state": state,
                            "failures": failures,
                            "total_requests": total,
                        }),
                    );
                }

                Ok(NodeOutput::data(result))
            }
            _ => Err(NodeError::ExecutionFailed(format!(
                "unknown circuit breaker state: '{}'",
                current_state
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_closed_passes_through() {
        let mut node = CircuitBreakerNode::new();
        node.init(&serde_json::json!({"failure_threshold": 3}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"data": "hello"}), &ctx)
            .await
            .unwrap();

        let data = &output.ports["default"];
        assert_eq!(data["data"], "hello");
        assert_eq!(data["_circuit_breaker"]["state"], "closed");
        assert_eq!(data["_circuit_breaker"]["failures"], 0);
    }

    #[tokio::test]
    async fn test_failures_increment() {
        let mut node = CircuitBreakerNode::new();
        node.init(&serde_json::json!({"failure_threshold": 3}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();

        // Send error input
        let output = node
            .execute(serde_json::json!({"_error": "fail1"}), &ctx)
            .await
            .unwrap();

        let data = &output.ports["default"];
        assert_eq!(data["_circuit_breaker"]["failures"], 1);
        assert_eq!(data["_circuit_breaker"]["state"], "closed");
    }

    #[tokio::test]
    async fn test_opens_after_threshold() {
        let mut node = CircuitBreakerNode::new();
        node.init(&serde_json::json!({"failure_threshold": 2}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();

        // Two errors to reach threshold
        node.execute(serde_json::json!({"_error": "fail1"}), &ctx)
            .await
            .unwrap();
        let output = node
            .execute(serde_json::json!({"_error": "fail2"}), &ctx)
            .await
            .unwrap();

        let data = &output.ports["default"];
        assert_eq!(data["_circuit_breaker"]["state"], "open");

        // Next request should be blocked
        let output = node
            .execute(serde_json::json!({"data": "test"}), &ctx)
            .await
            .unwrap();

        let data = &output.ports["default"];
        assert_eq!(data["blocked"], true);
        assert_eq!(data["state"], "open");
    }

    #[tokio::test]
    async fn test_half_open_success_resets() {
        let mut node = CircuitBreakerNode::new();
        node.init(&serde_json::json!({
            "failure_threshold": 1,
            "reset_timeout_ms": 0
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();

        // Trigger open
        node.execute(serde_json::json!({"_error": "fail"}), &ctx)
            .await
            .unwrap();

        // With reset_timeout_ms = 0, it should transition to half_open immediately
        let output = node
            .execute(serde_json::json!({"data": "test"}), &ctx)
            .await
            .unwrap();

        let data = &output.ports["default"];
        // The request went through half_open and succeeded, so state resets to closed
        assert_eq!(data["_circuit_breaker"]["state"], "closed");
        assert_eq!(data["_circuit_breaker"]["failures"], 0);
    }

    #[tokio::test]
    async fn test_half_open_failure_reopens() {
        let mut node = CircuitBreakerNode::new();
        node.init(&serde_json::json!({
            "failure_threshold": 1,
            "reset_timeout_ms": 0,
            "half_open_max": 2
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();

        // Trigger open
        node.execute(serde_json::json!({"_error": "fail"}), &ctx)
            .await
            .unwrap();

        // Half_open transition, but send error
        let output = node
            .execute(serde_json::json!({"_error": "fail_again"}), &ctx)
            .await
            .unwrap();

        let data = &output.ports["default"];
        assert_eq!(data["_circuit_breaker"]["state"], "open");
    }

    #[tokio::test]
    async fn test_invalid_config() {
        let mut node = CircuitBreakerNode::new();
        let result = node
            .init(&serde_json::json!({"failure_threshold": 0}))
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_node_type_and_kind() {
        let node = CircuitBreakerNode::new();
        assert_eq!(node.node_type(), "circuit_breaker");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
