use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Env node — reads environment variables.
///
/// Config:
/// ```json
/// { "variables": ["VAR1", "VAR2"] }
/// { "variable": "SINGLE_VAR" }
/// { "prefix": "APP_" }
/// ```
///
/// SECURITY: Never logs or exposes env var values in error messages.
pub struct EnvNode {
    variables: Vec<String>,
    variable: Option<String>,
    prefix: Option<String>,
}

impl Default for EnvNode {
    fn default() -> Self {
        Self::new()
    }
}

impl EnvNode {
    pub fn new() -> Self {
        Self {
            variables: Vec::new(),
            variable: None,
            prefix: None,
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for EnvNode {
    fn node_type(&self) -> &str {
        "env"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Source
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(vars) = config.get("variables").and_then(|v| v.as_array()) {
            self.variables = vars
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }
        if let Some(var) = config.get("variable").and_then(|v| v.as_str()) {
            self.variable = Some(var.to_string());
        }
        if let Some(pfx) = config.get("prefix").and_then(|v| v.as_str()) {
            self.prefix = Some(pfx.to_string());
        }

        // Must have at least one mode configured
        if self.variables.is_empty() && self.variable.is_none() && self.prefix.is_none() {
            return Err(NodeError::InvalidConfig(
                "env node requires 'variables', 'variable', or 'prefix' in config".into(),
            ));
        }

        Ok(())
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let mut result = serde_json::Map::new();

        // Single variable mode
        if let Some(ref var) = self.variable {
            let value = std::env::var(var).unwrap_or_default();
            result.insert(var.clone(), serde_json::Value::String(value));
            return Ok(NodeOutput::data(serde_json::Value::Object(result)));
        }

        // Multiple variables mode
        if !self.variables.is_empty() {
            for var in &self.variables {
                let value = std::env::var(var).unwrap_or_default();
                result.insert(var.clone(), serde_json::Value::String(value));
            }
            return Ok(NodeOutput::data(serde_json::Value::Object(result)));
        }

        // Prefix mode
        if let Some(ref prefix) = self.prefix {
            for (key, value) in std::env::vars() {
                if key.starts_with(prefix) {
                    result.insert(key, serde_json::Value::String(value));
                }
            }
            return Ok(NodeOutput::data(serde_json::Value::Object(result)));
        }

        Ok(NodeOutput::data(serde_json::Value::Object(result)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_single_variable() {
        std::env::set_var("ANP_TEST_VAR", "hello");
        let mut node = EnvNode::new();
        node.init(&serde_json::json!({"variable": "ANP_TEST_VAR"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        assert_eq!(output.ports["default"]["ANP_TEST_VAR"], "hello");
        std::env::remove_var("ANP_TEST_VAR");
    }

    #[tokio::test]
    async fn test_multiple_variables() {
        std::env::set_var("ANP_A", "1");
        std::env::set_var("ANP_B", "2");
        let mut node = EnvNode::new();
        node.init(&serde_json::json!({"variables": ["ANP_A", "ANP_B"]}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        assert_eq!(output.ports["default"]["ANP_A"], "1");
        assert_eq!(output.ports["default"]["ANP_B"], "2");
        std::env::remove_var("ANP_A");
        std::env::remove_var("ANP_B");
    }

    #[tokio::test]
    async fn test_missing_variable_returns_empty() {
        let mut node = EnvNode::new();
        node.init(&serde_json::json!({"variable": "ANP_NONEXISTENT_VAR_12345"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        assert_eq!(
            output.ports["default"]["ANP_NONEXISTENT_VAR_12345"],
            ""
        );
    }

    #[tokio::test]
    async fn test_no_config_fails() {
        let mut node = EnvNode::new();
        let result = node.init(&serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_node_type_and_kind() {
        let node = EnvNode::new();
        assert_eq!(node.node_type(), "env");
        assert_eq!(node.node_kind(), NodeKind::Source);
    }
}
