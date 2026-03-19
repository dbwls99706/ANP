use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// JSON Parse node — parse, stringify, query, and manipulate JSON
pub struct JsonParseNode {
    operation: String,
    path: String,
    pretty: bool,
    set_value: serde_json::Value,
}

impl Default for JsonParseNode {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonParseNode {
    pub fn new() -> Self {
        Self {
            operation: "parse".to_string(),
            path: String::new(),
            pretty: false,
            set_value: serde_json::Value::Null,
        }
    }
}

fn resolve_dot_path(value: &serde_json::Value, path: &str) -> Option<serde_json::Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = value;
    for part in parts {
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
    Some(current.clone())
}

fn set_dot_path(value: &mut serde_json::Value, path: &str, new_val: serde_json::Value) {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = value;
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            if let serde_json::Value::Object(map) = current {
                map.insert(part.to_string(), new_val);
                return;
            }
        } else if let serde_json::Value::Object(map) = current {
            current = map
                .entry(part.to_string())
                .or_insert_with(|| serde_json::json!({}));
        } else {
            return;
        }
    }
}

fn delete_dot_path(value: &mut serde_json::Value, path: &str) {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = value;
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            if let serde_json::Value::Object(map) = current {
                map.remove(*part);
            }
        } else if let serde_json::Value::Object(map) = current {
            if let Some(next) = map.get_mut(*part) {
                current = next;
            } else {
                return;
            }
        } else {
            return;
        }
    }
}

fn flatten_json(
    value: &serde_json::Value,
    prefix: &str,
    result: &mut serde_json::Map<String, serde_json::Value>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{}.{}", prefix, k)
                };
                flatten_json(v, &key, result);
            }
        }
        other => {
            result.insert(prefix.to_string(), other.clone());
        }
    }
}

fn unflatten_json(flat: &serde_json::Map<String, serde_json::Value>) -> serde_json::Value {
    let mut result = serde_json::json!({});
    for (key, val) in flat {
        set_dot_path(&mut result, key, val.clone());
    }
    result
}

