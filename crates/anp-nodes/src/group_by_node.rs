use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// GroupByNode — group array items by field value
pub struct GroupByNode {
    field: String,
    aggregate: Option<String>,
    aggregate_field: Option<String>,
}

impl Default for GroupByNode {
    fn default() -> Self {
        Self::new()
    }
}

impl GroupByNode {
    pub fn new() -> Self {
        Self {
            field: String::new(),
            aggregate: None,
            aggregate_field: None,
        }
    }
}

fn get_dot_path<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = value;
    for part in parts {
        current = current.get(part)?;
    }
    Some(current)
}

fn value_to_group_key(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

fn extract_array(input: &serde_json::Value) -> Option<&Vec<serde_json::Value>> {
    if let Some(arr) = input.as_array() {
        return Some(arr);
    }
    // Try common fields
    for field in &["items", "data", "rows", "results"] {
        if let Some(arr) = input.get(*field).and_then(|v| v.as_array()) {
            return Some(arr);
        }
    }
    None
}

#[async_trait::async_trait]
impl NodeExecutor for GroupByNode {
    fn node_type(&self) -> &str {
        "group_by"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        self.field = config
            .get("field")
            .and_then(|v| v.as_str())
            .ok_or_else(|| NodeError::InvalidConfig("'field' is required for group_by".into()))?
            .to_string();

        if let Some(agg) = config.get("aggregate").and_then(|v| v.as_str()) {
            match agg {
                "count" | "sum" | "avg" | "min" | "max" | "list" => {
                    self.aggregate = Some(agg.to_string());
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown aggregate '{}'. Use: count, sum, avg, min, max, list",
                        other
                    )));
                }
            }
        }

        if let Some(af) = config.get("aggregate_field").and_then(|v| v.as_str()) {
            self.aggregate_field = Some(af.to_string());
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let arr = extract_array(&input).ok_or_else(|| {
            NodeError::ExecutionFailed("group_by requires an array input".into())
        })?;

        // Group items by field value
        let mut groups: std::collections::BTreeMap<String, Vec<&serde_json::Value>> =
            std::collections::BTreeMap::new();

        for item in arr {
            let key = get_dot_path(item, &self.field)
                .map(value_to_group_key)
                .unwrap_or_else(|| "_undefined".to_string());
            groups.entry(key).or_default().push(item);
        }

        let total_items = arr.len();
        let group_count = groups.len();

        let result = match self.aggregate.as_deref() {
            Some("count") => {
                let mut group_obj = serde_json::Map::new();
                for (key, items) in &groups {
                    group_obj.insert(key.clone(), serde_json::json!(items.len()));
                }
                serde_json::json!({
                    "groups": group_obj,
                    "group_count": group_count,
                    "total_items": total_items,
                })
            }
            Some("sum") | Some("avg") | Some("min") | Some("max") => {
                let agg = self.aggregate.as_deref().unwrap();
                let agg_field = self.aggregate_field.as_deref().unwrap_or("");
                let mut group_obj = serde_json::Map::new();

                for (key, items) in &groups {
                    let values: Vec<f64> = items
                        .iter()
                        .filter_map(|item| {
                            let val: &serde_json::Value = if agg_field.is_empty() {
                                item
                            } else {
                                get_dot_path(item, agg_field)?
                            };
                            val.as_f64()
                        })
                        .collect();

                    let result_val = if values.is_empty() {
                        serde_json::Value::Null
                    } else {
                        match agg {
                            "sum" => serde_json::json!(values.iter().sum::<f64>()),
                            "avg" => {
                                serde_json::json!(values.iter().sum::<f64>() / values.len() as f64)
                            }
                            "min" => serde_json::json!(values
                                .iter()
                                .cloned()
                                .fold(f64::INFINITY, f64::min)),
                            "max" => serde_json::json!(values
                                .iter()
                                .cloned()
                                .fold(f64::NEG_INFINITY, f64::max)),
                            _ => serde_json::Value::Null,
                        }
                    };
                    group_obj.insert(key.clone(), result_val);
                }

                serde_json::json!({
                    "groups": group_obj,
                    "group_count": group_count,
                    "total_items": total_items,
                })
            }
            _ => {
                // "list" or no aggregate — group into arrays
                let mut group_obj = serde_json::Map::new();
                for (key, items) in &groups {
                    let arr: Vec<serde_json::Value> = items.iter().map(|v| (*v).clone()).collect();
                    group_obj.insert(key.clone(), serde_json::Value::Array(arr));
                }
                serde_json::json!({
                    "groups": group_obj,
                    "group_count": group_count,
                    "total_items": total_items,
                })
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
    async fn test_group_by_list() {
        let mut node = GroupByNode::new();
        node.init(&json!({"field": "category"})).await.unwrap();
        let input = json!([
            {"name": "apple", "category": "fruit"},
            {"name": "banana", "category": "fruit"},
            {"name": "carrot", "category": "vegetable"}
        ]);
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["group_count"], 2);
        assert_eq!(data["total_items"], 3);
        assert_eq!(data["groups"]["fruit"].as_array().unwrap().len(), 2);
        assert_eq!(data["groups"]["vegetable"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_group_by_count() {
        let mut node = GroupByNode::new();
        node.init(&json!({"field": "status", "aggregate": "count"}))
            .await
            .unwrap();
        let input = json!([
            {"id": 1, "status": "active"},
            {"id": 2, "status": "active"},
            {"id": 3, "status": "inactive"}
        ]);
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["groups"]["active"], 2);
        assert_eq!(data["groups"]["inactive"], 1);
    }

    #[tokio::test]
    async fn test_group_by_sum() {
        let mut node = GroupByNode::new();
        node.init(&json!({"field": "dept", "aggregate": "sum", "aggregate_field": "salary"}))
            .await
            .unwrap();
        let input = json!([
            {"name": "Alice", "dept": "eng", "salary": 100},
            {"name": "Bob", "dept": "eng", "salary": 120},
            {"name": "Carol", "dept": "hr", "salary": 90}
        ]);
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["groups"]["eng"], 220.0);
        assert_eq!(data["groups"]["hr"], 90.0);
    }

    #[tokio::test]
    async fn test_group_by_avg() {
        let mut node = GroupByNode::new();
        node.init(&json!({"field": "type", "aggregate": "avg", "aggregate_field": "score"}))
            .await
            .unwrap();
        let input = json!([
            {"type": "A", "score": 10},
            {"type": "A", "score": 20},
            {"type": "B", "score": 30}
        ]);
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["groups"]["A"], 15.0);
        assert_eq!(data["groups"]["B"], 30.0);
    }

    #[tokio::test]
    async fn test_group_by_missing_field_config() {
        let mut node = GroupByNode::new();
        let result = node.init(&json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_group_by_nested_field() {
        let mut node = GroupByNode::new();
        node.init(&json!({"field": "meta.region"})).await.unwrap();
        let input = json!([
            {"id": 1, "meta": {"region": "US"}},
            {"id": 2, "meta": {"region": "EU"}},
            {"id": 3, "meta": {"region": "US"}}
        ]);
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["groups"]["US"].as_array().unwrap().len(), 2);
        assert_eq!(data["groups"]["EU"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_group_by_from_items_field() {
        let mut node = GroupByNode::new();
        node.init(&json!({"field": "color"})).await.unwrap();
        let input = json!({"items": [
            {"name": "sky", "color": "blue"},
            {"name": "grass", "color": "green"},
            {"name": "ocean", "color": "blue"}
        ]});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["group_count"], 2);
    }
}
