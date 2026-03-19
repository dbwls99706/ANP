use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// LookupNode — lookup/join between datasets
pub struct LookupNode {
    lookup_data: Vec<serde_json::Value>,
    key_field: String,
    lookup_key: String,
    result_fields: Option<Vec<String>>,
    mode: String,
}

impl Default for LookupNode {
    fn default() -> Self {
        Self::new()
    }
}

impl LookupNode {
    pub fn new() -> Self {
        Self {
            lookup_data: Vec::new(),
            key_field: String::new(),
            lookup_key: String::new(),
            result_fields: None,
            mode: "first".to_string(),
        }
    }
}

fn values_equal(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    // Compare with type coercion for numbers/strings
    match (a, b) {
        (serde_json::Value::String(sa), serde_json::Value::Number(nb)) => sa == &nb.to_string(),
        (serde_json::Value::Number(na), serde_json::Value::String(sb)) => &na.to_string() == sb,
        _ => a == b,
    }
}

fn extract_fields(
    entry: &serde_json::Value,
    fields: &Option<Vec<String>>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut result = serde_json::Map::new();
    if let Some(obj) = entry.as_object() {
        match fields {
            Some(field_list) => {
                for field in field_list {
                    if let Some(val) = obj.get(field) {
                        result.insert(field.clone(), val.clone());
                    }
                }
            }
            None => {
                for (key, val) in obj {
                    result.insert(key.clone(), val.clone());
                }
            }
        }
    }
    result
}

#[async_trait::async_trait]
impl NodeExecutor for LookupNode {
    fn node_type(&self) -> &str {
        "lookup"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        self.key_field = config
            .get("key_field")
            .and_then(|v| v.as_str())
            .ok_or_else(|| NodeError::InvalidConfig("'key_field' is required".into()))?
            .to_string();

        self.lookup_key = config
            .get("lookup_key")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.key_field)
            .to_string();

        self.lookup_data = config
            .get("lookup_data")
            .and_then(|v| v.as_array())
            .ok_or_else(|| NodeError::InvalidConfig("'lookup_data' array is required".into()))?
            .clone();

        if let Some(rf) = config.get("result_fields").and_then(|v| v.as_array()) {
            self.result_fields = Some(
                rf.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect(),
            );
        }

        if let Some(m) = config.get("mode").and_then(|v| v.as_str()) {
            match m {
                "first" | "all" => self.mode = m.to_string(),
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown mode '{}'. Use: first, all",
                        other
                    )));
                }
            }
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let input_key_value = input.get(&self.key_field).ok_or_else(|| {
            NodeError::ExecutionFailed(format!("input missing key_field '{}'", self.key_field))
        })?;

        // Find matching entries
        let matches: Vec<&serde_json::Value> = self
            .lookup_data
            .iter()
            .filter(|entry| {
                entry
                    .get(&self.lookup_key)
                    .map(|v| values_equal(v, input_key_value))
                    .unwrap_or(false)
            })
            .collect();

        let mut result = input.as_object().cloned().unwrap_or_default();

        if matches.is_empty() {
            result.insert("_lookup_found".to_string(), serde_json::Value::Bool(false));
        } else {
            result.insert("_lookup_found".to_string(), serde_json::Value::Bool(true));

            match self.mode.as_str() {
                "first" => {
                    let fields = extract_fields(matches[0], &self.result_fields);
                    for (key, val) in fields {
                        result.insert(key, val);
                    }
                }
                "all" => {
                    let results: Vec<serde_json::Value> = matches
                        .iter()
                        .map(|entry| {
                            let fields = extract_fields(entry, &self.result_fields);
                            serde_json::Value::Object(fields)
                        })
                        .collect();
                    result.insert(
                        "_lookup_results".to_string(),
                        serde_json::Value::Array(results),
                    );
                }
                _ => {}
            }
        }

        Ok(NodeOutput::data(serde_json::Value::Object(result)))
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
    async fn test_lookup_first_match() {
        let mut node = LookupNode::new();
        node.init(&json!({
            "key_field": "country_code",
            "lookup_key": "code",
            "lookup_data": [
                {"code": "US", "name": "United States"},
                {"code": "GB", "name": "United Kingdom"}
            ]
        }))
        .await
        .unwrap();

        let input = json!({"country_code": "US", "city": "NYC"});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["_lookup_found"], true);
        assert_eq!(data["name"], "United States");
        assert_eq!(data["city"], "NYC");
    }

    #[tokio::test]
    async fn test_lookup_not_found() {
        let mut node = LookupNode::new();
        node.init(&json!({
            "key_field": "id",
            "lookup_data": [
                {"id": 1, "name": "Alice"},
                {"id": 2, "name": "Bob"}
            ]
        }))
        .await
        .unwrap();

        let input = json!({"id": 99});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["_lookup_found"], false);
    }

    #[tokio::test]
    async fn test_lookup_all_mode() {
        let mut node = LookupNode::new();
        node.init(&json!({
            "key_field": "dept",
            "lookup_key": "department",
            "mode": "all",
            "lookup_data": [
                {"department": "eng", "employee": "Alice"},
                {"department": "eng", "employee": "Bob"},
                {"department": "hr", "employee": "Carol"}
            ]
        }))
        .await
        .unwrap();

        let input = json!({"dept": "eng"});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["_lookup_found"], true);
        let results = data["_lookup_results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_lookup_result_fields() {
        let mut node = LookupNode::new();
        node.init(&json!({
            "key_field": "id",
            "lookup_data": [
                {"id": 1, "name": "Alice", "email": "alice@test.com", "secret": "xyz"}
            ],
            "result_fields": ["name", "email"]
        }))
        .await
        .unwrap();

        let input = json!({"id": 1});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["name"], "Alice");
        assert_eq!(data["email"], "alice@test.com");
        assert!(data.get("secret").is_none());
    }

    #[tokio::test]
    async fn test_lookup_missing_key_field() {
        let mut node = LookupNode::new();
        let result = node.init(&json!({"lookup_data": []})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_lookup_type_coercion() {
        let mut node = LookupNode::new();
        node.init(&json!({
            "key_field": "id",
            "lookup_data": [
                {"id": "42", "name": "Answer"}
            ]
        }))
        .await
        .unwrap();

        // Input has numeric 42, lookup has string "42"
        let input = json!({"id": 42});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["_lookup_found"], true);
        assert_eq!(data["name"], "Answer");
    }
}
