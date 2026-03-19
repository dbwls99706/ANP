use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Log node — structured logging sink that passes through data.
///
/// Config:
/// ```json
/// { "level": "info", "message": "Processing {{name}} with status {{status}}" }
/// ```
///
/// If `message` template is provided, `{{placeholder}}` values are resolved from input.
/// If no template, logs the entire input as JSON.
/// Always passes through input data unchanged.
pub struct LogNode {
    level: String,
    message: Option<String>,
}

impl Default for LogNode {
    fn default() -> Self {
        Self::new()
    }
}

impl LogNode {
    pub fn new() -> Self {
        Self {
            level: "info".to_string(),
            message: None,
        }
    }
}

/// Render a template string by replacing `{{key}}` with values from the input.
fn render_template(template: &str, data: &serde_json::Value) -> String {
    let mut result = template.to_string();
    // Find all {{...}} placeholders and replace them
    loop {
        let start = result.find("{{");
        let end = result.find("}}");
        match (start, end) {
            (Some(s), Some(e)) if e > s => {
                let key = &result[s + 2..e].trim();
                let replacement = resolve_dot_path(data, key)
                    .map(|v| match v {
                        serde_json::Value::String(s) => s,
                        other => other.to_string(),
                    })
                    .unwrap_or_default();
                result = format!("{}{}{}", &result[..s], replacement, &result[e + 2..]);
            }
            _ => break,
        }
    }
    result
}

/// Resolve a dot-path on a JSON value.
fn resolve_dot_path(value: &serde_json::Value, path: &str) -> Option<serde_json::Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = value;
    for part in parts {
        current = current.get(part)?;
    }
    Some(current.clone())
}

#[async_trait::async_trait]
impl NodeExecutor for LogNode {
    fn node_type(&self) -> &str {
        "log"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Sink
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(level) = config.get("level").and_then(|v| v.as_str()) {
            self.level = level.to_string();
        }
        if let Some(msg) = config.get("message").and_then(|v| v.as_str()) {
            self.message = Some(msg.to_string());
        }
        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let log_message = if let Some(ref template) = self.message {
            render_template(template, &input)
        } else if input.is_null() {
            "(null)".to_string()
        } else if let Some(s) = input.as_str() {
            s.to_string()
        } else {
            serde_json::to_string_pretty(&input).unwrap_or_else(|_| format!("{:?}", input))
        };

        match self.level.as_str() {
            "error" => tracing::error!("{}", log_message),
            "warn" => tracing::warn!("{}", log_message),
            "debug" => tracing::debug!("{}", log_message),
            "trace" => tracing::trace!("{}", log_message),
            _ => tracing::info!("{}", log_message),
        }

        // Pass through input data unchanged
        Ok(NodeOutput::data(input))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_log_passthrough() {
        let mut node = LogNode::new();
        node.init(&serde_json::json!({"level": "info"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let input = serde_json::json!({"key": "value"});
        let output = node.execute(input.clone(), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], input);
    }

    #[test]
    fn test_render_template() {
        let data = serde_json::json!({"name": "Alice", "status": "ok"});
        let result = render_template("Hello {{name}}, status: {{status}}", &data);
        assert_eq!(result, "Hello Alice, status: ok");
    }

    #[test]
    fn test_render_template_missing_key() {
        let data = serde_json::json!({"name": "Alice"});
        let result = render_template("Hello {{name}}, age: {{age}}", &data);
        assert_eq!(result, "Hello Alice, age: ");
    }

    #[test]
    fn test_render_template_dot_path() {
        let data = serde_json::json!({"user": {"name": "Bob"}});
        let result = render_template("Hello {{user.name}}", &data);
        assert_eq!(result, "Hello Bob");
    }

    #[tokio::test]
    async fn test_node_type_and_kind() {
        let node = LogNode::new();
        assert_eq!(node.node_type(), "log");
        assert_eq!(node.node_kind(), NodeKind::Sink);
    }
}
