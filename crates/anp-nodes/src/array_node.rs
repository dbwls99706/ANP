use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// ArrayNode — array manipulation operations
pub struct ArrayNode {
    operation: String,
    start: i64,
    end: Option<i64>,
    field: String,
    value: serde_json::Value,
    size: usize,
    count: usize,
    order: String,
}

impl Default for ArrayNode {
    fn default() -> Self {
        Self::new()
    }
}

impl ArrayNode {
    pub fn new() -> Self {
        Self {
            operation: "length".to_string(),
            start: 0,
            end: None,
            field: String::new(),
            value: serde_json::Value::Null,
            size: 3,
            count: 5,
            order: "asc".to_string(),
        }
    }
}

fn resolve_field<'a>(value: &'a serde_json::Value, field: &str) -> Option<&'a serde_json::Value> {
    let parts: Vec<&str> = field.split('.').collect();
    let mut current = value;
    for part in parts {
        current = current.get(part)?;
    }
    Some(current)
}

#[async_trait::async_trait]
impl NodeExecutor for ArrayNode {
    fn node_type(&self) -> &str {
        "array"
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
            "sort", "reverse", "slice", "flatten", "unique", "find", "length", "first",
            "last", "push", "pop", "compact", "zip", "chunk", "map_field", "pluck",
            "index_of", "includes", "take", "skip",
        ];
        if !valid_ops.contains(&self.operation.as_str()) {
            return Err(NodeError::InvalidConfig(format!(
                "unknown operation '{}'. Valid: {}",
                self.operation,
                valid_ops.join(", ")
            )));
        }

