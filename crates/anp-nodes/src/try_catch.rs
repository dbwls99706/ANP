use std::collections::HashMap;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// TryCatchNode — error handling with fallback routing.
///
/// Config example:
/// ```json
/// {
///   "catch_port": "error",
///   "finally_port": "finally"
/// }
/// ```
///
/// Routing behavior:
/// - If input contains an "_error" field, routes to `catch_port` with
///   `{ "_caught": true, "_error_message": "..." }` added to the output.
/// - If no "_error" field, routes to "default" port (pass-through).
/// - If `finally_port` is configured, always also sends to that port.
pub struct TryCatchNode {
    catch_port: String,
    finally_port: Option<String>,
}

impl Default for TryCatchNode {
    fn default() -> Self {
        Self::new()
    }
}

impl TryCatchNode {
    pub fn new() -> Self {
        Self {
            catch_port: "error".to_string(),
            finally_port: None,
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for TryCatchNode {
    fn node_type(&self) -> &str {
        "try_catch"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(cp) = config.get("catch_port").and_then(|v| v.as_str()) {
            self.catch_port = cp.to_string();
        }

        if let Some(fp) = config.get("finally_port").and_then(|v| v.as_str()) {
            if !fp.is_empty() {
                self.finally_port = Some(fp.to_string());
            }
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let mut ports = HashMap::new();
        let has_error = input.get("_error").is_some();

        if has_error {
            // Extract the error message
            let error_message = input
                .get("_error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error")
                .to_string();

            // Build catch output: original input + catch metadata
            let mut catch_output = input.clone();
            if let serde_json::Value::Object(ref mut map) = catch_output {
                map.insert("_caught".to_string(), serde_json::json!(true));
                map.insert(
                    "_error_message".to_string(),
                    serde_json::json!(error_message),
                );
            }

            ports.insert(self.catch_port.clone(), catch_output);
        } else {
            // No error — pass through to default port
            ports.insert("default".to_string(), input.clone());
        }

        // Always send to finally_port if configured
        if let Some(ref finally_port) = self.finally_port {
            ports.insert(finally_port.clone(), input);
        }

        let log_msg = if has_error {
            format!("error caught, routed to '{}'", self.catch_port)
        } else {
            "no error, routed to 'default'".to_string()
        };

        Ok(NodeOutput::multi(ports).with_log(log_msg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_no_error_passes_through() {
        let mut node = TryCatchNode::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"data": "hello"}), &ctx)
            .await
            .unwrap();

        assert!(output.ports.contains_key("default"));
        assert_eq!(output.ports["default"]["data"], "hello");
        assert!(!output.ports.contains_key("error"));
    }

    #[tokio::test]
    async fn test_error_routes_to_catch() {
        let mut node = TryCatchNode::new();
        node.init(&serde_json::json!({"catch_port": "err"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({"_error": "something failed", "data": 42}),
                &ctx,
            )
            .await
            .unwrap();

        assert!(output.ports.contains_key("err"));
        assert!(!output.ports.contains_key("default"));
        assert_eq!(output.ports["err"]["_caught"], true);
        assert_eq!(output.ports["err"]["_error_message"], "something failed");
        assert_eq!(output.ports["err"]["data"], 42);
    }

    #[tokio::test]
    async fn test_finally_port_always_fires() {
        let mut node = TryCatchNode::new();
        node.init(&serde_json::json!({
            "catch_port": "error",
            "finally_port": "cleanup"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();

        // Without error
        let output = node
            .execute(serde_json::json!({"data": "ok"}), &ctx)
            .await
            .unwrap();
        assert!(output.ports.contains_key("default"));
        assert!(output.ports.contains_key("cleanup"));

        // With error
        let output = node
            .execute(serde_json::json!({"_error": "fail"}), &ctx)
            .await
            .unwrap();
        assert!(output.ports.contains_key("error"));
        assert!(output.ports.contains_key("cleanup"));
    }

    #[tokio::test]
    async fn test_error_non_string_value() {
        let mut node = TryCatchNode::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"_error": 500}), &ctx)
            .await
            .unwrap();

        assert!(output.ports.contains_key("error"));
        assert_eq!(output.ports["error"]["_caught"], true);
        assert_eq!(output.ports["error"]["_error_message"], "unknown error");
    }
}
