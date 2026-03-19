use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// YamlNode — parse and generate simple YAML
pub struct YamlNode {
    operation: String,
}

impl Default for YamlNode {
    fn default() -> Self {
        Self::new()
    }
}

impl YamlNode {
    pub fn new() -> Self {
        Self {
            operation: "parse".to_string(),
        }
    }
}

fn parse_yaml(input: &str) -> Result<serde_json::Value, String> {
    let lines: Vec<&str> = input.lines().collect();
    if lines.is_empty() {
        return Ok(serde_json::Value::Null);
    }
    let (value, _) = parse_yaml_block(&lines, 0, 0)?;
    Ok(value)
}

fn parse_yaml_block(
    lines: &[&str],
    start: usize,
    _expected_indent: usize,
) -> Result<(serde_json::Value, usize), String> {
    let mut idx = start;
    while idx < lines.len() {
        let trimmed = lines[idx].trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            idx += 1;
        } else {
            break;
        }
    }
    if idx >= lines.len() {
        return Ok((serde_json::Value::Null, idx));
    }

    let first_indent = count_indent(lines[idx]);
    let trimmed_first = lines[idx].trim_start();

    if trimmed_first.starts_with("- ") || trimmed_first == "-" {
        return parse_yaml_array(lines, idx, first_indent);
    }

    parse_yaml_object(lines, idx, first_indent)
}

fn parse_yaml_array(
    lines: &[&str],
    start: usize,
    base_indent: usize,
) -> Result<(serde_json::Value, usize), String> {
    let mut arr = Vec::new();
    let mut idx = start;

    while idx < lines.len() {
        let line = lines[idx];
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            idx += 1;
            continue;
        }
        let indent = count_indent(line);
        if indent < base_indent || indent != base_indent {
            break;
        }

        let content = line.trim_start();
        if !content.starts_with("- ") && content != "-" {
            break;
        }

        let item_text = if content == "-" { "" } else { &content[2..] };

        if let Some(colon_pos) = find_colon_pos(item_text) {
            let key = item_text[..colon_pos].trim();
            let val_str = item_text[colon_pos + 1..].trim();
            let mut obj = serde_json::Map::new();

            if val_str.is_empty() {
                idx += 1;
                if idx < lines.len() {
                    let next_indent = count_indent(lines[idx]);
                    if next_indent > base_indent {
                        let (sub_val, next_idx) = parse_yaml_block(lines, idx, next_indent)?;
                        obj.insert(key.to_string(), sub_val);
                        idx = next_idx;
                    } else {
                        obj.insert(key.to_string(), serde_json::Value::Null);
                    }
                } else {
                    obj.insert(key.to_string(), serde_json::Value::Null);
                }
            } else {
                obj.insert(key.to_string(), parse_yaml_scalar(val_str));
                idx += 1;
            }

            let item_indent = base_indent + 2;
            while idx < lines.len() {
                let next_line = lines[idx];
                let next_trimmed = next_line.trim();
                if next_trimmed.is_empty() || next_trimmed.starts_with('#') {
                    idx += 1;
                    continue;
                }
                let next_indent = count_indent(next_line);
                if next_indent != item_indent {
                    break;
                }
                if let Some(cp) = find_colon_pos(next_trimmed) {
                    let k = next_trimmed[..cp].trim();
                    let v = next_trimmed[cp + 1..].trim();
                    if v.is_empty() {
                        idx += 1;
                        if idx < lines.len() {
                            let sub_indent = count_indent(lines[idx]);
                            if sub_indent > next_indent {
                                let (sub_val, next_idx) = parse_yaml_block(lines, idx, sub_indent)?;
                                obj.insert(k.to_string(), sub_val);
                                idx = next_idx;
                            } else {
                                obj.insert(k.to_string(), serde_json::Value::Null);
                            }
                        } else {
                            obj.insert(k.to_string(), serde_json::Value::Null);
                        }
                    } else {
                        obj.insert(k.to_string(), parse_yaml_scalar(v));
                        idx += 1;
                    }
                } else {
                    break;
                }
            }

            arr.push(serde_json::Value::Object(obj));
        } else {
            arr.push(parse_yaml_scalar(item_text));
            idx += 1;
        }
    }

    Ok((serde_json::Value::Array(arr), idx))
}

