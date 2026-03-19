use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// MarkdownRenderNode — renders markdown in a styled container for web display.
///
/// Unlike the basic `markdown` node, this wraps rendered markdown in a styled
/// div with CSS for nice typography, supporting light and dark themes.
///
/// Config example:
/// ```json
/// {
///   "theme": "dark",
///   "max_width": "800px",
///   "font_family": "system-ui, sans-serif",
///   "code_theme": "dark"
/// }
/// ```
///
/// Output: `{ "html": "<div class='markdown-body' style='...'>...</div>", "_content_type": "text/html" }`
pub struct MarkdownRenderNode {
    theme: String,
    max_width: String,
    font_family: String,
    code_theme: String,
    field: String,
}

impl Default for MarkdownRenderNode {
    fn default() -> Self {
        Self::new()
    }
}

impl MarkdownRenderNode {
    pub fn new() -> Self {
        Self {
            theme: "light".to_string(),
            max_width: "800px".to_string(),
            font_family: "system-ui, sans-serif".to_string(),
            code_theme: "light".to_string(),
            field: "text".to_string(),
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
        if (trimmed.starts_with("- ") || trimmed.starts_with("* ")) && trimmed.len() > 2 {
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
    (start..chars.len()).find(|&i| chars[i] == target)
}

fn find_double_char(chars: &[char], start: usize, target: char) -> Option<usize> {
    if chars.len() < 2 {
        return None;
    }
    (start..chars.len() - 1).find(|&i| chars[i] == target && chars[i + 1] == target)
}

fn find_single_char_not_double(chars: &[char], start: usize, target: char) -> Option<usize> {
    (start..chars.len())
        .find(|&i| chars[i] == target && (i + 1 >= chars.len() || chars[i + 1] != target))
}

fn build_container_style(theme: &str, max_width: &str, font_family: &str) -> String {
    let (bg, text_color, link_color) = match theme {
        "dark" => ("#1e1e1e", "#d4d4d4", "#6cb6ff"),
        _ => ("#ffffff", "#24292e", "#0366d6"),
    };

    format!(
        "max-width: {}; margin: 0 auto; padding: 20px; \
         background-color: {}; color: {}; \
         font-family: {}; line-height: 1.6; \
         word-wrap: break-word;",
        max_width, bg, text_color, font_family
    )
    .replace("--link-color-placeholder--", link_color)
}

fn build_inline_css(theme: &str, code_theme: &str) -> String {
    let link_color = match theme {
        "dark" => "#6cb6ff",
        _ => "#0366d6",
    };

    let (code_bg, code_text) = match code_theme {
        "dark" => ("#2d2d2d", "#d4d4d4"),
        _ => ("#f6f8fa", "#24292e"),
    };

    let heading_border = match theme {
        "dark" => "#444",
        _ => "#eaecef",
    };

    let hr_color = match theme {
        "dark" => "#444",
        _ => "#e1e4e8",
    };

    format!(
        "<style>\
         .markdown-body a {{ color: {link_color}; text-decoration: none; }} \
         .markdown-body a:hover {{ text-decoration: underline; }} \
         .markdown-body code {{ \
           background-color: {code_bg}; color: {code_text}; \
           padding: 2px 6px; border-radius: 3px; \
           font-family: 'SFMono-Regular', Consolas, 'Liberation Mono', Menlo, monospace; \
           font-size: 85%; \
         }} \
         .markdown-body h1, .markdown-body h2 {{ \
           border-bottom: 1px solid {heading_border}; padding-bottom: 0.3em; \
         }} \
         .markdown-body hr {{ \
           border: none; border-top: 1px solid {hr_color}; margin: 24px 0; \
         }} \
         .markdown-body ul {{ padding-left: 2em; }} \
         .markdown-body li {{ margin: 4px 0; }} \
         .markdown-body p {{ margin: 16px 0; }} \
         </style>",
        link_color = link_color,
        code_bg = code_bg,
        code_text = code_text,
        heading_border = heading_border,
        hr_color = hr_color,
    )
}

#[async_trait::async_trait]
impl NodeExecutor for MarkdownRenderNode {
    fn node_type(&self) -> &str {
        "markdown_render"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(t) = config.get("theme").and_then(|v| v.as_str()) {
            match t {
                "light" | "dark" => self.theme = t.to_string(),
                _ => {
                    return Err(NodeError::InvalidConfig(
                        "theme must be 'light' or 'dark'".into(),
                    ))
                }
            }
        }

        self.code_theme = config
            .get("code_theme")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.theme)
            .to_string();

        if self.code_theme != "light" && self.code_theme != "dark" {
            return Err(NodeError::InvalidConfig(
                "code_theme must be 'light' or 'dark'".into(),
            ));
        }

        if let Some(mw) = config.get("max_width").and_then(|v| v.as_str()) {
            self.max_width = mw.to_string();
        }

        if let Some(ff) = config.get("font_family").and_then(|v| v.as_str()) {
            self.font_family = ff.to_string();
        }

        if let Some(f) = config.get("field").and_then(|v| v.as_str()) {
            self.field = f.to_string();
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
        } else if let Some(s) = input.get("markdown").and_then(|v| v.as_str()) {
            s.to_string()
        } else {
            return Err(NodeError::ExecutionFailed(
                "markdown_render requires a string input in 'text' or 'markdown' field".into(),
            ));
        };

        let html_body = convert_markdown_to_html(&text);
        let container_style =
            build_container_style(&self.theme, &self.max_width, &self.font_family);
        let css = build_inline_css(&self.theme, &self.code_theme);

        let html = format!(
            "{}<div class=\"markdown-body\" style=\"{}\">{}</div>",
            css, container_style, html_body
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
    async fn test_light_theme_default() {
        let mut node = MarkdownRenderNode::new();
        node.init(&json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({ "text": "# Hello" });
        let output = node.execute(input, &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(html.contains("class=\"markdown-body\""));
        assert!(html.contains("background-color: #ffffff"));
        assert!(html.contains("<h1>Hello</h1>"));
        assert_eq!(output.ports["default"]["_content_type"], "text/html");
    }

    #[tokio::test]
    async fn test_dark_theme() {
        let mut node = MarkdownRenderNode::new();
        node.init(&json!({ "theme": "dark" })).await.unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({ "text": "Some text" });
        let output = node.execute(input, &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(html.contains("background-color: #1e1e1e"));
        assert!(html.contains("color: #d4d4d4"));
    }

    #[tokio::test]
    async fn test_invalid_theme() {
        let mut node = MarkdownRenderNode::new();
        let result = node.init(&json!({ "theme": "purple" })).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_code_styling() {
        let mut node = MarkdownRenderNode::new();
        node.init(&json!({ "code_theme": "dark" })).await.unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({ "text": "Use `println!` macro" });
        let output = node.execute(input, &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(html.contains("<code>println!</code>"));
        assert!(html.contains("background-color: #2d2d2d"));
    }

    #[tokio::test]
    async fn test_custom_max_width_and_font() {
        let mut node = MarkdownRenderNode::new();
        node.init(&json!({
            "max_width": "1200px",
            "font_family": "Georgia, serif"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({ "text": "Hello" });
        let output = node.execute(input, &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(html.contains("max-width: 1200px"));
        assert!(html.contains("Georgia, serif"));
    }

    #[tokio::test]
    async fn test_markdown_field_input() {
        let mut node = MarkdownRenderNode::new();
        node.init(&json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({ "markdown": "**bold**" });
        let output = node.execute(input, &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(html.contains("<strong>bold</strong>"));
    }

    #[tokio::test]
    async fn test_missing_input() {
        let mut node = MarkdownRenderNode::new();
        node.init(&json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let result = node.execute(json!({ "other": 123 }), &ctx).await;
        assert!(result.is_err());
    }
}
