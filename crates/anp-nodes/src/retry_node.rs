use std::collections::HashMap;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// RetryNode — retry logic with exponential backoff.
///
/// Config example:
/// ```json
/// {
///   "max_retries": 3,
///   "backoff_ms": 1000,
///   "backoff_multiplier": 2.0,
///   "max_backoff_ms": 30000
/// }
/// ```
///
/// This node implements retry LOGIC by inspecting input fields:
/// - If input has "_error" and "_retry_count" < max_retries: increments count,
///   adds backoff delay info, routes to "retry" port.
/// - If max retries exceeded: routes to "failed" port.
/// - If no error: routes to "success" port.
///
/// Output includes `{ "_retry_count": n, "_next_backoff_ms": ms }`.
pub struct RetryNode {
    max_retries: u64,
    backoff_ms: u64,
    backoff_multiplier: f64,
    max_backoff_ms: u64,
}

impl Default for RetryNode {
    fn default() -> Self {
        Self::new()
    }
}

impl RetryNode {
    pub fn new() -> Self {
        Self {
            max_retries: 3,
            backoff_ms: 1000,
            backoff_multiplier: 2.0,
            max_backoff_ms: 30000,
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for RetryNode {
    fn node_type(&self) -> &str {
        "retry"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(v) = config.get("max_retries").and_then(|v| v.as_u64()) {
            self.max_retries = v;
        }

        if let Some(v) = config.get("backoff_ms").and_then(|v| v.as_u64()) {
            self.backoff_ms = v;
        }

        if let Some(v) = config.get("backoff_multiplier").and_then(|v| v.as_f64()) {
            self.backoff_multiplier = v;
        }

        if let Some(v) = config.get("max_backoff_ms").and_then(|v| v.as_u64()) {
            self.max_backoff_ms = v;
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let has_error = input.get("_error").is_some();

        if !has_error {
            // No error — success
            let mut ports = HashMap::new();
            ports.insert("success".to_string(), input);
            return Ok(
                NodeOutput::multi(ports).with_log("no error, routed to 'success'".to_string())
            );
        }

        // Has error — check retry count
        let retry_count = input
            .get("_retry_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        if retry_count >= self.max_retries {
            // Max retries exceeded
            let mut output = input;
            if let serde_json::Value::Object(ref mut map) = output {
                map.insert("_retry_count".to_string(), serde_json::json!(retry_count));
            }

            let mut ports = HashMap::new();
            ports.insert("failed".to_string(), output);
            return Ok(NodeOutput::multi(ports).with_log(format!(
                "max retries ({}) exceeded, routed to 'failed'",
                self.max_retries
            )));
        }

        // Calculate next backoff with exponential increase
        let next_count = retry_count + 1;
        let backoff =
            (self.backoff_ms as f64 * self.backoff_multiplier.powi(retry_count as i32)) as u64;
        let capped_backoff = backoff.min(self.max_backoff_ms);

        let mut output = input;
        if let serde_json::Value::Object(ref mut map) = output {
            map.insert("_retry_count".to_string(), serde_json::json!(next_count));
            map.insert(
                "_next_backoff_ms".to_string(),
                serde_json::json!(capped_backoff),
            );
        }

        let mut ports = HashMap::new();
        ports.insert("retry".to_string(), output);
        Ok(NodeOutput::multi(ports).with_log(format!(
            "retry {} of {}, next backoff {}ms",
            next_count, self.max_retries, capped_backoff
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_no_error_routes_to_success() {
        let mut node = RetryNode::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"data": "ok"}), &ctx)
            .await
            .unwrap();

        assert!(output.ports.contains_key("success"));
        assert_eq!(output.ports["success"]["data"], "ok");
    }

    #[tokio::test]
    async fn test_error_first_retry() {
        let mut node = RetryNode::new();
        node.init(&serde_json::json!({
            "max_retries": 3,
            "backoff_ms": 1000,
            "backoff_multiplier": 2.0
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"_error": "timeout"}), &ctx)
            .await
            .unwrap();

        assert!(output.ports.contains_key("retry"));
        assert_eq!(output.ports["retry"]["_retry_count"], 1);
        assert_eq!(output.ports["retry"]["_next_backoff_ms"], 1000);
    }

    #[tokio::test]
    async fn test_error_second_retry_exponential() {
        let mut node = RetryNode::new();
        node.init(&serde_json::json!({
            "max_retries": 3,
            "backoff_ms": 1000,
            "backoff_multiplier": 2.0
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({"_error": "timeout", "_retry_count": 1}),
                &ctx,
            )
            .await
            .unwrap();

        assert!(output.ports.contains_key("retry"));
        assert_eq!(output.ports["retry"]["_retry_count"], 2);
        assert_eq!(output.ports["retry"]["_next_backoff_ms"], 2000);
    }

    #[tokio::test]
    async fn test_max_retries_exceeded() {
        let mut node = RetryNode::new();
        node.init(&serde_json::json!({"max_retries": 3}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({"_error": "timeout", "_retry_count": 3}),
                &ctx,
            )
            .await
            .unwrap();

        assert!(output.ports.contains_key("failed"));
        assert!(!output.ports.contains_key("retry"));
    }

    #[tokio::test]
    async fn test_backoff_capped_at_max() {
        let mut node = RetryNode::new();
        node.init(&serde_json::json!({
            "max_retries": 10,
            "backoff_ms": 1000,
            "backoff_multiplier": 10.0,
            "max_backoff_ms": 5000
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({"_error": "fail", "_retry_count": 2}),
                &ctx,
            )
            .await
            .unwrap();

        assert!(output.ports.contains_key("retry"));
        // 1000 * 10^2 = 100000, but capped at 5000
        assert_eq!(output.ports["retry"]["_next_backoff_ms"], 5000);
    }
}
