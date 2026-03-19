use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// PivotNode — pivot, unpivot, and transpose data transformations
pub struct PivotNode {
    operation: String,
    row_field: String,
    column_field: String,
    value_field: String,
    key_name: String,
    value_name: String,
    exclude: Vec<String>,
}

impl Default for PivotNode {
    fn default() -> Self {
        Self::new()
    }
}

impl PivotNode {
    pub fn new() -> Self {
        Self {
            operation: String::new(),
            row_field: String::new(),
            column_field: String::new(),
            value_field: String::new(),
            key_name: "key".to_string(),
            value_name: "value".to_string(),
            exclude: Vec::new(),
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for PivotNode {
    fn node_type(&self) -> &str {
        "pivot"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        self.operation = config
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| NodeError::InvalidConfig("'operation' is required for pivot".into()))?
            .to_string();

        match self.operation.as_str() {
            "pivot" | "unpivot" | "transpose" => {}
            other => {
                return Err(NodeError::InvalidConfig(format!(
                    "unknown operation '{}'. Use: pivot, unpivot, transpose",
                    other
                )));
            }
        }

        if let Some(v) = config.get("row_field").and_then(|v| v.as_str()) {
            self.row_field = v.to_string();
        }
        if let Some(v) = config.get("column_field").and_then(|v| v.as_str()) {
            self.column_field = v.to_string();
        }
        if let Some(v) = config.get("value_field").and_then(|v| v.as_str()) {
            self.value_field = v.to_string();
        }
        if let Some(v) = config.get("key_name").and_then(|v| v.as_str()) {
            self.key_name = v.to_string();
        }
        if let Some(v) = config.get("value_name").and_then(|v| v.as_str()) {
            self.value_name = v.to_string();
        }
        if let Some(arr) = config.get("exclude").and_then(|v| v.as_array()) {
            self.exclude = arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let result = match self.operation.as_str() {
            "pivot" => self.execute_pivot(&input)?,
            "unpivot" => self.execute_unpivot(&input)?,
            "transpose" => self.execute_transpose(&input)?,
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

impl PivotNode {
    fn execute_pivot(&self, input: &serde_json::Value) -> Result<serde_json::Value, NodeError> {
        let arr = input
            .as_array()
            .ok_or_else(|| NodeError::ExecutionFailed("pivot requires an array input".into()))?;

        if self.row_field.is_empty() || self.column_field.is_empty() || self.value_field.is_empty()
        {
            return Err(NodeError::ExecutionFailed(
                "pivot requires row_field, column_field, and value_field".into(),
            ));
        }

        // Collect unique row keys and column keys preserving order
        let mut row_keys: Vec<String> = Vec::new();
        let mut col_keys: Vec<String> = Vec::new();
        // Map: row_key -> (col_key -> value)
        let mut pivot_data: std::collections::HashMap<
            String,
            std::collections::HashMap<String, serde_json::Value>,
        > = std::collections::HashMap::new();

        for item in arr {
            let row_val = item
                .get(&self.row_field)
                .map(value_to_string)
                .unwrap_or_default();
            let col_val = item
                .get(&self.column_field)
                .map(value_to_string)
                .unwrap_or_default();
            let value = item
                .get(&self.value_field)
                .cloned()
                .unwrap_or(serde_json::Value::Null);

            if !row_keys.contains(&row_val) {
                row_keys.push(row_val.clone());
            }
            if !col_keys.contains(&col_val) {
                col_keys.push(col_val.clone());
            }

            pivot_data
                .entry(row_val)
                .or_default()
                .insert(col_val, value);
        }

        // Build result rows
        let rows: Vec<serde_json::Value> = row_keys
            .iter()
            .map(|row_key| {
                let mut obj = serde_json::Map::new();
                obj.insert(
                    self.row_field.clone(),
                    serde_json::Value::String(row_key.clone()),
                );
                let row_data = pivot_data.get(row_key);
                for col_key in &col_keys {
                    let val = row_data
                        .and_then(|rd| rd.get(col_key))
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    obj.insert(col_key.clone(), val);
                }
                serde_json::Value::Object(obj)
            })
            .collect();

        Ok(serde_json::json!({
            "rows": rows,
            "columns": col_keys,
            "row_count": row_keys.len(),
        }))
    }

    fn execute_unpivot(&self, input: &serde_json::Value) -> Result<serde_json::Value, NodeError> {
        let obj = input
            .as_object()
            .ok_or_else(|| NodeError::ExecutionFailed("unpivot requires an object input".into()))?;

        let mut rows: Vec<serde_json::Value> = Vec::new();
        for (key, value) in obj {
            if self.exclude.contains(key) {
                continue;
            }
            let mut row = serde_json::Map::new();
            row.insert(
                self.key_name.clone(),
                serde_json::Value::String(key.clone()),
            );
            row.insert(self.value_name.clone(), value.clone());
            rows.push(serde_json::Value::Object(row));
        }

        Ok(serde_json::json!({
            "rows": rows,
            "count": rows.len(),
        }))
    }

    fn execute_transpose(&self, input: &serde_json::Value) -> Result<serde_json::Value, NodeError> {
        let arr = input.as_array().ok_or_else(|| {
            NodeError::ExecutionFailed("transpose requires a 2D array input".into())
        })?;

        if arr.is_empty() {
            return Ok(serde_json::json!([]));
        }

        // Determine max column count
        let max_cols = arr
            .iter()
            .filter_map(|row| row.as_array().map(|a| a.len()))
            .max()
            .unwrap_or(0);

        let num_rows = arr.len();
        let mut transposed: Vec<Vec<serde_json::Value>> = vec![Vec::new(); max_cols];

        for row in arr {
            if let Some(row_arr) = row.as_array() {
                for (col_idx, val) in row_arr.iter().enumerate() {
                    if col_idx < max_cols {
                        transposed[col_idx].push(val.clone());
                    }
                }
                // Pad with null if row is shorter
                for item in transposed.iter_mut().take(max_cols).skip(row_arr.len()) {
                    item.push(serde_json::Value::Null);
                }
            } else {
                for item in transposed.iter_mut().take(max_cols) {
                    item.push(serde_json::Value::Null);
                }
            }
        }

        let result: Vec<serde_json::Value> = transposed
            .into_iter()
            .map(serde_json::Value::Array)
            .collect();

        Ok(serde_json::json!({
            "rows": result,
            "original_rows": num_rows,
            "original_cols": max_cols,
        }))
    }
}

fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
        other => other.to_string(),
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
    async fn test_pivot_basic() {
        let mut node = PivotNode::new();
        node.init(&json!({
            "operation": "pivot",
            "row_field": "region",
            "column_field": "product",
            "value_field": "amount"
        }))
        .await
        .unwrap();

        let input = json!([
            {"region": "US", "product": "A", "amount": 100},
            {"region": "US", "product": "B", "amount": 200},
            {"region": "EU", "product": "A", "amount": 150}
        ]);

        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["row_count"], 2);
        let rows = data["rows"].as_array().unwrap();
        // Find US row
        let us_row = rows.iter().find(|r| r["region"] == "US").unwrap();
        assert_eq!(us_row["A"], 100);
        assert_eq!(us_row["B"], 200);
    }

    #[tokio::test]
    async fn test_unpivot_basic() {
        let mut node = PivotNode::new();
        node.init(&json!({"operation": "unpivot"})).await.unwrap();

        let input = json!({"name": "Alice", "age": 30, "city": "NYC"});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["count"], 3);
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 3);
    }

    #[tokio::test]
    async fn test_unpivot_with_exclude() {
        let mut node = PivotNode::new();
        node.init(&json!({"operation": "unpivot", "exclude": ["id"]}))
            .await
            .unwrap();

        let input = json!({"id": 1, "name": "Alice", "age": 30});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["count"], 2);
    }

    #[tokio::test]
    async fn test_transpose_basic() {
        let mut node = PivotNode::new();
        node.init(&json!({"operation": "transpose"})).await.unwrap();

        let input = json!([[1, 2, 3], [4, 5, 6]]);
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], json!([1, 4]));
        assert_eq!(rows[1], json!([2, 5]));
        assert_eq!(rows[2], json!([3, 6]));
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let mut node = PivotNode::new();
        let result = node.init(&json!({"operation": "invalid"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_missing_operation() {
        let mut node = PivotNode::new();
        let result = node.init(&json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_unpivot_custom_names() {
        let mut node = PivotNode::new();
        node.init(&json!({"operation": "unpivot", "key_name": "field", "value_name": "val"}))
            .await
            .unwrap();

        let input = json!({"x": 1, "y": 2});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        let rows = data["rows"].as_array().unwrap();
        // Check that custom key/value names are used
        assert!(rows[0].get("field").is_some());
        assert!(rows[0].get("val").is_some());
    }
}
