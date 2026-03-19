use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// StringNode — string manipulation operations
pub struct StringNode {
    operation: String,
    delimiter: String,
    from: String,
    to: String,
    start: i64,
    end: Option<i64>,
    length: usize,
    pad_char: String,
    count: usize,
    search: String,
}

impl Default for StringNode {
    fn default() -> Self {
        Self::new()
    }
}

impl StringNode {
    pub fn new() -> Self {
        Self {
            operation: "trim".to_string(),
            delimiter: ",".to_string(),
            from: String::new(),
            to: String::new(),
            start: 0,
            end: None,
            length: 0,
            pad_char: " ".to_string(),
            count: 1,
            search: String::new(),
        }
    }
}

fn extract_text(input: &serde_json::Value) -> Option<String> {
    if let Some(s) = input.as_str() {
        Some(s.to_string())
    } else if let Some(s) = input.get("text").and_then(|v| v.as_str()) {
        Some(s.to_string())
    } else {
        None
    }
}

#[async_trait::async_trait]
impl NodeExecutor for StringNode {
    fn node_type(&self) -> &str {
        "string"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        self.operation = config
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| NodeError::InvalidConfig("missing 'operation' in config".into()))?
            .to_string();

        let valid_ops = [
            "split", "join", "trim", "replace", "uppercase", "lowercase", "slice",
            "pad_start", "pad_end", "repeat", "contains", "starts_with", "ends_with",
            "length", "reverse", "substring",
        ];
        if !valid_ops.contains(&self.operation.as_str()) {
            return Err(NodeError::InvalidConfig(format!(
                "unknown operation '{}'. Valid: {}",
                self.operation,
                valid_ops.join(", ")
            )));
        }