#[async_trait::async_trait]
impl NodeExecutor for JsonParseNode {
    fn node_type(&self) -> &str {
        "json_parse"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            self.operation = op.to_string();
        }
        if let Some(p) = config.get("path").and_then(|v| v.as_str()) {
            self.path = p.to_string();
        }
        if let Some(p) = config.get("pretty").and_then(|v| v.as_bool()) {
            self.pretty = p;
        }
        if let Some(v) = config.get("value") {
            self.set_value = v.clone();
        }
        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        match self.operation.as_str() {
            "parse" => {
                let s = input.as_str().unwrap_or("");
                let parsed: serde_json::Value = serde_json::from_str(s)
                    .map_err(|e| NodeError::ExecutionFailed(format!("JSON parse error: {}", e)))?;
                Ok(NodeOutput::data(parsed))
            }
            "stringify" => {
                let s = if self.pretty {
                    serde_json::to_string_pretty(&input)
                } else {
                    serde_json::to_string(&input)
                }
                .map_err(|e| NodeError::ExecutionFailed(e.to_string()))?;
                Ok(NodeOutput::data(serde_json::json!(s)))
            }
            "query" => {
                let result =
                    resolve_dot_path(&input, &self.path).unwrap_or(serde_json::Value::Null);
                Ok(NodeOutput::data(result))
            }
            "set" => {
                let mut data = input.clone();
                set_dot_path(&mut data, &self.path, self.set_value.clone());
                Ok(NodeOutput::data(data))
            }
            "delete" => {
                let mut data = input.clone();
                delete_dot_path(&mut data, &self.path);
                Ok(NodeOutput::data(data))
            }
            "keys" => {
                if let serde_json::Value::Object(map) = &input {
                    let keys: Vec<serde_json::Value> = map
                        .keys()
                        .map(|k| serde_json::Value::String(k.clone()))
                        .collect();
                    Ok(NodeOutput::data(serde_json::Value::Array(keys)))
                } else {
                    Ok(NodeOutput::data(serde_json::json!([])))
                }
            }
            "values" => {
                if let serde_json::Value::Object(map) = &input {
                    let values: Vec<serde_json::Value> = map.values().cloned().collect();
                    Ok(NodeOutput::data(serde_json::Value::Array(values)))
                } else {
                    Ok(NodeOutput::data(serde_json::json!([])))
                }
            }
            "entries" => {
                if let serde_json::Value::Object(map) = &input {
                    let entries: Vec<serde_json::Value> = map
                        .iter()
                        .map(|(k, v)| serde_json::json!({"key": k, "value": v}))
                        .collect();
                    Ok(NodeOutput::data(serde_json::Value::Array(entries)))
                } else {
                    Ok(NodeOutput::data(serde_json::json!([])))
                }
            }
            "flatten" => {
                let mut result = serde_json::Map::new();
                flatten_json(&input, "", &mut result);
                Ok(NodeOutput::data(serde_json::Value::Object(result)))
            }
            "unflatten" => {
                if let serde_json::Value::Object(map) = &input {
                    Ok(NodeOutput::data(unflatten_json(map)))
                } else {
                    Ok(NodeOutput::data(input))
                }
            }
            other => Err(NodeError::InvalidConfig(format!(
                "unknown operation: {}",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_parse() {
        let mut node = JsonParseNode::new();
        node.init(&serde_json::json!({"operation": "parse"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!("{\"a\":1,\"b\":2}"), &ctx)
            .await
            .unwrap();
        assert_eq!(
            output.ports["default"],
            serde_json::json!({"a": 1, "b": 2})
        );
    }

    #[tokio::test]
    async fn test_stringify() {
        let mut node = JsonParseNode::new();
        node.init(&serde_json::json!({"operation": "stringify"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"x": 42}), &ctx)
            .await
            .unwrap();
        let s = output.ports["default"].as_str().unwrap();
        assert!(s.contains("\"x\""));
        assert!(s.contains("42"));
    }

    #[tokio::test]
    async fn test_query() {
        let mut node = JsonParseNode::new();
        node.init(&serde_json::json!({"operation": "query", "path": "a.b"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"a": {"b": 99}}), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"], serde_json::json!(99));
    }

    #[tokio::test]
    async fn test_query_missing_path() {
        let mut node = JsonParseNode::new();
        node.init(&serde_json::json!({"operation": "query", "path": "x.y.z"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"a": 1}), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn test_set() {
        let mut node = JsonParseNode::new();
        node.init(&serde_json::json!({"operation": "set", "path": "a.c", "value": "hello"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"a": {"b": 1}}), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["a"]["c"], serde_json::json!("hello"));
        assert_eq!(output.ports["default"]["a"]["b"], serde_json::json!(1));
    }

    #[tokio::test]
    async fn test_delete() {
        let mut node = JsonParseNode::new();
        node.init(&serde_json::json!({"operation": "delete", "path": "b"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"a": 1, "b": 2, "c": 3}), &ctx)
            .await
            .unwrap();
        assert_eq!(
            output.ports["default"],
            serde_json::json!({"a": 1, "c": 3})
        );
    }

    #[tokio::test]
    async fn test_keys() {
        let mut node = JsonParseNode::new();
        node.init(&serde_json::json!({"operation": "keys"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"x": 1, "y": 2}), &ctx)
            .await
            .unwrap();
        let keys = output.ports["default"].as_array().unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&serde_json::json!("x")));
        assert!(keys.contains(&serde_json::json!("y")));
    }

    #[tokio::test]
    async fn test_values() {
        let mut node = JsonParseNode::new();
        node.init(&serde_json::json!({"operation": "values"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"a": 10, "b": 20}), &ctx)
            .await
            .unwrap();
        let vals = output.ports["default"].as_array().unwrap();
        assert_eq!(vals.len(), 2);
        assert!(vals.contains(&serde_json::json!(10)));
        assert!(vals.contains(&serde_json::json!(20)));
    }

    #[tokio::test]
    async fn test_entries() {
        let mut node = JsonParseNode::new();
        node.init(&serde_json::json!({"operation": "entries"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"k": "v"}), &ctx)
            .await
            .unwrap();
        assert_eq!(
            output.ports["default"],
            serde_json::json!([{"key": "k", "value": "v"}])
        );
    }

    #[tokio::test]
    async fn test_flatten() {
        let mut node = JsonParseNode::new();
        node.init(&serde_json::json!({"operation": "flatten"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"a": {"b": 1, "c": 2}}), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["a.b"], serde_json::json!(1));
        assert_eq!(output.ports["default"]["a.c"], serde_json::json!(2));
    }
}
