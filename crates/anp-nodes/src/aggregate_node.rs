use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// AggregateNode — multi-field aggregation pipeline over array inputs.
///
/// Config example:
/// ```json
/// {
///   "input_field": "data",
///   "aggregations": [
///     { "field": "price", "operation": "sum", "output_key": "total_price" },
///     { "field": "name", "operation": "count", "output_key": "name_count" },
///     { "field": "name", "operation": "concat", "output_key": "all_names", "separator": " | " }
///   ]
/// }
/// ```
///
/// Processes an array input and computes all aggregations in one pass.
/// Output: `{ "results": { "output_key1": value1, ... }, "input_count": N }`
pub struct AggregateNode {
    input_field: Option<String>,
    aggregations: Vec<AggregationSpec>,
}

#[derive(Debug, Clone)]
struct AggregationSpec {
    field: String,
    operation: String,
    output_key: String,
    separator: String,
}

impl Default for AggregateNode {
    fn default() -> Self {
        Self::new()
    }
}

impl AggregateNode {
    pub fn new() -> Self {
        Self {
            input_field: None,
            aggregations: Vec::new(),
        }
    }
}

/// Resolve a dot-path on a JSON value.
fn resolve_dot_path(value: &serde_json::Value, path: &str) -> Option<serde_json::Value> {
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

fn find_input_array(
    input: &serde_json::Value,
    input_field: &Option<String>,
) -> Option<Vec<serde_json::Value>> {
    // If input_field is specified, use it
    if let Some(ref field) = input_field {
        return resolve_dot_path(input, field).and_then(|v| v.as_array().cloned());
    }

    // Try common field names
    for key in &["data", "items"] {
        if let Some(arr) = input.get(*key).and_then(|v| v.as_array()) {
            return Some(arr.clone());
        }
    }

    // Try input itself as array
    if let Some(arr) = input.as_array() {
        return Some(arr.clone());
    }

    None
}

#[async_trait::async_trait]
impl NodeExecutor for AggregateNode {
    fn node_type(&self) -> &str {
        "aggregate"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(field) = config.get("input_field").and_then(|v| v.as_str()) {
            self.input_field = Some(field.to_string());
        }

        let aggs = config
            .get("aggregations")
            .and_then(|v| v.as_array())
            .ok_or_else(|| NodeError::InvalidConfig("'aggregations' array is required".into()))?;

        if aggs.is_empty() {
            return Err(NodeError::InvalidConfig(
                "aggregations array must not be empty".into(),
            ));
        }

        for agg in aggs {
            let field = agg
                .get("field")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    NodeError::InvalidConfig("each aggregation requires a 'field'".into())
                })?
                .to_string();

            let operation = agg
                .get("operation")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    NodeError::InvalidConfig("each aggregation requires an 'operation'".into())
                })?;

            match operation {
                "sum" | "avg" | "min" | "max" | "count" | "first" | "last" | "concat" => {}
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "invalid aggregation operation '{}': must be sum, avg, min, max, count, first, last, or concat",
                        other
                    )));
                }
            }

            let output_key = agg
                .get("output_key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    NodeError::InvalidConfig("each aggregation requires an 'output_key'".into())
                })?
                .to_string();

            let separator = agg
                .get("separator")
                .and_then(|v| v.as_str())
                .unwrap_or(", ")
                .to_string();

            self.aggregations.push(AggregationSpec {
                field,
                operation: operation.to_string(),
                output_key,
                separator,
            });
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let items = find_input_array(&input, &self.input_field).ok_or_else(|| {
            NodeError::ExecutionFailed(
                "could not find input array: provide 'input_field' config or use 'data', 'items', or pass an array directly".into(),
            )
        })?;

        let input_count = items.len();
        let mut results = serde_json::Map::new();

        for agg in &self.aggregations {
            let values: Vec<serde_json::Value> = items
                .iter()
                .filter_map(|item| resolve_dot_path(item, &agg.field))
                .filter(|v| !v.is_null())
                .collect();

            let result = match agg.operation.as_str() {
                "count" => {
                    serde_json::json!(values.len())
                }
                "sum" => {
                    let sum: f64 = values.iter().filter_map(|v| v.as_f64()).sum();
                    serde_json::json!(sum)
                }
                "avg" => {
                    let nums: Vec<f64> = values.iter().filter_map(|v| v.as_f64()).collect();
                    if nums.is_empty() {
                        serde_json::Value::Null
                    } else {
                        let avg = nums.iter().sum::<f64>() / nums.len() as f64;
                        serde_json::json!(avg)
                    }
                }
                "min" => values
                    .iter()
                    .filter_map(|v| v.as_f64())
                    .fold(None, |min: Option<f64>, x| {
                        Some(min.map_or(x, |m| m.min(x)))
                    })
                    .map(|v| serde_json::json!(v))
                    .unwrap_or(serde_json::Value::Null),
                "max" => values
                    .iter()
                    .filter_map(|v| v.as_f64())
                    .fold(None, |max: Option<f64>, x| {
                        Some(max.map_or(x, |m| m.max(x)))
                    })
                    .map(|v| serde_json::json!(v))
                    .unwrap_or(serde_json::Value::Null),
                "first" => values.first().cloned().unwrap_or(serde_json::Value::Null),
                "last" => values.last().cloned().unwrap_or(serde_json::Value::Null),
                "concat" => {
                    let strings: Vec<String> = values
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    serde_json::json!(strings.join(&agg.separator))
                }
                _ => serde_json::Value::Null,
            };

            results.insert(agg.output_key.clone(), result);
        }

        Ok(NodeOutput::data(serde_json::json!({
            "results": results,
            "input_count": input_count,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ExecutionContext {
        ExecutionContext::default()
    }

    #[tokio::test]
    async fn test_sum_aggregation() {
        let mut node = AggregateNode::new();
        node.init(&serde_json::json!({
            "aggregations": [
                { "field": "price", "operation": "sum", "output_key": "total" }
            ]
        }))
        .await
        .unwrap();

        let input = serde_json::json!({
            "data": [
                { "price": 10 },
                { "price": 20 },
                { "price": 30 }
            ]
        });

        let result = node.execute(input, &ctx()).await.unwrap();
        let out = result.ports.get("default").unwrap();
        assert_eq!(out["results"]["total"], 60.0);
        assert_eq!(out["input_count"], 3);
    }

    #[tokio::test]
    async fn test_avg_aggregation() {
        let mut node = AggregateNode::new();
        node.init(&serde_json::json!({
            "aggregations": [
                { "field": "score", "operation": "avg", "output_key": "avg_score" }
            ]
        }))
        .await
        .unwrap();

        let input = serde_json::json!({
            "data": [
                { "score": 10 },
                { "score": 20 },
                { "score": 30 }
            ]
        });

        let result = node.execute(input, &ctx()).await.unwrap();
        let out = result.ports.get("default").unwrap();
        assert_eq!(out["results"]["avg_score"], 20.0);
    }

    #[tokio::test]
    async fn test_min_max_count() {
        let mut node = AggregateNode::new();
        node.init(&serde_json::json!({
            "aggregations": [
                { "field": "v", "operation": "min", "output_key": "min_v" },
                { "field": "v", "operation": "max", "output_key": "max_v" },
                { "field": "v", "operation": "count", "output_key": "cnt" }
            ]
        }))
        .await
        .unwrap();

        let input = serde_json::json!({
            "data": [
                { "v": 5 },
                { "v": 1 },
                { "v": 9 },
                { "v": null }
            ]
        });

        let result = node.execute(input, &ctx()).await.unwrap();
        let out = result.ports.get("default").unwrap();
        assert_eq!(out["results"]["min_v"], 1.0);
        assert_eq!(out["results"]["max_v"], 9.0);
        assert_eq!(out["results"]["cnt"], 3); // null excluded
    }

    #[tokio::test]
    async fn test_first_last_concat() {
        let mut node = AggregateNode::new();
        node.init(&serde_json::json!({
            "aggregations": [
                { "field": "name", "operation": "first", "output_key": "first_name" },
                { "field": "name", "operation": "last", "output_key": "last_name" },
                { "field": "name", "operation": "concat", "output_key": "all_names", "separator": " | " }
            ]
        }))
        .await
        .unwrap();

        let input = serde_json::json!({
            "items": [
                { "name": "Alice" },
                { "name": "Bob" },
                { "name": "Charlie" }
            ]
        });

        let result = node.execute(input, &ctx()).await.unwrap();
        let out = result.ports.get("default").unwrap();
        assert_eq!(out["results"]["first_name"], "Alice");
        assert_eq!(out["results"]["last_name"], "Charlie");
        assert_eq!(out["results"]["all_names"], "Alice | Bob | Charlie");
    }

    #[tokio::test]
    async fn test_missing_aggregations_config() {
        let mut node = AggregateNode::new();
        let result = node.init(&serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let mut node = AggregateNode::new();
        let result = node
            .init(&serde_json::json!({
                "aggregations": [
                    { "field": "x", "operation": "invalid", "output_key": "y" }
                ]
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_input_field_config() {
        let mut node = AggregateNode::new();
        node.init(&serde_json::json!({
            "input_field": "records",
            "aggregations": [
                { "field": "val", "operation": "sum", "output_key": "total" }
            ]
        }))
        .await
        .unwrap();

        let input = serde_json::json!({
            "records": [
                { "val": 1 },
                { "val": 2 }
            ]
        });

        let result = node.execute(input, &ctx()).await.unwrap();
        let out = result.ports.get("default").unwrap();
        assert_eq!(out["results"]["total"], 3.0);
    }

    #[tokio::test]
    async fn test_no_array_found_error() {
        let mut node = AggregateNode::new();
        node.init(&serde_json::json!({
            "aggregations": [
                { "field": "x", "operation": "sum", "output_key": "y" }
            ]
        }))
        .await
        .unwrap();

        let result = node
            .execute(serde_json::json!({"foo": "bar"}), &ctx())
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_node_type_and_kind() {
        let node = AggregateNode::new();
        assert_eq!(node.node_type(), "aggregate");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
