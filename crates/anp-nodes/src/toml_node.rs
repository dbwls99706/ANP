use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// TomlNode — TOML parse/stringify (basic subset)
pub struct TomlNode {
    operation: String,
    field: String,
}

impl Default for TomlNode {
    fn default() -> Self {
        Self::new()
    }
}

impl TomlNode {
    pub fn new() -> Self {
        Self {
            operation: "parse".to_string(),
            field: "text".to_string(),
        }
    }
}

fn parse_toml(input: &str) -> Result<serde_json::Value, String> {
    let mut root = serde_json::Map::new();
    let mut current_section: Option<String> = None;

    for line in input.lines() {
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Section header: [section]
        if trimmed.starts_with('[') && trimmed.ends_with(']') && !trimmed.starts_with("[[") {
            let section_name = trimmed[1..trimmed.len() - 1].trim().to_string();
            if section_name.is_empty() {
                return Err("empty section name".to_string());
            }
            // Ensure section object exists
            if !root.contains_key(&section_name) {
                root.insert(section_name.clone(), serde_json::json!({}));
            }
            current_section = Some(section_name);
            continue;
        }

        // Key = Value
        if let Some(eq_pos) = trimmed.find('=') {
            let key = trimmed[..eq_pos].trim().to_string();
            let val_str = trimmed[eq_pos + 1..].trim();
            let value = parse_toml_value(val_str)?;

            match &current_section {
                Some(section) => {
                    if let Some(obj) = root.get_mut(section).and_then(|v| v.as_object_mut()) {
                        obj.insert(key, value);
                    }
                }
                None => {
                    root.insert(key, value);
                }
            }
        } else {
            return Err(format!("invalid line: {}", trimmed));
        }
    }

    Ok(serde_json::Value::Object(root))
}

fn parse_toml_value(s: &str) -> Result<serde_json::Value, String> {
    let s = s.trim();

    if s.is_empty() {
        return Ok(serde_json::Value::String(String::new()));
    }

    // Boolean
    if s == "true" {
        return Ok(serde_json::Value::Bool(true));
    }
    if s == "false" {
        return Ok(serde_json::Value::Bool(false));
    }

    // Quoted string (double quotes)
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        let inner = &s[1..s.len() - 1];
        let unescaped = inner
            .replace("\\n", "\n")
            .replace("\\t", "\t")
            .replace("\\\\", "\\")
            .replace("\\\"", "\"");
        return Ok(serde_json::Value::String(unescaped));
    }

    // Quoted string (single quotes — literal, no escape processing)
    if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 {
        let inner = &s[1..s.len() - 1];
        return Ok(serde_json::Value::String(inner.to_string()));
    }

    // Array: [val1, val2, ...]
    if s.starts_with('[') && s.ends_with(']') {
        let inner = s[1..s.len() - 1].trim();
        if inner.is_empty() {
            return Ok(serde_json::Value::Array(Vec::new()));
        }
        let items = split_toml_array(inner);
        let mut arr = Vec::new();
        for item in items {
            arr.push(parse_toml_value(item.trim())?);
        }
        return Ok(serde_json::Value::Array(arr));
    }

    // Strip inline comment (only if not in a string)
    let value_str = if let Some(hash_pos) = s.find(" #") {
        s[..hash_pos].trim()
    } else {
        s
    };

    // Integer
    if let Ok(n) = value_str.parse::<i64>() {
        return Ok(serde_json::json!(n));
    }

    // Float
    if let Ok(n) = value_str.parse::<f64>() {
        if n.is_finite() {
            return Ok(serde_json::json!(n));
        }
    }

    // Bare string (unquoted) — treat as string
    Ok(serde_json::Value::String(value_str.to_string()))
}

fn split_toml_array(s: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut depth = 0;
    let mut in_quotes = false;
    let mut quote_char = ' ';
    let mut start = 0;

    for (i, ch) in s.char_indices() {
        if in_quotes {
            if ch == quote_char {
                in_quotes = false;
            }
            continue;
        }

        match ch {
            '"' | '\'' => {
                in_quotes = true;
                quote_char = ch;
            }
            '[' => depth += 1,
            ']' => depth -= 1,
            ',' if depth == 0 => {
                items.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }

    if start < s.len() {
        items.push(&s[start..]);
    }

    items
}

fn json_to_toml(value: &serde_json::Value) -> Result<String, String> {
    let obj = value
        .as_object()
        .ok_or("TOML stringify requires an object")?;

    let mut lines = Vec::new();

    // First pass: top-level scalar values
    for (key, val) in obj {
        if !val.is_object() {
            lines.push(format!("{} = {}", key, toml_value_str(val)));
        }
    }

    // Second pass: sections (nested objects)
    for (key, val) in obj {
        if let Some(section_obj) = val.as_object() {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push(format!("[{}]", key));
            for (k, v) in section_obj {
                lines.push(format!("{} = {}", k, toml_value_str(v)));
            }
        }
    }

    Ok(lines.join("\n"))
}

fn toml_value_str(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => {
            format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
        }
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "\"\"".to_string(),
        serde_json::Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(toml_value_str).collect();
            format!("[{}]", items.join(", "))
        }
        serde_json::Value::Object(_) => "{}".to_string(),
    }
}