        if let Some(d) = config.get("delimiter").and_then(|v| v.as_str()) {
            self.delimiter = d.to_string();
        }
        if let Some(f) = config.get("from").and_then(|v| v.as_str()) {
            self.from = f.to_string();
        }
        if let Some(t) = config.get("to").and_then(|v| v.as_str()) {
            self.to = t.to_string();
        }
        if let Some(s) = config.get("start").and_then(|v| v.as_i64()) {
            self.start = s;
        }
        if let Some(e) = config.get("end").and_then(|v| v.as_i64()) {
            self.end = Some(e);
        }
        if let Some(l) = config.get("length").and_then(|v| v.as_u64()) {
            self.length = l as usize;
        }
        if let Some(c) = config.get("char").and_then(|v| v.as_str()) {
            self.pad_char = c.to_string();
        }
        if let Some(c) = config.get("count").and_then(|v| v.as_u64()) {
            self.count = c as usize;
        }
        if let Some(s) = config.get("search").and_then(|v| v.as_str()) {
            self.search = s.to_string();
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        if self.operation == "join" {
            let arr = input.as_array().ok_or_else(|| {
                NodeError::ExecutionFailed("join operation requires an array input".into())
            })?;
            let strings: Vec<String> = arr
                .iter()
                .map(|v| {
                    if let Some(s) = v.as_str() {
                        s.to_string()
                    } else {
                        v.to_string()
                    }
                })
                .collect();
            let joined = strings.join(&self.delimiter);
            return Ok(NodeOutput::data(serde_json::Value::String(joined)));
        }

        let text = extract_text(&input).ok_or_else(|| {
            NodeError::ExecutionFailed("input must be a string or { \"text\": \"...\" }".into())
        })?;

        let result = match self.operation.as_str() {
            "split" => {
                let parts: Vec<serde_json::Value> = text
                    .split(&self.delimiter)
                    .map(|s| serde_json::Value::String(s.to_string()))
                    .collect();
                serde_json::Value::Array(parts)
            }
            "trim" => serde_json::Value::String(text.trim().to_string()),
            "replace" => serde_json::Value::String(text.replace(&self.from, &self.to)),
            "uppercase" => serde_json::Value::String(text.to_uppercase()),
            "lowercase" => serde_json::Value::String(text.to_lowercase()),
            "slice" => {
                let chars: Vec<char> = text.chars().collect();
                let len = chars.len() as i64;
                let start = if self.start < 0 {
                    (len + self.start).max(0) as usize
                } else {
                    self.start as usize
                };
                let end = match self.end {
                    Some(e) => {
                        if e < 0 {
                            (len + e).max(0) as usize
                        } else {
                            (e as usize).min(chars.len())
                        }
                    }
                    None => chars.len(),
                };
                let sliced: String = if start < end && start < chars.len() {
                    chars[start..end.min(chars.len())].iter().collect()
                } else {
                    String::new()
                };
                serde_json::Value::String(sliced)
            }
            "pad_start" => {
                let pad_char = self.pad_char.chars().next().unwrap_or(' ');
                let current_len = text.chars().count();
                if current_len >= self.length {
                    serde_json::Value::String(text)
                } else {
                    let padding: String =
                        std::iter::repeat(pad_char).take(self.length - current_len).collect();
                    serde_json::Value::String(format!("{}{}", padding, text))
                }
            }
            "pad_end" => {
                let pad_char = self.pad_char.chars().next().unwrap_or(' ');
                let current_len = text.chars().count();
                if current_len >= self.length {
                    serde_json::Value::String(text)
                } else {
                    let padding: String =
                        std::iter::repeat(pad_char).take(self.length - current_len).collect();
                    serde_json::Value::String(format!("{}{}", text, padding))
                }
            }
            "repeat" => serde_json::Value::String(text.repeat(self.count)),
            "contains" => serde_json::json!(text.contains(&self.search)),
            "starts_with" => serde_json::json!(text.starts_with(&self.search)),
            "ends_with" => serde_json::json!(text.ends_with(&self.search)),
            "length" => serde_json::json!(text.chars().count()),
            "reverse" => serde_json::Value::String(text.chars().rev().collect()),
            "substring" => {
                let chars: Vec<char> = text.chars().collect();
                let start = self.start.max(0) as usize;
                let len = if self.length > 0 {
                    self.length
                } else {
                    chars.len().saturating_sub(start)
                };
                let sub: String = chars.iter().skip(start).take(len).collect();
                serde_json::Value::String(sub)
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
    use anp_core::runtime::context::ExecutionContext;
    use serde_json::json;

    async fn make_node(config: serde_json::Value) -> StringNode {
        let mut node = StringNode::new();
        node.init(&config).await.unwrap();
        node
    }

    #[tokio::test]
    async fn test_split() {
        let node = make_node(json!({"operation": "split", "delimiter": ","})).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!("a,b,c"), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], json!(["a", "b", "c"]));
    }

    #[tokio::test]
    async fn test_join() {
        let node = make_node(json!({"operation": "join", "delimiter": "-"})).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!(["x", "y", "z"]), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], "x-y-z");
    }

    #[tokio::test]
    async fn test_trim() {
        let node = make_node(json!({"operation": "trim"})).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!("  hello  "), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], "hello");
    }

    #[tokio::test]
    async fn test_replace() {
        let node = make_node(json!({"operation": "replace", "from": "world", "to": "rust"})).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!("hello world"), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], "hello rust");
    }

    #[tokio::test]
    async fn test_uppercase() {
        let node = make_node(json!({"operation": "uppercase"})).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!("hello"), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], "HELLO");
    }

    #[tokio::test]
    async fn test_lowercase() {
        let node = make_node(json!({"operation": "lowercase"})).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!("HELLO"), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], "hello");
    }

    #[tokio::test]
    async fn test_slice() {
        let node = make_node(json!({"operation": "slice", "start": 1, "end": 4})).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!("abcdef"), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], "bcd");
    }

    #[tokio::test]
    async fn test_slice_negative() {
        let node = make_node(json!({"operation": "slice", "start": -3})).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!("abcdef"), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], "def");
    }

    #[tokio::test]
    async fn test_contains_true() {
        let node = make_node(json!({"operation": "contains", "search": "ell"})).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!("hello"), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], true);
    }

    #[tokio::test]
    async fn test_contains_false() {
        let node = make_node(json!({"operation": "contains", "search": "xyz"})).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!("hello"), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], false);
    }

    #[tokio::test]
    async fn test_reverse() {
        let node = make_node(json!({"operation": "reverse"})).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!("abcd"), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], "dcba");
    }

    #[tokio::test]
    async fn test_length() {
        let node = make_node(json!({"operation": "length"})).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!("hello"), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], 5);
    }

    #[tokio::test]
    async fn test_substring() {
        let node = make_node(json!({"operation": "substring", "start": 2, "length": 3})).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!("abcdefgh"), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], "cde");
    }
}
