use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// CsvNode — parse and generate CSV data
pub struct CsvNode {
    operation: String,
    headers: bool,
    delimiter: char,
}

impl Default for CsvNode {
    fn default() -> Self {
        Self::new()
    }
}

impl CsvNode {
    pub fn new() -> Self {
        Self {
            operation: "parse".to_string(),
            headers: true,
            delimiter: ',',
        }
    }
}

/// Parse a CSV line, handling basic double-quote escaping
fn parse_csv_line(line: &str, delimiter: char) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                // Check for escaped quote ""
                if chars.peek() == Some(&'"') {
                    current.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                current.push(ch);
            }
        } else if ch == '"' {
            in_quotes = true;
        } else if ch == delimiter {
            fields.push(current.clone());
            current.clear();
        } else {
            current.push(ch);
        }
    }
    fields.push(current);
    fields
}

/// Check if a field needs quoting in CSV output
fn needs_quoting(field: &str, delimiter: char) -> bool {
    field.contains(delimiter) || field.contains('"') || field.contains('\n') || field.contains('\r')
}

/// Quote a CSV field
fn quote_field(field: &str, delimiter: char) -> String {
    if needs_quoting(field, delimiter) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

fn extract_text(input: &serde_json::Value) -> Option<String> {
    if let Some(s) = input.as_str() {
        Some(s.to_string())
    } else if let Some(s) = input.get("text").and_then(|v| v.as_str()) {
        Some(s.to_string())
    } else {
        input
            .get("data")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }
}

#[async_trait::async_trait]
impl NodeExecutor for CsvNode {
    fn node_type(&self) -> &str {
        "csv"
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

        if let Some(h) = config.get("headers").and_then(|v| v.as_bool()) {
            self.headers = h;
        }

        if let Some(d) = config.get("delimiter").and_then(|v| v.as_str()) {
            self.delimiter = d.chars().next().unwrap_or(',');
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
                let text = extract_text(&input).ok_or_else(|| {
                    NodeError::ExecutionFailed("parse requires a string input".into())
                })?;

                let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
                if lines.is_empty() {
                    return Ok(NodeOutput::data(serde_json::Value::Array(vec![])));
                }

                if self.headers {
                    let header_fields = parse_csv_line(lines[0], self.delimiter);
                    let rows: Vec<serde_json::Value> = lines[1..]
                        .iter()
                        .map(|line| {
                            let values = parse_csv_line(line, self.delimiter);
                            let mut obj = serde_json::Map::new();
                            for (i, header) in header_fields.iter().enumerate() {
                                let val = values
                                    .get(i)
                                    .map(|s| s.trim().to_string())
                                    .unwrap_or_default();
                                obj.insert(
                                    header.trim().to_string(),
                                    serde_json::Value::String(val),
                                );
                            }
                            serde_json::Value::Object(obj)
                        })
                        .collect();
                    serde_json::Value::Array(rows)
                } else {
                    let rows: Vec<serde_json::Value> = lines
                        .iter()
                        .map(|line| {
                            let values = parse_csv_line(line, self.delimiter);
                            let arr: Vec<serde_json::Value> = values
                                .into_iter()
                                .map(|s| serde_json::Value::String(s.trim().to_string()))
                                .collect();
                            serde_json::Value::Array(arr)
                        })
                        .collect();
                    serde_json::Value::Array(rows)
                }
            }
            "stringify" => {
                let arr = input.as_array().ok_or_else(|| {
                    NodeError::ExecutionFailed("stringify requires an array input".into())
                })?;

                if arr.is_empty() {
                    return Ok(NodeOutput::data(serde_json::Value::String(String::new())));
                }

                // Check if array of objects or array of arrays
                if arr[0].is_object() {
                    // Collect all unique headers
                    let mut headers: Vec<String> = Vec::new();
                    for item in arr {
                        if let Some(obj) = item.as_object() {
                            for key in obj.keys() {
                                if !headers.contains(key) {
                                    headers.push(key.clone());
                                }
                            }
                        }
                    }

                    let mut lines = Vec::new();
                    if self.headers {
                        let header_line: Vec<String> = headers
                            .iter()
                            .map(|h| quote_field(h, self.delimiter))
                            .collect();
                        lines.push(header_line.join(&self.delimiter.to_string()));
                    }

                    for item in arr {
                        if let Some(obj) = item.as_object() {
                            let row: Vec<String> = headers
                                .iter()
                                .map(|h| {
                                    let val = obj
                                        .get(h)
                                        .map(|v| {
                                            if let Some(s) = v.as_str() {
                                                s.to_string()
                                            } else {
                                                v.to_string()
                                            }
                                        })
                                        .unwrap_or_default();
                                    quote_field(&val, self.delimiter)
                                })
                                .collect();
                            lines.push(row.join(&self.delimiter.to_string()));
                        }
                    }

                    serde_json::Value::String(lines.join("\n"))
                } else {
                    // Array of arrays
                    let lines: Vec<String> = arr
                        .iter()
                        .map(|row| {
                            if let Some(values) = row.as_array() {
                                values
                                    .iter()
                                    .map(|v| {
                                        let s = if let Some(s) = v.as_str() {
                                            s.to_string()
                                        } else {
                                            v.to_string()
                                        };
                                        quote_field(&s, self.delimiter)
                                    })
                                    .collect::<Vec<_>>()
                                    .join(&self.delimiter.to_string())
                            } else {
                                String::new()
                            }
                        })
                        .collect();
                    serde_json::Value::String(lines.join("\n"))
                }
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

    async fn make_node(config: serde_json::Value) -> CsvNode {
        let mut node = CsvNode::new();
        node.init(&config).await.unwrap();
        node
    }

    #[tokio::test]
    async fn test_parse_with_headers() {
        let node = make_node(json!({"operation": "parse"})).await;
        let ctx = ExecutionContext::new();
        let input = json!("name,age\nAlice,30\nBob,25");
        let output = node.execute(input, &ctx).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data[0]["name"], "Alice");
        assert_eq!(data[0]["age"], "30");
        assert_eq!(data[1]["name"], "Bob");
        assert_eq!(data[1]["age"], "25");
    }

    #[tokio::test]
    async fn test_parse_without_headers() {
        let node = make_node(json!({"operation": "parse", "headers": false})).await;
        let ctx = ExecutionContext::new();
        let input = json!("Alice,30\nBob,25");
        let output = node.execute(input, &ctx).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data[0], json!(["Alice", "30"]));
        assert_eq!(data[1], json!(["Bob", "25"]));
    }

    #[tokio::test]
    async fn test_parse_tab_delimiter() {
        let node = make_node(json!({"operation": "parse", "delimiter": "\t"})).await;
        let ctx = ExecutionContext::new();
        let input = json!("name\tage\nAlice\t30");
        let output = node.execute(input, &ctx).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data[0]["name"], "Alice");
        assert_eq!(data[0]["age"], "30");
    }

    #[tokio::test]
    async fn test_parse_semicolon_delimiter() {
        let node = make_node(json!({"operation": "parse", "delimiter": ";"})).await;
        let ctx = ExecutionContext::new();
        let input = json!("x;y\n1;2");
        let output = node.execute(input, &ctx).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data[0]["x"], "1");
        assert_eq!(data[0]["y"], "2");
    }

    #[tokio::test]
    async fn test_stringify_objects_with_headers() {
        let node = make_node(json!({"operation": "stringify"})).await;
        let ctx = ExecutionContext::new();
        let input = json!([{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}]);
        let output = node.execute(input, &ctx).await.unwrap();
        let csv_str = output.ports["default"].as_str().unwrap();
        let lines: Vec<&str> = csv_str.lines().collect();
        assert!(lines[0].contains("name"));
        assert!(lines[0].contains("age"));
        assert_eq!(lines.len(), 3);
    }

    #[tokio::test]
    async fn test_stringify_without_headers() {
        let node = make_node(json!({"operation": "stringify", "headers": false})).await;
        let ctx = ExecutionContext::new();
        let input = json!([{"name": "Alice"}]);
        let output = node.execute(input, &ctx).await.unwrap();
        let csv_str = output.ports["default"].as_str().unwrap();
        let lines: Vec<&str> = csv_str.lines().collect();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "Alice");
    }

    #[tokio::test]
    async fn test_stringify_arrays() {
        let node = make_node(json!({"operation": "stringify"})).await;
        let ctx = ExecutionContext::new();
        let input = json!([["a", "b"], ["c", "d"]]);
        let output = node.execute(input, &ctx).await.unwrap();
        let csv_str = output.ports["default"].as_str().unwrap();
        assert_eq!(csv_str, "a,b\nc,d");
    }

    #[tokio::test]
    async fn test_parse_empty_input() {
        let node = make_node(json!({"operation": "parse"})).await;
        let ctx = ExecutionContext::new();
        let input = json!("");
        let output = node.execute(input, &ctx).await.unwrap();
        assert_eq!(output.ports["default"], json!([]));
    }

    #[tokio::test]
    async fn test_parse_quoted_fields() {
        let node = make_node(json!({"operation": "parse"})).await;
        let ctx = ExecutionContext::new();
        let input = json!("name,desc\nAlice,\"hello, world\"");
        let output = node.execute(input, &ctx).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data[0]["desc"], "hello, world");
    }

    #[tokio::test]
    async fn test_parse_text_field_input() {
        let node = make_node(json!({"operation": "parse"})).await;
        let ctx = ExecutionContext::new();
        let input = json!({"text": "a,b\n1,2"});
        let output = node.execute(input, &ctx).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data[0]["a"], "1");
        assert_eq!(data[0]["b"], "2");
    }
}
