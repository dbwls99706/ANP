use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// HTML Template node — renders full HTML pages with `{{variable}}` placeholders.
///
/// Unlike the basic `template` node, this is designed for HTML content and
/// outputs structured data that `http_server` can serve as `text/html`.
///
/// Config example:
/// ```json
/// {
///   "html": "<!DOCTYPE html><html><body><h1>{{title}}</h1></body></html>"
/// }
/// ```
///
/// Input: a JSON object whose keys match the `{{placeholders}}`.
/// Output: `{ "html": "<rendered>", "_content_type": "text/html" }`
pub struct HtmlTemplateNode {
    html: String,
}

impl Default for HtmlTemplateNode {
    fn default() -> Self {
        Self::new()
    }
}

impl HtmlTemplateNode {
    pub fn new() -> Self {
        Self {
            html: String::new(),
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

    match current {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Null => Some("".to_string()),
        other => Some(other.to_string()),
    }
}

/// Render an HTML template by replacing `{{key}}` with values from the data.
/// HTML-escapes values by default. Use `{{{key}}}` for raw (unescaped) output.
fn render_html(template: &str, data: &serde_json::Value) -> (String, Vec<String>) {
    let mut result = String::with_capacity(template.len());
    let mut unresolved = Vec::new();
    let mut chars = template.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '{' && chars.peek() == Some(&'{') {
            chars.next(); // consume second '{'

            // Check for triple-brace (raw/unescaped)
            let raw = chars.peek() == Some(&'{');
            if raw {
                chars.next(); // consume third '{'
            }

            // Read key until closing braces
            let mut key = String::new();
            let mut found_close = false;
            while let Some(c) = chars.next() {
                if c == '}' && chars.peek() == Some(&'}') {
                    chars.next(); // consume second '}'
                    if raw {
                        // Consume third '}' for triple-brace
                        if chars.peek() == Some(&'}') {
                            chars.next();
                        }
                    }
                    found_close = true;
                    break;
                }
                key.push(c);
            }

            let key = key.trim();
            if found_close && !key.is_empty() {
                match resolve_path(data, key) {
                    Some(val) => {
                        if raw {
                            result.push_str(&val);
                        } else {
                            result.push_str(&html_escape(&val));
                        }
                    }
                    None => {
                        unresolved.push(key.to_string());
                        result.push_str("{{");
                        result.push_str(key);
                        result.push_str("}}");
                    }
                }
            } else {
                result.push_str("{{");
                result.push_str(key);
            }
        } else {
            result.push(ch);
        }
    }

    (result, unresolved)
}

/// Basic HTML escaping to prevent XSS
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

#[async_trait::async_trait]
impl NodeExecutor for HtmlTemplateNode {
    fn node_type(&self) -> &str {
        "html_template"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        self.html = config
            .get("html")
            .and_then(|v| v.as_str())
            .ok_or_else(|| NodeError::InvalidConfig("missing 'html' string in config".into()))?
            .to_string();

        if self.html.is_empty() {
            return Err(NodeError::InvalidConfig(
                "html template must not be empty".into(),
            ));
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let data = if let Some(s) = input.as_str() {
            serde_json::from_str(s).unwrap_or(input)
        } else {
            input
        };

        let (rendered, unresolved) = render_html(&self.html, &data);

        let mut output = NodeOutput::data(serde_json::json!({
            "html": rendered,
            "_content_type": "text/html"
        }));

        if !unresolved.is_empty() {
            output = output.with_log(format!(
                "unresolved placeholders: {}",
                unresolved.join(", ")
            ));
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_basic_html() {
        let data = serde_json::json!({"title": "Hello", "body": "World"});
        let (result, unresolved) = render_html("<h1>{{title}}</h1><p>{{body}}</p>", &data);
        assert_eq!(result, "<h1>Hello</h1><p>World</p>");
        assert!(unresolved.is_empty());
    }

    #[test]
    fn test_html_escaping() {
        let data = serde_json::json!({"name": "<script>alert('xss')</script>"});
        let (result, _) = render_html("<p>{{name}}</p>", &data);
        assert_eq!(
            result,
            "<p>&lt;script&gt;alert(&#x27;xss&#x27;)&lt;/script&gt;</p>"
        );
    }

    #[test]
    fn test_raw_triple_brace() {
        let data = serde_json::json!({"content": "<b>bold</b>"});
        let (result, _) = render_html("<div>{{{content}}}</div>", &data);
        assert_eq!(result, "<div><b>bold</b></div>");
    }

    #[test]
    fn test_nested_path() {
        let data = serde_json::json!({"page": {"title": "ANP", "desc": "AI Native"}});
        let (result, _) = render_html(
            "<title>{{page.title}}</title><meta content=\"{{page.desc}}\">",
            &data,
        );
        assert_eq!(result, "<title>ANP</title><meta content=\"AI Native\">");
    }

    #[test]
    fn test_missing_placeholder() {
        let data = serde_json::json!({"title": "Hello"});
        let (result, unresolved) = render_html("<h1>{{title}}</h1><p>{{missing}}</p>", &data);
        assert_eq!(result, "<h1>Hello</h1><p>{{missing}}</p>");
        assert_eq!(unresolved, vec!["missing"]);
    }

    #[tokio::test]
    async fn test_execute_output_format() {
        let mut node = HtmlTemplateNode::new();
        node.init(&serde_json::json!({
            "html": "<h1>{{title}}</h1>"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"title": "Test"}), &ctx)
            .await
            .unwrap();

        let result = &output.ports["default"];
        assert_eq!(result["html"], "<h1>Test</h1>");
        assert_eq!(result["_content_type"], "text/html");
    }

    #[tokio::test]
    async fn test_init_missing_html() {
        let mut node = HtmlTemplateNode::new();
        assert!(node.init(&serde_json::json!({})).await.is_err());
    }

    #[tokio::test]
    async fn test_init_empty_html() {
        let mut node = HtmlTemplateNode::new();
        assert!(node.init(&serde_json::json!({"html": ""})).await.is_err());
    }
}
