use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// LayoutNode — wraps HTML content in a full HTML5 page template.
///
/// Config example:
/// ```json
/// {
///   "title": "{{page_title}}",
///   "head_extra": "<link rel='stylesheet' href='/style.css'>",
///   "body_class": "main",
///   "header_html": "<header><h1>My Site</h1></header>",
///   "footer_html": "<footer>Copyright 2024</footer>",
///   "nav_html": "<nav><a href='/'>Home</a></nav>",
///   "css": "body { margin: 0; }"
/// }
/// ```
///
/// Input: `{ "html": "<p>content</p>", "page_title": "My Page" }`
/// Output: `{ "html": "<!DOCTYPE html><html>...</html>", "_content_type": "text/html" }`
pub struct LayoutNode {
    title: String,
    head_extra: String,
    body_class: Option<String>,
    header_html: Option<String>,
    footer_html: Option<String>,
    nav_html: Option<String>,
    css: Option<String>,
}

impl Default for LayoutNode {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutNode {
    pub fn new() -> Self {
        Self {
            title: String::new(),
            head_extra: String::new(),
            body_class: None,
            header_html: None,
            footer_html: None,
            nav_html: None,
            css: None,
        }
    }
}

/// Replace `{{variable}}` placeholders with values from a JSON object.
/// Supports dot-path resolution like `{{user.name}}`.
fn replace_placeholders(template: &str, data: &serde_json::Value) -> String {
    let mut result = String::new();
    let chars: Vec<char> = template.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if i + 1 < len && chars[i] == '{' && chars[i + 1] == '{' {
            // Find closing }}
            if let Some(end) = find_closing_braces(&chars, i + 2) {
                let key: String = chars[i + 2..end].iter().collect();
                let key = key.trim();
                let value = resolve_path(data, key).unwrap_or_default();
                result.push_str(&value);
                i = end + 2;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }

    result
}

fn find_closing_braces(chars: &[char], start: usize) -> Option<usize> {
    let len = chars.len();
    if len < 2 {
        return None;
    }
    (start..len - 1).find(|&i| chars[i] == '}' && chars[i + 1] == '}')
}

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

#[async_trait::async_trait]
impl NodeExecutor for LayoutNode {
    fn node_type(&self) -> &str {
        "layout"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        self.title = config
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        self.head_extra = config
            .get("head_extra")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        self.body_class = config
            .get("body_class")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        self.header_html = config
            .get("header_html")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        self.footer_html = config
            .get("footer_html")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        self.nav_html = config
            .get("nav_html")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        self.css = config
            .get("css")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        // Get body content from input
        let body_content = input
            .get("html")
            .and_then(|v| v.as_str())
            .or_else(|| input.get("body").and_then(|v| v.as_str()))
            .unwrap_or("");

        // Replace placeholders in all config strings
        let title = replace_placeholders(&self.title, &input);
        let head_extra = replace_placeholders(&self.head_extra, &input);
        let header_html = self
            .header_html
            .as_ref()
            .map(|h| replace_placeholders(h, &input))
            .unwrap_or_default();
        let nav_html = self
            .nav_html
            .as_ref()
            .map(|n| replace_placeholders(n, &input))
            .unwrap_or_default();
        let footer_html = self
            .footer_html
            .as_ref()
            .map(|f| replace_placeholders(f, &input))
            .unwrap_or_default();
        let body_content = replace_placeholders(body_content, &input);

        let body_class_attr = match &self.body_class {
            Some(cls) => {
                let cls = replace_placeholders(cls, &input);
                format!(" class=\"{}\"", cls)
            }
            None => String::new(),
        };

        let style_tag = match &self.css {
            Some(css) => {
                let css = replace_placeholders(css, &input);
                format!("<style>{}</style>", css)
            }
            None => String::new(),
        };

        let html = format!(
            "<!DOCTYPE html>\
             <html>\
             <head>\
             <meta charset=\"UTF-8\">\
             <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\
             <title>{title}</title>\
             {style_tag}\
             {head_extra}\
             </head>\
             <body{body_class_attr}>\
             {nav_html}\
             {header_html}\
             {body_content}\
             {footer_html}\
             </body>\
             </html>",
            title = title,
            style_tag = style_tag,
            head_extra = head_extra,
            body_class_attr = body_class_attr,
            nav_html = nav_html,
            header_html = header_html,
            body_content = body_content,
            footer_html = footer_html,
        );

        Ok(NodeOutput::data(serde_json::json!({
            "html": html,
            "_content_type": "text/html"
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_basic_layout() {
        let mut node = LayoutNode::new();
        node.init(&json!({ "title": "Test Page" })).await.unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({ "html": "<p>Hello</p>" });
        let output = node.execute(input, &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<title>Test Page</title>"));
        assert!(html.contains("<p>Hello</p>"));
        assert!(html.contains("<meta charset=\"UTF-8\">"));
        assert_eq!(output.ports["default"]["_content_type"], "text/html");
    }

    #[tokio::test]
    async fn test_template_variables() {
        let mut node = LayoutNode::new();
        node.init(&json!({ "title": "{{page_title}}" }))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({
            "html": "<p>Content</p>",
            "page_title": "Dynamic Title"
        });
        let output = node.execute(input, &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(html.contains("<title>Dynamic Title</title>"));
    }

    #[tokio::test]
    async fn test_header_nav_footer() {
        let mut node = LayoutNode::new();
        node.init(&json!({
            "title": "Full",
            "header_html": "<header>HEAD</header>",
            "nav_html": "<nav>NAV</nav>",
            "footer_html": "<footer>FOOT</footer>"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({ "html": "<main>BODY</main>" });
        let output = node.execute(input, &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(html.contains("<header>HEAD</header>"));
        assert!(html.contains("<nav>NAV</nav>"));
        assert!(html.contains("<footer>FOOT</footer>"));
        assert!(html.contains("<main>BODY</main>"));
    }

    #[tokio::test]
    async fn test_css_and_body_class() {
        let mut node = LayoutNode::new();
        node.init(&json!({
            "title": "Styled",
            "css": "body { margin: 0; }",
            "body_class": "main-page"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({ "html": "<p>x</p>" });
        let output = node.execute(input, &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(html.contains("<style>body { margin: 0; }</style>"));
        assert!(html.contains("class=\"main-page\""));
    }

    #[tokio::test]
    async fn test_body_field_input() {
        let mut node = LayoutNode::new();
        node.init(&json!({ "title": "Test" })).await.unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({ "body": "<div>Body content</div>" });
        let output = node.execute(input, &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(html.contains("<div>Body content</div>"));
    }

    #[tokio::test]
    async fn test_head_extra() {
        let mut node = LayoutNode::new();
        node.init(&json!({
            "title": "Test",
            "head_extra": "<link rel=\"stylesheet\" href=\"/style.css\">"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({ "html": "" });
        let output = node.execute(input, &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(html.contains("<link rel=\"stylesheet\" href=\"/style.css\">"));
    }

    #[tokio::test]
    async fn test_empty_input() {
        let mut node = LayoutNode::new();
        node.init(&json!({ "title": "Empty" })).await.unwrap();

        let ctx = ExecutionContext::new();
        let output = node.execute(json!({}), &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<title>Empty</title>"));
    }
}
