use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Merge node — combines multiple input objects into a single unified object.
///
/// This is the graph-native replacement for "collecting results from parallel branches."
/// When multiple upstream nodes feed into a merge node, their outputs are
/// deep-merged into one JSON object.
///
/// Config example:
/// ```json
/// {
///   "strategy": "shallow"
/// }
/// ```
///
/// Strategies:
/// - `"shallow"` (default): Top-level merge. Later keys overwrite earlier ones.
/// - `"deep"`: Recursive merge. Nested objects are merged, arrays are concatenated.
/// - `"array"`: Wraps all inputs into an array.
pub struct MergeNode {
    strategy: MergeStrategy,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum MergeStrategy {
    Shallow,
    Deep,
    Array,
}

impl Default for MergeNode {
    fn default() -> Self {
        Self::new()
    }
}

impl MergeNode {
    pub fn new() -> Self {
        Self {
            strategy: MergeStrategy::Shallow,
        }
    }
}

/// Deep merge two JSON values
fn deep_merge(base: &mut serde_json::Value, overlay: serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(overlay_map)) => {
            for (key, value) in overlay_map {
                let entry = base_map
                    .entry(key)
                    .or_insert(serde_json::Value::Null);
                deep_merge(entry, value);
            }
        }
        (serde_json::Value::Array(base_arr), serde_json::Value::Array(overlay_arr)) => {
            base_arr.extend(overlay_arr);
        }
        (base, overlay) => {
            *base = overlay;
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for MergeNode {
    fn node_type(&self) -> &str {
        "merge"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(strategy) = config.get("strategy").and_then(|v| v.as_str()) {
            self.strategy = match strategy {
                "shallow" => MergeStrategy::Shallow,
                "deep" => MergeStrategy::Deep,
                "array" => MergeStrategy::Array,
                other => {
                    return Err(anp_core::node::traits::NodeError::InvalidConfig(
                        format!(
                            "unknown merge strategy '{}', expected: shallow, deep, array",
                            other
                        ),
                    ))
                }
            };
        }
        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        match self.strategy {
            MergeStrategy::Array => {
                // Wrap input in array if not already
                let result = if input.is_array() {
                    input
                } else {
                    serde_json::json!([input])
                };
                Ok(NodeOutput::data(result))
            }
            MergeStrategy::Shallow => {
                // For shallow merge, if input is an array of objects, merge them
                if let serde_json::Value::Array(items) = &input {
                    let mut merged = serde_json::Map::new();
                    for item in items {
                        if let serde_json::Value::Object(obj) = item {
                            for (k, v) in obj {
                                merged.insert(k.clone(), v.clone());
                            }
                        }
                    }
                    Ok(NodeOutput::data(serde_json::Value::Object(merged))
                        .with_log(format!("shallow-merged {} items", items.len())))
                } else {
                    // Single input passes through
                    Ok(NodeOutput::data(input))
                }
            }
            MergeStrategy::Deep => {
                if let serde_json::Value::Array(items) = &input {
                    let mut merged = serde_json::Value::Object(serde_json::Map::new());
                    for item in items {
                        deep_merge(&mut merged, item.clone());
                    }
                    Ok(NodeOutput::data(merged)
                        .with_log(format!("deep-merged {} items", items.len())))
                } else {
                    Ok(NodeOutput::data(input))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_shallow_merge() {
        let mut node = MergeNode::new();
        node.init(&serde_json::json!({"strategy": "shallow"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let input = serde_json::json!([
            {"name": "Alice", "role": "admin"},
            {"age": 30, "role": "user"}
        ]);
        let output = node.execute(input, &ctx).await.unwrap();

        let result = &output.ports["default"];
        assert_eq!(result["name"], "Alice");
        assert_eq!(result["age"], 30);
        assert_eq!(result["role"], "user"); // later overwrites
    }

    #[tokio::test]
    async fn test_deep_merge() {
        let mut node = MergeNode::new();
        node.init(&serde_json::json!({"strategy": "deep"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let input = serde_json::json!([
            {"user": {"name": "Alice"}, "tags": ["admin"]},
            {"user": {"age": 30}, "tags": ["active"]}
        ]);
        let output = node.execute(input, &ctx).await.unwrap();

        let result = &output.ports["default"];
        assert_eq!(result["user"]["name"], "Alice");
        assert_eq!(result["user"]["age"], 30);
        assert_eq!(result["tags"], serde_json::json!(["admin", "active"]));
    }

    #[tokio::test]
    async fn test_array_strategy() {
        let mut node = MergeNode::new();
        node.init(&serde_json::json!({"strategy": "array"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"key": "value"}), &ctx)
            .await
            .unwrap();

        let result = &output.ports["default"];
        assert_eq!(result, &serde_json::json!([{"key": "value"}]));
    }

    #[tokio::test]
    async fn test_single_input_passthrough() {
        let mut node = MergeNode::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"key": "value"}), &ctx)
            .await
            .unwrap();

        assert_eq!(output.ports["default"], serde_json::json!({"key": "value"}));
    }

    #[tokio::test]
    async fn test_invalid_strategy() {
        let mut node = MergeNode::new();
        let result = node.init(&serde_json::json!({"strategy": "invalid"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_deep_merge_nested() {
        let mut node = MergeNode::new();
        node.init(&serde_json::json!({"strategy": "deep"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let input = serde_json::json!([
            {"config": {"db": {"host": "localhost"}}},
            {"config": {"db": {"port": 5432}, "cache": true}}
        ]);
        let output = node.execute(input, &ctx).await.unwrap();

        let result = &output.ports["default"];
        assert_eq!(result["config"]["db"]["host"], "localhost");
        assert_eq!(result["config"]["db"]["port"], 5432);
        assert_eq!(result["config"]["cache"], true);
    }
}