fn parse_yaml_object(
    lines: &[&str],
    start: usize,
    base_indent: usize,
) -> Result<(serde_json::Value, usize), String> {
    let mut obj = serde_json::Map::new();
    let mut idx = start;

    while idx < lines.len() {
        let line = lines[idx];
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            idx += 1;
            continue;
        }
        let indent = count_indent(line);
        if indent != base_indent {
            break;
        }

        if let Some(colon_pos) = find_colon_pos(trimmed) {
            let key = trimmed[..colon_pos].trim().to_string();
            let val_str = trimmed[colon_pos + 1..].trim();

            if val_str.is_empty() {
                idx += 1;
                if idx < lines.len() {
                    let next_trimmed = lines[idx].trim();
                    if !next_trimmed.is_empty() && !next_trimmed.starts_with('#') {
                        let next_indent = count_indent(lines[idx]);
                        if next_indent > base_indent {
                            let (sub_val, next_idx) = parse_yaml_block(lines, idx, next_indent)?;
                            obj.insert(key, sub_val);
                            idx = next_idx;
                        } else {
                            obj.insert(key, serde_json::Value::Null);
                        }
                    } else {
                        obj.insert(key, serde_json::Value::Null);
                    }
                } else {
                    obj.insert(key, serde_json::Value::Null);
                }
            } else {
                obj.insert(key, parse_yaml_scalar(val_str));
                idx += 1;
            }
        } else {
            break;
        }
    }

    Ok((serde_json::Value::Object(obj), idx))
}

fn parse_yaml_scalar(s: &str) -> serde_json::Value {
    let s = s.trim();
    if s.is_empty() || s == "null" || s == "~" {
        return serde_json::Value::Null;
    }
    if s == "true" {
        return serde_json::Value::Bool(true);
    }
    if s == "false" {
        return serde_json::Value::Bool(false);
    }
    if ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
        && s.len() >= 2
    {
        return serde_json::Value::String(s[1..s.len() - 1].to_string());
    }
    if let Ok(n) = s.parse::<i64>() {
        return serde_json::json!(n);
    }
    if let Ok(n) = s.parse::<f64>() {
        if n.is_finite() {
            return serde_json::json!(n);
        }
    }
    serde_json::Value::String(s.to_string())
}

fn count_indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn find_colon_pos(s: &str) -> Option<usize> {
    let mut in_quotes = false;
    let mut quote_char = ' ';
    for (i, ch) in s.char_indices() {
        if in_quotes {
            if ch == quote_char {
                in_quotes = false;
            }
        } else if ch == '"' || ch == '\'' {
            in_quotes = true;
            quote_char = ch;
        } else if ch == ':' {
            let rest = &s[i + 1..];
            if rest.is_empty() || rest.starts_with(' ') {
                return Some(i);
            }
        }
    }
    None
}

