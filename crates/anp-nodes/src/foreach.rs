use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// ForEach node — iterates over an array and emits each element individually.
///
/// This is the graph-native replacement for `for` loops. Instead of writing
/// loop code, you connect this node to downstream processors that handle
/// each element. Results are collected back into an array.
///
/// Config example:
/// ```json
/// {
///   "field": "items",
///   "collect": true
/// }
/// ```
///
/// Input: `{ "items": [1, 2, 3] }` (or a direct array)
/// Output (collect=false): emits each element to "default" port
/// Output (collect=true): emits `{ "items": [...results], "count": N }`
pub struct ForEachNode {
    field: Option<String>,
    collect: bool,
}

impl Default for ForEachNode {
    fn default() -> Self {
        Self::new()
    }
}

impl ForEachNode {
    pub fn new() -> Self {
        Self {
            field: None,
            collect: true,
        }
    }
}

/// Resolve a dot-path on a JSON value
fn resolve_field(value: &serde_json::Value, path: &str) -> Option<serde_json::Value> {
    let mut current = value;
    for part in path.split('.') {
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

#[async_trait::async_trait]
impl NodeExecutor for ForEachNode {
    fn node_type(&self) -> &str {
        "foreach"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(field) = config.get("field").and_then(|v| v.as_str()) {
            self.field = Some(field.to_string());
        }

        if let Some(collect) = config.get("collect").and_then(|v| v.as_bool()) {
            self.collect = collect;
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        // Extract the array to iterate over
        let arr = if let Some(field) = &self.field {
            resolve_field(&input, field).ok_or_else(|| {
                NodeError::MissingInput(format!("field '{}' not found in input", field))
            })?
        } else {
            input.clone()
        };

        let items = match arr {
            serde_json::Value::Array(items) => items,
            // Single value → treat as 1-element array
            other => vec![other],
        };

        let count = items.len();

        if self.collect {
            // Emit all items as an array with metadata
            Ok(NodeOutput::data(serde_json::json!({
                "items": items,
                "count": count
            }))
            .with_log(format!("iterating over {} items", count)))
        } else {
            // Emit the array directly — downstream processors handle each element
            Ok(NodeOutput::data(serde_json::Value::Array(items))
                .with_log(format!("emitting {} items", count)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_foreach_direct_array() {
        let mut node = ForEachNode::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!([1, 2, 3]), &ctx)
            .await
            .unwrap();

        let result = &output.ports["default"];
        assert_eq!(result["count"], 3);
        assert_eq!(result["items"], serde_json::json!([1, 2, 3]));
    }

    #[tokio::test]
    async fn test_foreach_field_extraction() {
        let mut node = ForEachNode::new();
        node.init(&serde_json::json!({"field": "data.users"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let input = serde_json::json!({
            "data": {
                "users": ["alice", "bob", "charlie"]
            }
        });
        let output = node.execute(input, &ctx).await.unwrap();

        let result = &output.ports["default"];
        assert_eq!(result["count"], 3);
        assert_eq!(
            result["items"],
            serde_json::json!(["alice", "bob", "charlie"])
        );
    }

    #[tokio::test]
    async fn test_foreach_missing_field() {
        let mut node = ForEachNode::new();
        node.init(&serde_json::json!({"field": "nonexistent"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let result = node.execute(serde_json::json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_foreach_single_value_wraps() {
        let mut node = ForEachNode::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!("hello"), &ctx)
            .await
            .unwrap();

        let result = &output.ports["default"];
        assert_eq!(result["count"], 1);
        assert_eq!(result["items"], serde_json::json!(["hello"]));
    }

    #[tokio::test]
    async fn test_foreach_no_collect() {
        let mut node = ForEachNode::new();
        node.init(&serde_json::json!({"collect": false}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!([10, 20, 30]), &ctx)
            .await
            .unwrap();

        let result = &output.ports["default"];
        assert_eq!(result, &serde_json::json!([10, 20, 30]));
    }

    #[tokio::test]
    async fn test_foreach_empty_array() {
        let mut node = ForEachNode::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::json!([]), &ctx).await.unwrap();

        let result = &output.ports["default"];
        assert_eq!(result["count"], 0);
        assert_eq!(result["items"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn test_foreach_nested_objects() {
        let mut node = ForEachNode::new();
        node.init(&serde_json::json!({"field": "orders"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let input = serde_json::json!({
            "orders": [
                {"id": 1, "total": 100},
                {"id": 2, "total": 200}
            ]
        });
        let output = node.execute(input, &ctx).await.unwrap();

        let result = &output.ports["default"];
        assert_eq!(result["count"], 2);
        assert_eq!(result["items"][0]["id"], 1);
        assert_eq!(result["items"][1]["total"], 200);
    }
}
