use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Template node — renders string templates with `{{variable}}` placeholders.
///
/// Config example:
/// ```json
/// {
///   "template": "Hello, {{name}}! You have {{count}} messages."
/// }
/// ```
///
/// Input: a JSON object whose keys match the `{{placeholders}}`.
/// Output: the rendered string.
///
/// Supports nested dot-paths: `{{user.name}}` resolves `input.user.name`.
pub struct TemplateNode {
    template: String,
}

impl Default for TemplateNode {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateNode {
    pub fn new() -> Self {
        Self {
            template: String::new(),
        }
    }
}

/// Resolve a dot-path like "user.name" on a JSON value
fn resolve_path(value: &serde_json::Value, path: &str) -> Option<String> {
    let mut current = value;

    for part in path.split('.') {
        match current {
            serde_json::Value::Object(map) => {
                current = map.get(part)?;
            }
            serde_json::Value::Array(arr) => {
                let idx: usize = part.parse().ok()?;
                current = arr.get(idx)?;
            }
            _ => return None,
        }
    }

    // Convert to string representation
    match current {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Null => Some("null".to_string()),
        other => Some(other.to_string()),
    }
}

/// Render a template string by replacing `{{key}}` with values from the data.
fn render_template(template: &str, data: &serde_json::Value) -> (String, Vec<String>) {
    let mut result = String::with_capacity(template.len());
    let mut unresolved = Vec::new();
    let mut chars = template.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '{' && chars.peek() == Some(&'{') {
            chars.next(); // consume second '{'
            // Read until '}}'
            let mut key = String::new();
            let mut found_close = false;
            while let Some(c) = chars.next() {
                if c == '}' && chars.peek() == Some(&'}') {
                    chars.next(); // consume second '}'
                    found_close = true;
                    break;
                }
                key.push(c);
            }

            let key = key.trim();
            if found_close && !key.is_empty() {
                match resolve_path(data, key) {
                    Some(val) => result.push_str(&val),
                    None => {
                        unresolved.push(key.to_string());
                        // Leave placeholder intact for debugging
                        result.push_str("{{");
                        result.push_str(key);
                        result.push_str("}}");
                    }
                }
            } else {
                // Malformed — emit as-is
                result.push_str("{{");
                result.push_str(key);
            }
        } else {
            result.push(ch);
        }
    }

    (result, unresolved)
}

#[async_trait::async_trait]
impl NodeExecutor for TemplateNode {
    fn node_type(&self) -> &str {
        "template"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        self.template = config
            .get("template")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                NodeError::InvalidConfig("missing 'template' string in config".into())
            })?
            .to_string();

        if self.template.is_empty() {
            return Err(NodeError::InvalidConfig(
                "template string must not be empty".into(),
            ));
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        // If input is a string, try to parse as JSON first
        let data = if let Some(s) = input.as_str() {
            serde_json::from_str(s).unwrap_or(input)
        } else {
            input
        };

        let (rendered, unresolved) = render_template(&self.template, &data);

        let mut output = NodeOutput::data(serde_json::Value::String(rendered));
        if !unresolved.is_empty() {
            output = output.with_log(format!("unresolved placeholders: {}", unresolved.join(", ")));
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_simple() {
        let data = serde_json::json!({"name": "Alice", "count": 3});
        let (result, unresolved) =
            render_template("Hello, {{name}}! You have {{count}} messages.", &data);
        assert_eq!(result, "Hello, Alice! You have 3 messages.");
        assert!(unresolved.is_empty());
    }

    #[test]
    fn test_render_nested() {
        let data = serde_json::json!({"user": {"name": "Bob", "age": 30}});
        let (result, _) = render_template("Name: {{user.name}}, Age: {{user.age}}", &data);
        assert_eq!(result, "Name: Bob, Age: 30");
    }

    #[test]
    fn test_render_missing_key() {
        let data = serde_json::json!({"name": "Alice"});
        let (result, unresolved) = render_template("Hello, {{name}}! Role: {{role}}", &data);
        assert_eq!(result, "Hello, Alice! Role: {{role}}");
        assert_eq!(unresolved, vec!["role"]);
    }

    #[test]
    fn test_render_no_placeholders() {
        let data = serde_json::json!({});
        let (result, unresolved) = render_template("No variables here.", &data);
        assert_eq!(result, "No variables here.");
        assert!(unresolved.is_empty());
    }

    #[test]
    fn test_resolve_path() {
        let data = serde_json::json!({"a": {"b": {"c": "deep"}}});
        assert_eq!(resolve_path(&data, "a.b.c"), Some("deep".to_string()));
        assert_eq!(resolve_path(&data, "a.b.missing"), None);
    }

    #[tokio::test]
    async fn test_init_valid() {
        let mut node = TemplateNode::new();
        node.init(&serde_json::json!({"template": "Hello {{name}}"}))
            .await
            .unwrap();
        assert_eq!(node.template, "Hello {{name}}");
    }

    #[tokio::test]
    async fn test_init_missing_template() {
        let mut node = TemplateNode::new();
        let result = node.init(&serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_init_empty_template() {
        let mut node = TemplateNode::new();
        let result = node.init(&serde_json::json!({"template": ""})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute() {
        let mut node = TemplateNode::new();
        node.init(&serde_json::json!({"template": "Hi, {{name}}!"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"name": "World"}), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"], "Hi, World!");
    }
}