        if let Some(s) = config.get("start").and_then(|v| v.as_i64()) {
            self.start = s;
        }
        if let Some(e) = config.get("end").and_then(|v| v.as_i64()) {
            self.end = Some(e);
        }
        if let Some(f) = config.get("field").and_then(|v| v.as_str()) {
            self.field = f.to_string();
        }
        if let Some(v) = config.get("value") {
            self.value = v.clone();
        }
        if let Some(s) = config.get("size").and_then(|v| v.as_u64()) {
            self.size = s as usize;
        }
        if let Some(c) = config.get("count").and_then(|v| v.as_u64()) {
            self.count = c as usize;
        }
        if let Some(o) = config.get("order").and_then(|v| v.as_str()) {
            self.order = o.to_string();
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        // push can work with non-array input wrapped in the value
        if self.operation == "push" {
            let mut arr = match input {
                serde_json::Value::Array(a) => a,
                _ => {
                    return Err(NodeError::ExecutionFailed(
                        "push operation requires an array input".into(),
                    ));
                }
            };
            arr.push(self.value.clone());
            return Ok(NodeOutput::data(serde_json::Value::Array(arr)));
        }

        let arr = input.as_array().ok_or_else(|| {
            NodeError::ExecutionFailed("input must be an array".into())
        })?;

        let result = match self.operation.as_str() {
            "sort" => {
                let mut sorted = arr.clone();
                if self.field.is_empty() {
                    // Sort primitives
                    sorted.sort_by(|a, b| {
                        let a_str = value_sort_key(a);
                        let b_str = value_sort_key(b);
                        if self.order == "desc" {
                            b_str.partial_cmp(&a_str).unwrap_or(std::cmp::Ordering::Equal)
                        } else {
                            a_str.partial_cmp(&b_str).unwrap_or(std::cmp::Ordering::Equal)
                        }
                    });
                } else {
                    // Sort by field
                    let field = self.field.clone();
                    sorted.sort_by(|a, b| {
                        let a_val = resolve_field(a, &field).map(value_sort_key).unwrap_or_default();
                        let b_val = resolve_field(b, &field).map(value_sort_key).unwrap_or_default();
                        if self.order == "desc" {
                            b_val.partial_cmp(&a_val).unwrap_or(std::cmp::Ordering::Equal)
                        } else {
                            a_val.partial_cmp(&b_val).unwrap_or(std::cmp::Ordering::Equal)
                        }
                    });
                }
                serde_json::Value::Array(sorted)
            }
            "reverse" => {
                let mut reversed = arr.clone();
                reversed.reverse();
                serde_json::Value::Array(reversed)
            }
            "slice" => {
                let len = arr.len() as i64;
                let start = if self.start < 0 {
                    (len + self.start).max(0) as usize
                } else {
                    (self.start as usize).min(arr.len())
                };
                let end = match self.end {
                    Some(e) => {
                        if e < 0 {
                            (len + e).max(0) as usize
                        } else {
                            (e as usize).min(arr.len())
                        }
                    }
                    None => arr.len(),
                };
                if start < end {
                    serde_json::Value::Array(arr[start..end].to_vec())
                } else {
                    serde_json::Value::Array(vec![])
                }
            }
            "flatten" => {
                let mut flat = Vec::new();
                for item in arr {
                    if let serde_json::Value::Array(inner) = item {
                        flat.extend(inner.iter().cloned());
                    } else {
                        flat.push(item.clone());
                    }
                }
                serde_json::Value::Array(flat)
            }
            "unique" => {
                let mut seen = Vec::new();
                let mut unique = Vec::new();
                for item in arr {
                    let key = item.to_string();
                    if !seen.contains(&key) {
                        seen.push(key);
                        unique.push(item.clone());
                    }
                }
                serde_json::Value::Array(unique)
            }
            "find" => {
                if self.field.is_empty() {
                    // Find by value
                    arr.iter()
                        .find(|item| **item == self.value)
                        .cloned()
                        .unwrap_or(serde_json::Value::Null)
                } else {
                    // Find by field value
                    arr.iter()
                        .find(|item| {
                            resolve_field(item, &self.field)
                                .map(|v| *v == self.value)
                                .unwrap_or(false)
                        })
                        .cloned()
                        .unwrap_or(serde_json::Value::Null)
                }
            }
            "length" => serde_json::json!(arr.len()),
            "first" => arr.first().cloned().unwrap_or(serde_json::Value::Null),
            "last" => arr.last().cloned().unwrap_or(serde_json::Value::Null),
            "pop" => {
                let mut popped = arr.clone();
                let removed = popped.pop().unwrap_or(serde_json::Value::Null);
                serde_json::json!({
                    "array": popped,
                    "removed": removed
                })
            }
            "compact" => {
                let compacted: Vec<serde_json::Value> = arr
                    .iter()
                    .filter(|v| !v.is_null())
                    .cloned()
                    .collect();
                serde_json::Value::Array(compacted)
            }
            "zip" => {
                // Expects array of arrays, zips them together
                let arrays: Vec<&Vec<serde_json::Value>> = arr
                    .iter()
                    .filter_map(|v| v.as_array())
                    .collect();
                if arrays.is_empty() {
                    return Ok(NodeOutput::data(serde_json::Value::Array(vec![])));
                }
                let max_len = arrays.iter().map(|a| a.len()).max().unwrap_or(0);
                let mut zipped = Vec::new();
                for i in 0..max_len {
                    let tuple: Vec<serde_json::Value> = arrays
                        .iter()
                        .map(|a| a.get(i).cloned().unwrap_or(serde_json::Value::Null))
                        .collect();
                    zipped.push(serde_json::Value::Array(tuple));
                }
                serde_json::Value::Array(zipped)
            }
            "chunk" => {
                let chunks: Vec<serde_json::Value> = arr
                    .chunks(self.size.max(1))
                    .map(|chunk| serde_json::Value::Array(chunk.to_vec()))
                    .collect();
                serde_json::Value::Array(chunks)
            }
            "map_field" | "pluck" => {
                if self.field.is_empty() {
                    return Err(NodeError::ExecutionFailed(
                        "map_field/pluck requires 'field' in config".into(),
                    ));
                }
                let plucked: Vec<serde_json::Value> = arr
                    .iter()
                    .map(|item| {
                        resolve_field(item, &self.field)
                            .cloned()
                            .unwrap_or(serde_json::Value::Null)
                    })
                    .collect();
                serde_json::Value::Array(plucked)
            }
            "index_of" => {
                let idx = arr.iter().position(|item| *item == self.value);
                match idx {
                    Some(i) => serde_json::json!(i),
                    None => serde_json::json!(-1),
                }
            }
            "includes" => {
                serde_json::json!(arr.contains(&self.value))
            }
            "take" => {
                let taken: Vec<serde_json::Value> = arr.iter().take(self.count).cloned().collect();
                serde_json::Value::Array(taken)
            }
            "skip" => {
                let skipped: Vec<serde_json::Value> = arr.iter().skip(self.count).cloned().collect();
                serde_json::Value::Array(skipped)
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

    #[tokio::test]
    async fn test_sort_asc() {
        let mut node = ArrayNode::new();
        node.init(&serde_json::json!({"operation": "sort", "order": "asc"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!([3, 1, 2]), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"], serde_json::json!([1, 2, 3]));
    }

    #[tokio::test]
    async fn test_reverse() {
        let mut node = ArrayNode::new();
        node.init(&serde_json::json!({"operation": "reverse"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!([1, 2, 3]), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"], serde_json::json!([3, 2, 1]));
    }

    #[tokio::test]
    async fn test_slice() {
        let mut node = ArrayNode::new();
        node.init(&serde_json::json!({"operation": "slice", "start": 1, "end": 3}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!([10, 20, 30, 40, 50]), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"], serde_json::json!([20, 30]));
    }

    #[tokio::test]
    async fn test_flatten() {
        let mut node = ArrayNode::new();
        node.init(&serde_json::json!({"operation": "flatten"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!([[1, 2], [3, 4], 5]), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"], serde_json::json!([1, 2, 3, 4, 5]));
    }

    #[tokio::test]
    async fn test_unique() {
        let mut node = ArrayNode::new();
        node.init(&serde_json::json!({"operation": "unique"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!([1, 2, 2, 3, 3, 3]), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"], serde_json::json!([1, 2, 3]));
    }

    #[tokio::test]
    async fn test_find_by_field() {
        let mut node = ArrayNode::new();
        node.init(&serde_json::json!({"operation": "find", "field": "name", "value": "bob"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let input = serde_json::json!([
            {"name": "alice", "age": 30},
            {"name": "bob", "age": 25}
        ]);
        let output = node.execute(input, &ctx).await.unwrap();
        assert_eq!(
            output.ports["default"],
            serde_json::json!({"name": "bob", "age": 25})
        );
    }

    #[tokio::test]
    async fn test_length() {
        let mut node = ArrayNode::new();
        node.init(&serde_json::json!({"operation": "length"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!([1, 2, 3, 4]), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"], serde_json::json!(4));
    }

    #[tokio::test]
    async fn test_first_and_last() {
        let mut node = ArrayNode::new();
        node.init(&serde_json::json!({"operation": "first"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!([10, 20, 30]), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"], serde_json::json!(10));

        let mut node2 = ArrayNode::new();
        node2
            .init(&serde_json::json!({"operation": "last"}))
            .await
            .unwrap();
        let output2 = node2
            .execute(serde_json::json!([10, 20, 30]), &ctx)
            .await
            .unwrap();
        assert_eq!(output2.ports["default"], serde_json::json!(30));
    }

    #[tokio::test]
    async fn test_push() {
        let mut node = ArrayNode::new();
        node.init(&serde_json::json!({"operation": "push", "value": 4}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!([1, 2, 3]), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"], serde_json::json!([1, 2, 3, 4]));
    }

    #[tokio::test]
    async fn test_pop() {
        let mut node = ArrayNode::new();
        node.init(&serde_json::json!({"operation": "pop"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!([1, 2, 3]), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["array"], serde_json::json!([1, 2]));
        assert_eq!(output.ports["default"]["removed"], serde_json::json!(3));
    }

    #[tokio::test]
    async fn test_compact() {
        let mut node = ArrayNode::new();
        node.init(&serde_json::json!({"operation": "compact"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!([1, null, 2, null, 3]), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"], serde_json::json!([1, 2, 3]));
    }

    #[tokio::test]
    async fn test_chunk() {
        let mut node = ArrayNode::new();
        node.init(&serde_json::json!({"operation": "chunk", "size": 2}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!([1, 2, 3, 4, 5]), &ctx)
            .await
            .unwrap();
        assert_eq!(
            output.ports["default"],
            serde_json::json!([[1, 2], [3, 4], [5]])
        );
    }

    #[tokio::test]
    async fn test_pluck() {
        let mut node = ArrayNode::new();
        node.init(&serde_json::json!({"operation": "pluck", "field": "name"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let input = serde_json::json!([
            {"name": "alice", "age": 30},
            {"name": "bob", "age": 25}
        ]);
        let output = node.execute(input, &ctx).await.unwrap();
        assert_eq!(
            output.ports["default"],
            serde_json::json!(["alice", "bob"])
        );
    }
}

/// Convert a JSON value to a sortable key
fn value_sort_key(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Number(n) => {
            // Pad numbers for proper string sorting
            format!("{:020.6}", n.as_f64().unwrap_or(0.0))
        }
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => format!("{}", b),
        _ => v.to_string(),
    }
}
