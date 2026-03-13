use std::collections::HashMap;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult, NodeStatus};
use anp_core::runtime::context::ExecutionContext;

/// Transform node — maps input fields using dot-path notation
///
/// Config example:
/// ```json
/// {
///   "type": "map",
///   "mapping": {
///     "name": "user.name",
///     "age": "user.age"
///   }
/// }
/// ```
pub struct TransformNode {
    mapping: HashMap<String, String>,
}

impl Default for TransformNode {
    fn default() -> Self {
        Self::new()
    }
}

impl TransformNode {
    pub fn new() -> Self {
        Self {
            mapping: HashMap::new(),
        }
    }
}

/// Resolve a dot-path like "user.name" on a JSON value
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

#[async_trait::async_trait]
impl NodeExecutor for TransformNode {
    fn node_type(&self) -> &str {
        "transform"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        let mapping = config
            .get("mapping")
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                NodeError::InvalidConfig("missing 'mapping' object in config".into())
            })?;

        for (key, value) in mapping {
            let path = value
                .as_str()
                .ok_or_else(|| {
                    NodeError::InvalidConfig(format!("mapping value for '{}' must be a string", key))
                })?;
            self.mapping.insert(key.clone(), path.to_string());
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        // If input is a JSON string, try to parse it first
        let input = if let Some(s) = input.as_str() {
            serde_json::from_str(s).unwrap_or(input)
        } else {
            input
        };

        // If input is an array, transform each element
        if let serde_json::Value::Array(items) = &input {
            let results: Vec<serde_json::Value> = items
                .iter()
                .map(|item| transform_single(item, &self.mapping))
                .collect();
            return Ok(NodeOutput::data(serde_json::Value::Array(results)));
        }

        // Single object transform
        let result = transform_single(&input, &self.mapping);

        let mut ports = HashMap::new();
        ports.insert("default".to_string(), result.clone());

        Ok(NodeOutput {
            ports,
            status: NodeStatus::Ok,
            logs: vec![format!("transformed with {} mappings", self.mapping.len())],
            error: None,
        })
    }
}

fn transform_single(
    input: &serde_json::Value,
    mapping: &HashMap<String, String>,
) -> serde_json::Value {
    let mut result = serde_json::Map::new();

    for (output_key, input_path) in mapping {
        if let Some(value) = resolve_dot_path(input, input_path) {
            result.insert(output_key.clone(), value);
        }
    }

    serde_json::Value::Object(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_dot_path() {
        let data = serde_json::json!({
            "user": {
                "name": "Alice",
                "address": {
                    "city": "Seoul"
                }
            }
        });

        assert_eq!(
            resolve_dot_path(&data, "user.name"),
            Some(serde_json::json!("Alice"))
        );
        assert_eq!(
            resolve_dot_path(&data, "user.address.city"),
            Some(serde_json::json!("Seoul"))
        );
        assert_eq!(resolve_dot_path(&data, "user.missing"), None);
    }

    #[test]
    fn test_transform_single() {
        let input = serde_json::json!({
            "user": { "name": "Bob", "age": 30 }
        });

        let mut mapping = HashMap::new();
        mapping.insert("name".to_string(), "user.name".to_string());
        mapping.insert("age".to_string(), "user.age".to_string());

        let result = transform_single(&input, &mapping);
        assert_eq!(result["name"], "Bob");
        assert_eq!(result["age"], 30);
    }
}
