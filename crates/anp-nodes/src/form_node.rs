use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Form node — generates HTML forms from a declarative config.
///
/// Config example:
/// ```json
/// {
///   "action": "/submit",
///   "method": "POST",
///   "fields": [
///     { "name": "email", "type": "email", "label": "Email", "required": true }
///   ],
///   "submit_text": "Send",
///   "css_class": "my-form"
/// }
/// ```
///
/// Output: `{ "html": "<form ...>...</form>", "_content_type": "text/html" }`
pub struct FormNode {
    action: String,
    method: String,
    fields: Vec<FieldDef>,
    css_class: Option<String>,
    submit_text: String,
}

#[derive(Clone, Debug)]
struct FieldDef {
    name: String,
    field_type: String,
    label: String,
    required: bool,
    placeholder: String,
    options: Vec<String>,
}

impl Default for FormNode {
    fn default() -> Self {
        Self::new()
    }
}

impl FormNode {
    pub fn new() -> Self {
        Self {
            action: String::new(),
            method: "POST".to_string(),
            fields: Vec::new(),
            css_class: None,
            submit_text: "Submit".to_string(),
        }
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn render_field(field: &FieldDef) -> String {
    let escaped_label = html_escape(&field.label);
    let escaped_name = html_escape(&field.name);
    let escaped_placeholder = html_escape(&field.placeholder);
    let required_attr = if field.required { " required" } else { "" };

    let mut html = String::new();

    if field.field_type != "hidden" {
        html.push_str(&format!(
            "<label for=\"{}\">{}</label>",
            escaped_name, escaped_label
        ));
    }

    match field.field_type.as_str() {
        "textarea" => {
            html.push_str(&format!(
                "<textarea id=\"{}\" name=\"{}\" placeholder=\"{}\"{}></textarea>",
                escaped_name, escaped_name, escaped_placeholder, required_attr
            ));
        }
        "select" => {
            html.push_str(&format!(
                "<select id=\"{}\" name=\"{}\"{}>",
                escaped_name, escaped_name, required_attr
            ));
            for opt in &field.options {
                let escaped_opt = html_escape(opt);
                html.push_str(&format!(
                    "<option value=\"{}\">{}</option>",
                    escaped_opt, escaped_opt
                ));
            }
            html.push_str("</select>");
        }
        "checkbox" => {
            html.push_str(&format!(
                "<input type=\"checkbox\" id=\"{}\" name=\"{}\"{}>",
                escaped_name, escaped_name, required_attr
            ));
        }
        other => {
            let input_type = match other {
                "email" | "password" | "number" | "hidden" | "text" => other,
                _ => "text",
            };
            html.push_str(&format!(
                "<input type=\"{}\" id=\"{}\" name=\"{}\" placeholder=\"{}\"{}>",
                input_type, escaped_name, escaped_name, escaped_placeholder, required_attr
            ));
        }
    }

    html
}

#[async_trait::async_trait]
impl NodeExecutor for FormNode {
    fn node_type(&self) -> &str {
        "form"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        self.action = config
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| NodeError::InvalidConfig("missing 'action' in config".into()))?
            .to_string();

        self.method = config
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("POST")
            .to_uppercase();

        if self.method != "GET" && self.method != "POST" {
            return Err(NodeError::InvalidConfig(format!(
                "method must be GET or POST, got '{}'",
                self.method
            )));
        }

        self.css_class = config
            .get("css_class")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        self.submit_text = config
            .get("submit_text")
            .and_then(|v| v.as_str())
            .unwrap_or("Submit")
            .to_string();

        if let Some(fields) = config.get("fields").and_then(|v| v.as_array()) {
            for f in fields {
                let name = f
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if name.is_empty() {
                    return Err(NodeError::InvalidConfig(
                        "each field must have a non-empty 'name'".into(),
                    ));
                }
                let field_type = f
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("text")
                    .to_string();
                let label = f
                    .get("label")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&name)
                    .to_string();
                let required = f
                    .get("required")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let placeholder = f
                    .get("placeholder")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let options = f
                    .get("options")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();

                self.fields.push(FieldDef {
                    name,
                    field_type,
                    label,
                    required,
                    placeholder,
                    options,
                });
            }
        }

        Ok(())
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let escaped_action = html_escape(&self.action);
        let class_attr = match &self.css_class {
            Some(cls) => format!(" class=\"{}\"", html_escape(cls)),
            None => String::new(),
        };

        let mut html = format!(
            "<form action=\"{}\" method=\"{}\"{}>",
            escaped_action, self.method, class_attr
        );

        for field in &self.fields {
            html.push_str(&render_field(field));
        }

        html.push_str(&format!(
            "<button type=\"submit\">{}</button>",
            html_escape(&self.submit_text)
        ));
        html.push_str("</form>");

        Ok(NodeOutput::data(serde_json::json!({
            "html": html,
            "_content_type": "text/html"
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_form() {
        let mut node = FormNode::new();
        node.init(&serde_json::json!({
            "action": "/submit",
            "fields": [
                { "name": "username", "type": "text", "label": "Username" }
            ]
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::json!({}), &ctx).await.unwrap();
        let result = &output.ports["default"];
        let html = result["html"].as_str().unwrap();
        assert!(html.contains("<form action=\"/submit\" method=\"POST\">"));
        assert!(html.contains("<label for=\"username\">Username</label>"));
        assert!(html.contains("type=\"text\""));
        assert!(html.contains("<button type=\"submit\">Submit</button>"));
        assert_eq!(result["_content_type"], "text/html");
    }

    #[tokio::test]
    async fn test_missing_action() {
        let mut node = FormNode::new();
        let result = node.init(&serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_invalid_method() {
        let mut node = FormNode::new();
        let result = node
            .init(&serde_json::json!({
                "action": "/submit",
                "method": "DELETE"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_select_field() {
        let mut node = FormNode::new();
        node.init(&serde_json::json!({
            "action": "/submit",
            "fields": [
                { "name": "color", "type": "select", "label": "Color", "options": ["Red", "Blue"] }
            ]
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::json!({}), &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(html.contains("<select"));
        assert!(html.contains("<option value=\"Red\">Red</option>"));
        assert!(html.contains("<option value=\"Blue\">Blue</option>"));
    }

    #[tokio::test]
    async fn test_xss_escape() {
        let mut node = FormNode::new();
        node.init(&serde_json::json!({
            "action": "/submit",
            "fields": [
                { "name": "x", "type": "text", "label": "<script>alert('xss')</script>", "placeholder": "a\"b" }
            ]
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::json!({}), &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("&quot;"));
    }

    #[tokio::test]
    async fn test_textarea_and_required() {
        let mut node = FormNode::new();
        node.init(&serde_json::json!({
            "action": "/submit",
            "fields": [
                { "name": "bio", "type": "textarea", "label": "Bio", "required": true }
            ]
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::json!({}), &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(html.contains("<textarea"));
        assert!(html.contains(" required"));
    }

    #[tokio::test]
    async fn test_css_class_and_submit_text() {
        let mut node = FormNode::new();
        node.init(&serde_json::json!({
            "action": "/go",
            "css_class": "my-form",
            "submit_text": "Go!"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::json!({}), &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(html.contains("class=\"my-form\""));
        assert!(html.contains("Go!"));
    }

    #[tokio::test]
    async fn test_empty_field_name() {
        let mut node = FormNode::new();
        let result = node
            .init(&serde_json::json!({
                "action": "/submit",
                "fields": [{ "name": "", "type": "text" }]
            }))
            .await;
        assert!(result.is_err());
    }
}