fn json_to_yaml(value: &serde_json::Value, indent: usize) -> String {
    let prefix = "  ".repeat(indent);
    match value {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(b) => format!("{}", b),
        serde_json::Value::Number(n) => format!("{}", n),
        serde_json::Value::String(s) => {
            if s.contains(':')
                || s.contains('#')
                || s.contains('\n')
                || s.starts_with(' ')
                || s.ends_with(' ')
                || s == "true"
                || s == "false"
                || s == "null"
                || s.is_empty()
            {
                format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
            } else {
                s.clone()
            }
        }
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                return "[]".to_string();
            }
            let mut lines = Vec::new();
            for item in arr {
                match item {
                    serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                        let sub = json_to_yaml(item, indent + 1);
                        let sub_lines: Vec<&str> = sub.lines().collect();
                        if !sub_lines.is_empty() {
                            lines.push(format!("{}- {}", prefix, sub_lines[0].trim_start()));
                            for sub_line in &sub_lines[1..] {
                                lines.push(format!("{}  {}", prefix, sub_line.trim_start()));
                            }
                        }
                    }
                    _ => {
                        lines.push(format!("{}- {}", prefix, json_to_yaml(item, 0)));
                    }
                }
            }
            lines.join("\n")
        }
        serde_json::Value::Object(obj) => {
            if obj.is_empty() {
                return "{}".to_string();
            }
            let mut lines = Vec::new();
            for (key, val) in obj {
                let key_str = if key.contains(' ') || key.contains(':') {
                    format!("\"{}\"", key)
                } else {
                    key.clone()
                };
                match val {
                    serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                        lines.push(format!("{}{}:", prefix, key_str));
                        let sub = json_to_yaml(val, indent + 1);
                        lines.push(sub);
                    }
                    _ => {
                        lines.push(format!("{}{}: {}", prefix, key_str, json_to_yaml(val, 0)));
                    }
                }
            }
            lines.join("\n")
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for YamlNode {
    fn node_type(&self) -> &str {
        "yaml"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "parse" | "stringify" => self.operation = op.to_string(),
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown operation '{}'. Use: parse, stringify",
                        other
                    )));
                }
            }
        }
        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let result = match self.operation.as_str() {
            "parse" => {
                let text = input.as_str().ok_or_else(|| {
                    NodeError::ExecutionFailed("parse requires a string input".into())
                })?;
                parse_yaml(text)
                    .map_err(|e| NodeError::ExecutionFailed(format!("YAML parse error: {}", e)))?
            }
            "stringify" => {
                let yaml_str = json_to_yaml(&input, 0);
                serde_json::Value::String(yaml_str)
            }
            _ => {
                return Err(NodeError::ExecutionFailed(format!(
                    "unknown operation: {}",
                    self.operation
                )));
            }
        };

        Ok(NodeOutput::data(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anp_core::node::traits::NodeExecutor;
    use anp_core::runtime::context::ExecutionContext;
    use serde_json::json;

    async fn make_node(operation: &str) -> YamlNode {
        let mut node = YamlNode::new();
        node.init(&json!({ "operation": operation })).await.unwrap();
        node
    }

    #[tokio::test]
    async fn test_parse_simple_object() {
        let node = make_node("parse").await;
        let ctx = ExecutionContext::new();
        let input = json!("name: Alice\nage: 30");
        let output = node.execute(input, &ctx).await.unwrap();
        assert_eq!(output.ports["default"], json!({"name": "Alice", "age": 30}));
    }

    #[tokio::test]
    async fn test_parse_nested_object() {
        let node = make_node("parse").await;
        let ctx = ExecutionContext::new();
        let input = json!("person:\n  name: Bob\n  age: 25");
        let output = node.execute(input, &ctx).await.unwrap();
        assert_eq!(
            output.ports["default"],
            json!({"person": {"name": "Bob", "age": 25}})
        );
    }

    #[tokio::test]
    async fn test_parse_array() {
        let node = make_node("parse").await;
        let ctx = ExecutionContext::new();
        let input = json!("- hello\n- world\n- 42");
        let output = node.execute(input, &ctx).await.unwrap();
        assert_eq!(output.ports["default"], json!(["hello", "world", 42]));
    }

    #[tokio::test]
    async fn test_parse_boolean_and_null() {
        let node = make_node("parse").await;
        let ctx = ExecutionContext::new();
        let input = json!("active: true\ndeleted: false\nvalue: null");
        let output = node.execute(input, &ctx).await.unwrap();
        assert_eq!(
            output.ports["default"],
            json!({"active": true, "deleted": false, "value": null})
        );
    }

    #[tokio::test]
    async fn test_parse_empty_input() {
        let node = make_node("parse").await;
        let ctx = ExecutionContext::new();
        let input = json!("");
        let output = node.execute(input, &ctx).await.unwrap();
        assert_eq!(output.ports["default"], json!(null));
    }

    #[tokio::test]
    async fn test_stringify_simple_object() {
        let node = make_node("stringify").await;
        let ctx = ExecutionContext::new();
        let input = json!({"name": "Alice", "age": 30});
        let output = node.execute(input, &ctx).await.unwrap();
        let yaml_str = output.ports["default"].as_str().unwrap();
        assert!(yaml_str.contains("name: Alice"));
        assert!(yaml_str.contains("age: 30"));
    }

    #[tokio::test]
    async fn test_stringify_array() {
        let node = make_node("stringify").await;
        let ctx = ExecutionContext::new();
        let input = json!([1, 2, 3]);
        let output = node.execute(input, &ctx).await.unwrap();
        let yaml_str = output.ports["default"].as_str().unwrap();
        assert!(yaml_str.contains("- 1"));
        assert!(yaml_str.contains("- 2"));
        assert!(yaml_str.contains("- 3"));
    }

    #[tokio::test]
    async fn test_parse_non_string_input_fails() {
        let node = make_node("parse").await;
        let ctx = ExecutionContext::new();
        let input = json!(42);
        let result = node.execute(input, &ctx).await;
        assert!(result.is_err());
    }
}
