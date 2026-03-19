use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// SanitizeNode — Input sanitization for security.
///
/// Operations: html, sql, xss, trim, all
///
/// Config:
/// ```json
/// {
///   "operation": "all",
///   "field": "user.name",
///   "max_length": 255
/// }
/// ```
pub struct SanitizeNode {
    operation: String,
    field: Option<String>,
    max_length: Option<usize>,
}

impl Default for SanitizeNode {
    fn default() -> Self {
        Self::new()
    }
}

impl SanitizeNode {
    pub fn new() -> Self {
        Self {
            operation: "all".to_string(),
            field: None,
            max_length: None,
        }
    }
}

fn sanitize_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn sanitize_sql(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('\'', "''")
}

fn sanitize_xss(input: &str) -> String {
    // Strip <script>...</script> tags (case insensitive)
    let mut result = String::new();
    let lower = input.to_lowercase();
    let bytes = input.as_bytes();
    let lower_bytes = lower.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // Check for <script
        if i + 7 <= len && &lower[i..i + 7] == "<script" {
            // Find the closing </script>
            if let Some(close_start) = lower[i..].find("</script") {
                let close_pos = i + close_start;
                // Find the > after </script
                if let Some(gt_offset) = lower[close_pos..].find('>') {
                    i = close_pos + gt_offset + 1;
                    continue;
                }
            }
            // If no closing tag found, skip the opening tag
            if let Some(gt_offset) = input[i..].find('>') {
                i = i + gt_offset + 1;
                continue;
            }
        }

        // Check for event handlers in attributes: on<word>=
        // We look for patterns like onclick="..." onerror='...' onload=...
        if i + 2 < len
            && lower_bytes[i] == b'o'
            && lower_bytes[i + 1] == b'n'
            && (i + 2 < len && lower_bytes[i + 2].is_ascii_alphabetic())
        {
            // Check if preceded by a space or start (attribute context)
            let preceded_by_space = i == 0 || bytes[i - 1] == b' ' || bytes[i - 1] == b'"' || bytes[i - 1] == b'\'' || bytes[i - 1] == b'\t';
            if preceded_by_space {
                // Find the = sign
                let mut j = i + 2;
                while j < len && lower_bytes[j].is_ascii_alphabetic() {
                    j += 1;
                }
                if j < len && lower_bytes[j] == b'=' {
                    // Skip the entire attribute value
                    j += 1;
                    // Skip optional whitespace
                    while j < len && lower_bytes[j] == b' ' {
                        j += 1;
                    }
                    if j < len && (lower_bytes[j] == b'"' || lower_bytes[j] == b'\'') {
                        let quote = lower_bytes[j];
                        j += 1;
                        while j < len && lower_bytes[j] != quote {
                            j += 1;
                        }
                        if j < len {
                            j += 1; // skip closing quote
                        }
                    } else {
                        // Unquoted value — skip until space or >
                        while j < len && lower_bytes[j] != b' ' && lower_bytes[j] != b'>' {
                            j += 1;
                        }
                    }
                    i = j;
                    continue;
                }
            }
        }

        result.push(bytes[i] as char);
        i += 1;
    }

    result
}

fn sanitize_trim(input: &str) -> String {
    let trimmed = input.trim();
    let mut result = String::with_capacity(trimmed.len());
    let mut prev_was_whitespace = false;
    for ch in trimmed.chars() {
        if ch.is_whitespace() {
            if !prev_was_whitespace {
                result.push(' ');
            }
            prev_was_whitespace = true;
        } else {
            result.push(ch);
            prev_was_whitespace = false;
        }
    }
    result
}

fn apply_sanitization(input: &str, operation: &str) -> String {
    match operation {
        "html" => sanitize_html(input),
        "sql" => sanitize_sql(input),
        "xss" => sanitize_xss(input),
        "trim" => sanitize_trim(input),
        "all" => {
            let s = sanitize_xss(input);
            let s = sanitize_html(&s);
            sanitize_trim(&s)
        }
        _ => input.to_string(),
    }
}

fn apply_max_length(input: String, max_length: Option<usize>) -> String {
    match max_length {
        Some(max) if input.len() > max => input.chars().take(max).collect(),
        _ => input,
    }
}

/// Resolve a dot-path from a JSON value.
fn resolve_field<'a>(value: &'a serde_json::Value, field: &str) -> Option<&'a serde_json::Value> {
    let mut current = value;
    for part in field.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

/// Set a value at a dot-path in a JSON object.
fn set_field(value: &mut serde_json::Value, field: &str, new_val: serde_json::Value) {
    let parts: Vec<&str> = field.split('.').collect();
    if parts.is_empty() {
        return;
    }
    if parts.len() == 1 {
        if let Some(obj) = value.as_object_mut() {
            obj.insert(parts[0].to_string(), new_val);
        }
        return;
    }
    // Navigate to the parent object
    let mut current = value;
    for part in &parts[..parts.len() - 1] {
        match current.get_mut(*part) {
            Some(v) => current = v,
            None => return,
        }
    }
    if let Some(obj) = current.as_object_mut() {
        obj.insert(parts[parts.len() - 1].to_string(), new_val);
    }
}

