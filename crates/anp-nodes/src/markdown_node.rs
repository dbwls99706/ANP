use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// MarkdownNode — basic Markdown to HTML conversion
pub struct MarkdownNode {
    field: String,
    wrap: bool,
}

impl Default for MarkdownNode {
    fn default() -> Self {
        Self::new()
    }
}

impl MarkdownNode {
    pub fn new() -> Self {
        Self {
            field: "text".to_string(),
            wrap: true,
        }
    }
}

fn convert_markdown_to_html(md: &str) -> String {
    let lines: Vec<&str> = md.lines().collect();
    let mut html_parts: Vec<String> = Vec::new();
    let mut i = 0;
    let mut paragraph_lines: Vec<String> = Vec::new();

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // Blank line — flush paragraph
        if trimmed.is_empty() {
            flush_paragraph(&mut paragraph_lines, &mut html_parts);
            i += 1;
            continue;
        }

        // Horizontal rule
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            flush_paragraph(&mut paragraph_lines, &mut html_parts);
            html_parts.push("<hr>".to_string());
            i += 1;
            continue;
        }

        // Headings
        if trimmed.starts_with('#') {
            flush_paragraph(&mut paragraph_lines, &mut html_parts);
            let level = trimmed.chars().take_while(|c| *c == '#').count().min(6);
            let text = trimmed[level..].trim();
            let text = inline_format(text);
            html_parts.push(format!("<h{}>{}</h{}>", level, text, level));
            i += 1;
            continue;
        }

        // List items
        if (trimmed.starts_with("- ") || trimmed.starts_with("* "))
            && trimmed.len() > 2
        {
            flush_paragraph(&mut paragraph_lines, &mut html_parts);
            let mut list_items: Vec<String> = Vec::new();
            while i < lines.len() {
                let lt = lines[i].trim();
                if (lt.starts_with("- ") || lt.starts_with("* ")) && lt.len() > 2 {
                    let item_text = &lt[2..];
                    list_items.push(format!("<li>{}</li>", inline_format(item_text)));
                    i += 1;
                } else {
                    break;
                }
            }
            html_parts.push(format!("<ul>{}</ul>", list_items.join("")));
            continue;
        }

        // Regular text — accumulate for paragraph
        paragraph_lines.push(inline_format(trimmed));
        i += 1;
    }

    flush_paragraph(&mut paragraph_lines, &mut html_parts);
    html_parts.join("")
}

fn flush_paragraph(lines: &mut Vec<String>, parts: &mut Vec<String>) {
    if !lines.is_empty() {
        let content = lines.join(" ");
        parts.push(format!("<p>{}</p>", content));
        lines.clear();
    }
}

fn inline_format(text: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Links: [text](url)
        if chars[i] == '[' {
            if let Some(close_bracket) = find_char(&chars, i + 1, ']') {
                if close_bracket + 1 < len && chars[close_bracket + 1] == '(' {
                    if let Some(close_paren) = find_char(&chars, close_bracket + 2, ')') {
                        let link_text: String = chars[i + 1..close_bracket].iter().collect();
                        let url: String = chars[close_bracket + 2..close_paren].iter().collect();
                        result.push_str(&format!("<a href=\"{}\">{}</a>", url, link_text));
                        i = close_paren + 1;
                        continue;
                    }
                }
            }
        }

        // Bold: **text**
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            if let Some(end) = find_double_char(&chars, i + 2, '*') {
                let inner: String = chars[i + 2..end].iter().collect();
                result.push_str(&format!("<strong>{}</strong>", inner));
                i = end + 2;
                continue;
            }
        }

        // Italic: *text*
        if chars[i] == '*' && (i + 1 < len && chars[i + 1] != '*') {
            if let Some(end) = find_single_char_not_double(&chars, i + 1, '*') {
                let inner: String = chars[i + 1..end].iter().collect();
                result.push_str(&format!("<em>{}</em>", inner));
                i = end + 1;
                continue;
            }
        }

        // Inline code: `text`
        if chars[i] == '`' {
            if let Some(end) = find_char(&chars, i + 1, '`') {
                let inner: String = chars[i + 1..end].iter().collect();
                result.push_str(&format!("<code>{}</code>", inner));
                i = end + 1;
                continue;
            }
        }

        result.push(chars[i]);
        i += 1;
    }

    result
}

fn find_char(chars: &[char], start: usize, target: char) -> Option<usize> {
    for i in start..chars.len() {
        if chars[i] == target {
            return Some(i);
        }
    }
    None
}

