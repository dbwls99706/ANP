use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// XmlNode — simple XML parse/stringify
pub struct XmlNode {
    operation: String,
    root_tag: String,
    field: String,
}

impl Default for XmlNode {
    fn default() -> Self {
        Self::new()
    }
}

impl XmlNode {
    pub fn new() -> Self {
        Self {
            operation: "parse".to_string(),
            root_tag: "root".to_string(),
            field: "text".to_string(),
        }
    }
}

/// Parse simple XML into JSON. Handles flat and one-level-nested elements.
fn parse_xml(xml: &str) -> Result<serde_json::Value, String> {
    let xml = xml.trim();
    if xml.is_empty() {
        return Err("empty XML input".to_string());
    }

    let (tag, inner) = extract_tag_content(xml)?;
    let children = parse_children(&inner)?;

    let mut root_obj = serde_json::Map::new();
    root_obj.insert(tag, serde_json::Value::Object(children));
    Ok(serde_json::Value::Object(root_obj))
}

fn extract_tag_content(xml: &str) -> Result<(String, String), String> {
    let xml = xml.trim();
    if !xml.starts_with('<') {
        return Err("expected '<' at start".to_string());
    }

    let close_bracket = xml.find('>').ok_or("unclosed opening tag")?;
    let tag_name = &xml[1..close_bracket];
    if tag_name.is_empty() || tag_name.starts_with('/') {
        return Err("invalid tag name".to_string());
    }

    let closing_tag = format!("</{}>", tag_name);
    let close_pos = xml
        .rfind(&closing_tag)
        .ok_or_else(|| format!("missing closing tag </{}>", tag_name))?;

    let inner = &xml[close_bracket + 1..close_pos];
    Ok((tag_name.to_string(), inner.to_string()))
}

fn parse_children(inner: &str) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let mut obj = serde_json::Map::new();
    let inner = inner.trim();
    if inner.is_empty() {
        return Ok(obj);
    }

    // If no child tags, this is text content
    if !inner.contains('<') {
        obj.insert(
            "_text".to_string(),
            serde_json::Value::String(inner.to_string()),
        );
        return Ok(obj);
    }

    let mut pos = 0;
    let bytes = inner.as_bytes();
    while pos < inner.len() {
        // Skip whitespace
        while pos < inner.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= inner.len() {
            break;
        }
        if bytes[pos] != b'<' {
            return Err("unexpected content outside of tags".to_string());
        }

        let tag_start = pos;
        let close_bracket = inner[tag_start..].find('>').ok_or("unclosed tag")? + tag_start;
        let tag_name = &inner[tag_start + 1..close_bracket];
        if tag_name.is_empty() || tag_name.starts_with('/') {
            return Err("invalid child tag".to_string());
        }

        let closing_tag = format!("</{}>", tag_name);
        let close_pos = inner[close_bracket + 1..]
            .find(&closing_tag)
            .ok_or_else(|| format!("missing closing tag </{}>", tag_name))?
            + close_bracket
            + 1;

        let content = &inner[close_bracket + 1..close_pos];

        // Check if content has nested tags
        let value = if content.contains('<') {
            let nested = parse_children(content)?;
            serde_json::Value::Object(nested)
        } else {
            serde_json::Value::String(content.to_string())
        };

        obj.insert(tag_name.to_string(), value);
        pos = close_pos + closing_tag.len();
    }

    Ok(obj)
}

fn json_to_xml(value: &serde_json::Value, tag: &str) -> String {
    match value {
        serde_json::Value::Object(obj) => {
            let mut inner = String::new();
            for (key, val) in obj {
                inner.push_str(&json_to_xml(val, key));
            }
            format!("<{}>{}</{}>", tag, inner, tag)
        }
        serde_json::Value::Array(arr) => {
            let mut result = String::new();
            for item in arr {
                result.push_str(&json_to_xml(item, tag));
            }
            result
        }
        serde_json::Value::String(s) => format!("<{}>{}</{}>", tag, s, tag),
        serde_json::Value::Number(n) => format!("<{}>{}</{}>", tag, n, tag),
        serde_json::Value::Bool(b) => format!("<{}>{}</{}>", tag, b, tag),
        serde_json::Value::Null => format!("<{}/>", tag),
    }
}

fn extract_text_input(input: &serde_json::Value, field: &str) -> Option<String> {
    if let Some(s) = input.as_str() {
        Some(s.to_string())
    } else if let Some(s) = input.get(field).and_then(|v| v.as_str()) {
        Some(s.to_string())
    } else {
        input
            .get("data")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }
}

#[async_trait::async_trait]
impl NodeExecutor for XmlNode {
    fn node_type(&self) -> &str {
        "xml"
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
        if let Some(rt) = config.get("root_tag").and_then(|v| v.as_str()) {
            self.root_tag = rt.to_string();
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
                parse_xml(&text)
                    .map_err(|e| NodeError::ExecutionFailed(format!("XML parse error: {}", e)))?
            }
            "stringify" => {
                let xml = if let Some(obj) = input.as_object() {
                    // If input is an object with a single key, use that as root
                    if obj.len() == 1 {
                        let (key, val) = obj.iter().next().unwrap();
                        json_to_xml(val, key)
                    } else {
                        json_to_xml(&input, &self.root_tag)
                    }
                } else {
                    json_to_xml(&input, &self.root_tag)
                };
                serde_json::json!({ "xml": xml })
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
    async fn test_parse_simple_xml() {
        let mut node = XmlNode::new();
        node.init(&json!({"operation": "parse"})).await.unwrap();
        let input = json!({"text": "<root><name>John</name><age>30</age></root>"});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["root"]["name"], "John");
        assert_eq!(data["root"]["age"], "30");
    }

    #[tokio::test]
    async fn test_parse_nested_xml() {
        let mut node = XmlNode::new();
        node.init(&json!({"operation": "parse"})).await.unwrap();
        let input = json!({"text": "<person><address><city>NYC</city></address></person>"});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["person"]["address"]["city"], "NYC");
    }

    #[tokio::test]
    async fn test_stringify_json_to_xml() {
        let mut node = XmlNode::new();
        node.init(&json!({"operation": "stringify"})).await.unwrap();
        let input = json!({"person": {"name": "John", "age": 30}});
        let output = node.execute(input, &ctx()).await.unwrap();
        let xml = output.ports["default"]["xml"].as_str().unwrap();
        assert!(xml.contains("<person>"));
        assert!(xml.contains("<name>John</name>"));
        assert!(xml.contains("</person>"));
    }

    #[tokio::test]
    async fn test_parse_empty_xml_error() {
        let mut node = XmlNode::new();
        node.init(&json!({"operation": "parse"})).await.unwrap();
        let input = json!({"text": ""});
        let result = node.execute(input, &ctx()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let mut node = XmlNode::new();
        let result = node.init(&json!({"operation": "invalid"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_stringify_with_root_tag() {
        let mut node = XmlNode::new();
        node.init(&json!({"operation": "stringify", "root_tag": "data"}))
            .await
            .unwrap();
        let input = json!({"a": "1", "b": "2"});
        let output = node.execute(input, &ctx()).await.unwrap();
        let xml = output.ports["default"]["xml"].as_str().unwrap();
        assert!(xml.contains("<data>"));
        assert!(xml.contains("</data>"));
    }

    #[tokio::test]
    async fn test_parse_string_input_directly() {
        let mut node = XmlNode::new();
        node.init(&json!({"operation": "parse"})).await.unwrap();
        let input = serde_json::Value::String("<root><x>1</x></root>".to_string());
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["root"]["x"], "1");
    }
}
