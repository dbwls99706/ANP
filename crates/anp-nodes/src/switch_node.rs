use std::collections::HashMap;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// SwitchNode — multi-branch routing like a switch/case statement.
///
/// Config example:
/// ```json
/// {
///   "field": "status",
///   "cases": {
///     "active": "active_port",
///     "inactive": "inactive_port",
///     "pending": "pending_port"
///   },
///   "default_port": "other"
/// }
/// ```
///
/// Reads the value at the configured dot-path field from input,
/// matches against the cases map, and routes data to the corresponding port.
pub struct SwitchNode {
    field: String,
    cases: HashMap<String, String>,
    default_port: String,
}

impl Default for SwitchNode {
    fn default() -> Self {
        Self::new()
    }
}

impl SwitchNode {
    pub fn new() -> Self {
        Self {
            field: String::new(),
            cases: HashMap::new(),
            default_port: "default".to_string(),
        }
    }
}

/// Resolve a dot-path on a JSON value
fn resolve_field(value: &serde_json::Value, path: &str) -> Option<serde_json::Value> {
    let mut current = value;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    Some(current.clone())
}

/// Convert a JSON value to a string key for case matching
fn value_to_key(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

#[async_trait::async_trait]
impl NodeExecutor for SwitchNode {
    fn node_type(&self) -> &str {
        "switch"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        self.field = config
            .get("field")
            .and_then(|v| v.as_str())
            .ok_or_else(|| NodeError::InvalidConfig("missing 'field' in config".into()))?
            .to_string();

        let cases_obj = config
            .get("cases")
            .and_then(|v| v.as_object())
            .ok_or_else(|| NodeError::InvalidConfig("missing 'cases' object in config".into()))?;

        if cases_obj.is_empty() {
            return Err(NodeError::InvalidConfig(
                "cases object must not be empty".into(),
            ));
        }

        self.cases.clear();
        for (key, val) in cases_obj {
            let port = val.as_str().ok_or_else(|| {
                NodeError::InvalidConfig(format!(
                    "case '{}' value must be a string (port name)",
                    key
                ))
            })?;
            if port.is_empty() {
                return Err(NodeError::InvalidConfig(format!(
                    "case '{}' has empty port name",
                    key
                )));
            }
            self.cases.insert(key.clone(), port.to_string());
        }

        if let Some(dp) = config.get("default_port").and_then(|v| v.as_str()) {
            self.default_port = dp.to_string();
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let field_value = resolve_field(&input, &self.field);

        let key = match &field_value {
            Some(v) => value_to_key(v),
            None => "null".to_string(),
        };

        let port = self
            .cases
            .get(&key)
            .cloned()
            .unwrap_or_else(|| self.default_port.clone());

        let mut ports = HashMap::new();
        ports.insert(port.clone(), input);
        Ok(NodeOutput::multi(ports).with_log(format!(
            "field '{}' = '{}', routed to port '{}'",
            self.field, key, port
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_switch_matches_case() {
        let mut node = SwitchNode::new();
        node.init(&serde_json::json!({
            "field": "status",
            "cases": {
                "active": "active_port",
                "inactive": "inactive_port"
            }
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"status": "active", "data": 42}), &ctx)
            .await
            .unwrap();

        assert!(output.ports.contains_key("active_port"));
        assert_eq!(output.ports["active_port"]["data"], 42);
    }

    #[tokio::test]
    async fn test_switch_default_port() {
        let mut node = SwitchNode::new();
        node.init(&serde_json::json!({
            "field": "status",
            "cases": {
                "active": "active_port"
            },
            "default_port": "other"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"status": "unknown"}), &ctx)
            .await
            .unwrap();

        assert!(output.ports.contains_key("other"));
    }

    #[tokio::test]
    async fn test_switch_numeric_key() {
        let mut node = SwitchNode::new();
        node.init(&serde_json::json!({
            "field": "code",
            "cases": {
                "200": "ok",
                "404": "not_found",
                "500": "error"
            }
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"code": 404}), &ctx)
            .await
            .unwrap();

        assert!(output.ports.contains_key("not_found"));
    }

    #[tokio::test]
    async fn test_switch_dot_path() {
        let mut node = SwitchNode::new();
        node.init(&serde_json::json!({
            "field": "response.status",
            "cases": {
                "ok": "success"
            },
            "default_port": "fallback"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"response": {"status": "ok"}}), &ctx)
            .await
            .unwrap();

        assert!(output.ports.contains_key("success"));
    }

    #[tokio::test]
    async fn test_switch_missing_field() {
        let mut node = SwitchNode::new();
        node.init(&serde_json::json!({
            "field": "nonexistent",
            "cases": { "x": "port_x" },
            "default_port": "fallback"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"other": "data"}), &ctx)
            .await
            .unwrap();

        assert!(output.ports.contains_key("fallback"));
    }

    #[tokio::test]
    async fn test_switch_init_missing_field() {
        let mut node = SwitchNode::new();
        let result = node.init(&serde_json::json!({"cases": {"a": "b"}})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_switch_init_empty_cases() {
        let mut node = SwitchNode::new();
        let result = node
            .init(&serde_json::json!({"field": "x", "cases": {}}))
            .await;
        assert!(result.is_err());
    }
}