/// Recursively sanitize all string values in a JSON value.
fn sanitize_recursive(
    value: &mut serde_json::Value,
    operation: &str,
    max_length: Option<usize>,
) {
    match value {
        serde_json::Value::String(s) => {
            let sanitized = apply_sanitization(s, operation);
            let sanitized = apply_max_length(sanitized, max_length);
            *s = sanitized;
        }
        serde_json::Value::Object(map) => {
            for (_, v) in map.iter_mut() {
                sanitize_recursive(v, operation, max_length);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                sanitize_recursive(v, operation, max_length);
            }
        }
        _ => {}
    }
}

#[async_trait::async_trait]
impl NodeExecutor for SanitizeNode {
    fn node_type(&self) -> &str {
        "sanitize"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "html" | "sql" | "xss" | "trim" | "all" => {
                    self.operation = op.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown operation: '{}'. Use: html, sql, xss, trim, all",
                        other
                    )));
                }
            }
        }

        if let Some(field) = config.get("field").and_then(|v| v.as_str()) {
            self.field = Some(field.to_string());
        }

        if let Some(max) = config.get("max_length").and_then(|v| v.as_u64()) {
            self.max_length = Some(max as usize);
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let mut result = input.clone();

        if let Some(ref field_path) = self.field {
            // Sanitize only the specified field
            let field_val = resolve_field(&input, field_path)
                .ok_or_else(|| {
                    NodeError::ExecutionFailed(format!(
                        "field '{}' not found in input",
                        field_path
                    ))
                })?;

            if let Some(s) = field_val.as_str() {
                let sanitized = apply_sanitization(s, &self.operation);
                let sanitized = apply_max_length(sanitized, self.max_length);
                set_field(&mut result, field_path, serde_json::Value::String(sanitized));
            }
        } else {
            // Recursively sanitize all string values
            sanitize_recursive(&mut result, &self.operation, self.max_length);
        }

        // Add _sanitized marker
        if let Some(obj) = result.as_object_mut() {
            obj.insert("_sanitized".to_string(), serde_json::Value::Bool(true));
        }

        Ok(NodeOutput::data(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_html_sanitization() {
        let mut node = SanitizeNode::new();
        node.init(&serde_json::json!({"operation": "html", "field": "text"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({"text": "<script>alert('xss')</script>"}),
                &ctx,
            )
            .await
            .unwrap();

        let data = &output.ports["default"];
        let text = data["text"].as_str().unwrap();
        assert!(!text.contains('<'));
        assert!(!text.contains('>'));
        assert!(text.contains("&lt;"));
        assert!(text.contains("&gt;"));
        assert_eq!(data["_sanitized"], true);
    }

    #[tokio::test]
    async fn test_sql_sanitization() {
        let mut node = SanitizeNode::new();
        node.init(&serde_json::json!({"operation": "sql", "field": "query"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({"query": "Robert'; DROP TABLE students;--"}),
                &ctx,
            )
            .await
            .unwrap();

        let data = &output.ports["default"];
        let query = data["query"].as_str().unwrap();
        // Single quotes should be doubled
        assert!(query.contains("''"));
        // The original unescaped single quote injection should not be present
        // Original: Robert' -> Escaped: Robert''
        assert_eq!(query, "Robert''; DROP TABLE students;--");
    }

    #[tokio::test]
    async fn test_xss_strips_script_tags() {
        let mut node = SanitizeNode::new();
        node.init(&serde_json::json!({"operation": "xss", "field": "html"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({"html": "Hello<script>alert(1)</script>World"}),
                &ctx,
            )
            .await
            .unwrap();

        let data = &output.ports["default"];
        let html = data["html"].as_str().unwrap();
        assert_eq!(html, "HelloWorld");
    }

    #[tokio::test]
    async fn test_trim_sanitization() {
        let mut node = SanitizeNode::new();
        node.init(&serde_json::json!({"operation": "trim", "field": "text"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({"text": "  hello   world  "}),
                &ctx,
            )
            .await
            .unwrap();

        let data = &output.ports["default"];
        assert_eq!(data["text"], "hello world");
    }

    #[tokio::test]
    async fn test_recursive_sanitization() {
        let mut node = SanitizeNode::new();
        node.init(&serde_json::json!({"operation": "html"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({
                    "name": "<b>John</b>",
                    "nested": {
                        "bio": "A & B"
                    }
                }),
                &ctx,
            )
            .await
            .unwrap();

        let data = &output.ports["default"];
        assert!(data["name"].as_str().unwrap().contains("&lt;"));
        assert!(data["nested"]["bio"].as_str().unwrap().contains("&amp;"));
        assert_eq!(data["_sanitized"], true);
    }

    #[tokio::test]
    async fn test_max_length_truncation() {
        let mut node = SanitizeNode::new();
        node.init(&serde_json::json!({
            "operation": "trim",
            "field": "text",
            "max_length": 5
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"text": "hello world"}), &ctx)
            .await
            .unwrap();

        let data = &output.ports["default"];
        assert_eq!(data["text"], "hello");
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let mut node = SanitizeNode::new();
        let result = node
            .init(&serde_json::json!({"operation": "bad"}))
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_node_type_and_kind() {
        let node = SanitizeNode::new();
        assert_eq!(node.node_type(), "sanitize");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