fn extract_text_input(input: &serde_json::Value, field: &str) -> Option<String> {
    if let Some(s) = input.as_str() {
        Some(s.to_string())
    } else if let Some(s) = input.get(field).and_then(|v| v.as_str()) {
        Some(s.to_string())
    } else if let Some(s) = input.get("data").and_then(|v| v.as_str()) {
        Some(s.to_string())
    } else {
        None
    }
}

#[async_trait::async_trait]
impl NodeExecutor for TomlNode {
    fn node_type(&self) -> &str {
        "toml"
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
        let result = match self.operation.as_str() {
            "parse" => {
                let text = extract_text_input(&input, &self.field).ok_or_else(|| {
                    NodeError::ExecutionFailed("parse requires a string input".into())
                })?;
                parse_toml(&text).map_err(|e| {
                    NodeError::ExecutionFailed(format!("TOML parse error: {}", e))
                })?
            }
            "stringify" => {
                let toml_str = json_to_toml(&input).map_err(|e| {
                    NodeError::ExecutionFailed(format!("TOML stringify error: {}", e))
                })?;
                serde_json::json!({ "toml": toml_str })
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
    use serde_json::json;

    fn ctx() -> ExecutionContext {
        ExecutionContext::new()
    }

    #[tokio::test]
    async fn test_parse_basic_toml() {
        let mut node = TomlNode::new();
        node.init(&json!({"operation": "parse"})).await.unwrap();
        let input = json!({"text": "name = \"Alice\"\nage = 30\nactive = true"});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["name"], "Alice");
        assert_eq!(data["age"], 30);
        assert_eq!(data["active"], true);
    }

    #[tokio::test]
    async fn test_parse_with_sections() {
        let mut node = TomlNode::new();
        node.init(&json!({"operation": "parse"})).await.unwrap();
        let toml_str = "title = \"Config\"\n\n[database]\nhost = \"localhost\"\nport = 5432";
        let input = json!({"text": toml_str});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["title"], "Config");
        assert_eq!(data["database"]["host"], "localhost");
        assert_eq!(data["database"]["port"], 5432);
    }

    #[tokio::test]
    async fn test_parse_with_comments() {
        let mut node = TomlNode::new();
        node.init(&json!({"operation": "parse"})).await.unwrap();
        let input = json!({"text": "# This is a comment\nkey = \"value\"\n# Another comment"});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["key"], "value");
    }

    #[tokio::test]
    async fn test_stringify_basic() {
        let mut node = TomlNode::new();
        node.init(&json!({"operation": "stringify"}))
            .await
            .unwrap();
        let input = json!({"name": "Alice", "age": 30});
        let output = node.execute(input, &ctx()).await.unwrap();
        let toml = output.ports["default"]["toml"].as_str().unwrap();
        assert!(toml.contains("name = \"Alice\""));
        assert!(toml.contains("age = 30"));
    }

    #[tokio::test]
    async fn test_stringify_with_sections() {
        let mut node = TomlNode::new();
        node.init(&json!({"operation": "stringify"}))
            .await
            .unwrap();
        let input = json!({
            "title": "My App",
            "server": {"host": "localhost", "port": 8080}
        });
        let output = node.execute(input, &ctx()).await.unwrap();
        let toml = output.ports["default"]["toml"].as_str().unwrap();
        assert!(toml.contains("title = \"My App\""));
        assert!(toml.contains("[server]"));
        assert!(toml.contains("host = \"localhost\""));
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let mut node = TomlNode::new();
        let result = node.init(&json!({"operation": "invalid"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_parse_array_value() {
        let mut node = TomlNode::new();
        node.init(&json!({"operation": "parse"})).await.unwrap();
        let input = json!({"text": "ports = [80, 443, 8080]"});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        let ports = data["ports"].as_array().unwrap();
        assert_eq!(ports.len(), 3);
        assert_eq!(ports[0], 80);
        assert_eq!(ports[1], 443);
    }

    #[tokio::test]
    async fn test_parse_float_value() {
        let mut node = TomlNode::new();
        node.init(&json!({"operation": "parse"})).await.unwrap();
        let input = json!({"text": "pi = 3.14"});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["pi"], 3.14);
    }
}