fn find_double_char(chars: &[char], start: usize, target: char) -> Option<usize> {
    for i in start..chars.len() - 1 {
        if chars[i] == target && chars[i + 1] == target {
            return Some(i);
        }
    }
    None
}

fn find_single_char_not_double(chars: &[char], start: usize, target: char) -> Option<usize> {
    for i in start..chars.len() {
        if chars[i] == target {
            if i + 1 >= chars.len() || chars[i + 1] != target {
                return Some(i);
            }
        }
    }
    None
}

#[async_trait::async_trait]
impl NodeExecutor for MarkdownNode {
    fn node_type(&self) -> &str {
        "markdown"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(f) = config.get("field").and_then(|v| v.as_str()) {
            self.field = f.to_string();
        }
        if let Some(w) = config.get("wrap").and_then(|v| v.as_bool()) {
            self.wrap = w;
        }
        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let text = if let Some(s) = input.as_str() {
            s.to_string()
        } else if let Some(s) = input.get(&self.field).and_then(|v| v.as_str()) {
            s.to_string()
        } else {
            return Err(NodeError::ExecutionFailed(
                "markdown requires a string input".into(),
            ));
        };

        let html_body = convert_markdown_to_html(&text);

        let html = if self.wrap {
            format!("<html><body>{}</body></html>", html_body)
        } else {
            html_body
        };

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

    fn ctx() -> ExecutionContext {
        ExecutionContext::new("test")
    }

    #[tokio::test]
    async fn test_headings() {
        let mut node = MarkdownNode::new();
        node.init(&json!({"wrap": false})).await.unwrap();
        let input = json!({"text": "# Title\n## Subtitle"});
        let output = node.execute(input, &ctx()).await.unwrap();
        let html = output.data.unwrap()["html"].as_str().unwrap().to_string();
        assert!(html.contains("<h1>Title</h1>"));
        assert!(html.contains("<h2>Subtitle</h2>"));
    }

    #[tokio::test]
    async fn test_bold_and_italic() {
        let mut node = MarkdownNode::new();
        node.init(&json!({"wrap": false})).await.unwrap();
        let input = json!({"text": "This is **bold** and *italic* text"});
        let output = node.execute(input, &ctx()).await.unwrap();
        let html = output.data.unwrap()["html"].as_str().unwrap().to_string();
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<em>italic</em>"));
    }

    #[tokio::test]
    async fn test_inline_code() {
        let mut node = MarkdownNode::new();
        node.init(&json!({"wrap": false})).await.unwrap();
        let input = json!({"text": "Use `println!` macro"});
        let output = node.execute(input, &ctx()).await.unwrap();
        let html = output.data.unwrap()["html"].as_str().unwrap().to_string();
        assert!(html.contains("<code>println!</code>"));
    }

    #[tokio::test]
    async fn test_list_items() {
        let mut node = MarkdownNode::new();
        node.init(&json!({"wrap": false})).await.unwrap();
        let input = json!({"text": "- item 1\n- item 2\n- item 3"});
        let output = node.execute(input, &ctx()).await.unwrap();
        let html = output.data.unwrap()["html"].as_str().unwrap().to_string();
        assert!(html.contains("<ul>"));
        assert!(html.contains("<li>item 1</li>"));
        assert!(html.contains("<li>item 2</li>"));
        assert!(html.contains("<li>item 3</li>"));
    }

    #[tokio::test]
    async fn test_links() {
        let mut node = MarkdownNode::new();
        node.init(&json!({"wrap": false})).await.unwrap();
        let input = json!({"text": "Visit [Google](https://google.com)"});
        let output = node.execute(input, &ctx()).await.unwrap();
        let html = output.data.unwrap()["html"].as_str().unwrap().to_string();
        assert!(html.contains("<a href=\"https://google.com\">Google</a>"));
    }

    #[tokio::test]
    async fn test_horizontal_rule() {
        let mut node = MarkdownNode::new();
        node.init(&json!({"wrap": false})).await.unwrap();
        let input = json!({"text": "above\n\n---\n\nbelow"});
        let output = node.execute(input, &ctx()).await.unwrap();
        let html = output.data.unwrap()["html"].as_str().unwrap().to_string();
        assert!(html.contains("<hr>"));
    }

    #[tokio::test]
    async fn test_wrap_option() {
        let mut node = MarkdownNode::new();
        node.init(&json!({"wrap": true})).await.unwrap();
        let input = json!({"text": "# Hello"});
        let output = node.execute(input, &ctx()).await.unwrap();
        let html = output.data.unwrap()["html"].as_str().unwrap().to_string();
        assert!(html.starts_with("<html><body>"));
        assert!(html.ends_with("</body></html>"));
    }
}
