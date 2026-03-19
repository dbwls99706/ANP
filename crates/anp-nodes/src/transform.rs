use std::collections::HashMap;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Transform node — maps input fields using dot-path notation or literal values
///
/// Config example:
/// ```json
/// {
///   "type": "map",
///   "mapping": {
///     "name": "user.name",
///     "age": "user.age",
///     "greeting": "'Hello World'",
///     "count": "42",
///     "active": "true"
///   }
/// }
/// ```
///
/// Mapping values can be:
/// - Dot-path: `"user.name"` → resolves from input
/// - Literal string: `"'some text'"` → single-quoted literal
/// - Literal number: `"42"`, `"3.14"` → parsed as number
/// - Literal boolean: `"true"`, `"false"` → parsed as boolean
/// - Literal null: `"null"` → JSON null
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
        let (result, missing) = transform_single_with_report(&input, &self.mapping);

        let mut output = NodeOutput::data(result);
        output.logs.push(format!("transformed with {} mappings", self.mapping.len()));
        if !missing.is_empty() {
            output.logs.push(format!("unresolved paths: {}", missing.join(", ")));
        }
        Ok(output)
    }
}

fn transform_single(
    input: &serde_json::Value,
    mapping: &HashMap<String, String>,
) -> serde_json::Value {
    transform_single_with_report(input, mapping).0
}

/// Try to parse a mapping value as a literal.
/// Returns Some(value) if it's a literal, None if it should be treated as a dot-path.
fn parse_literal(s: &str) -> Option<serde_json::Value> {
    // Single-quoted string literal: 'hello world'
    if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 {
        return Some(serde_json::Value::String(s[1..s.len() - 1].to_string()));
    }

    // Boolean
    if s == "true" {
        return Some(serde_json::Value::Bool(true));
    }
    if s == "false" {
        return Some(serde_json::Value::Bool(false));
    }

    // Null
    if s == "null" {
        return Some(serde_json::Value::Null);
    }

    // Number (integer or float)
    if let Ok(n) = s.parse::<i64>() {
        return Some(serde_json::json!(n));
    }
    if let Ok(n) = s.parse::<f64>() {
        if n.is_finite() {
            return Some(serde_json::json!(n));
        }
    }

    None
}

fn transform_single_with_report(
    input: &serde_json::Value,
    mapping: &HashMap<String, String>,
) -> (serde_json::Value, Vec<String>) {
    let mut result = serde_json::Map::new();
    let mut missing = Vec::new();

    for (output_key, input_path) in mapping {
        // First check if this is a literal value
        if let Some(literal) = parse_literal(input_path) {
            result.insert(output_key.clone(), literal);
            continue;
        }

        // Otherwise resolve as dot-path
        match resolve_dot_path(input, input_path) {
            Some(value) => {
                result.insert(output_key.clone(), value);
            }
            None => {
                missing.push(input_path.clone());
            }
        }
    }

    (serde_json::Value::Object(result), missing)
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

    #[test]
    fn test_parse_literal_string() {
        assert_eq!(
            parse_literal("'Hello World'"),
            Some(serde_json::json!("Hello World"))
        );
        assert_eq!(
            parse_literal("'ANP — AI Native'"),
            Some(serde_json::json!("ANP — AI Native"))
        );
    }

    #[test]
    fn test_parse_literal_number() {
        assert_eq!(parse_literal("42"), Some(serde_json::json!(42)));
        assert_eq!(parse_literal("3.14"), Some(serde_json::json!(3.14)));
        assert_eq!(parse_literal("-1"), Some(serde_json::json!(-1)));
    }

    #[test]
    fn test_parse_literal_boolean_null() {
        assert_eq!(parse_literal("true"), Some(serde_json::json!(true)));
        assert_eq!(parse_literal("false"), Some(serde_json::json!(false)));
        assert_eq!(parse_literal("null"), Some(serde_json::Value::Null));
    }

    #[test]
    fn test_parse_literal_not_literal() {
        // Dot paths should NOT be parsed as literals
        assert_eq!(parse_literal("user.name"), None);
        assert_eq!(parse_literal("data"), None);
    }

    #[test]
    fn test_transform_with_literals() {
        let input = serde_json::json!({"user": {"name": "Alice"}});

        let mut mapping = HashMap::new();
        mapping.insert("name".to_string(), "user.name".to_string());
        mapping.insert("title".to_string(), "'Hello World'".to_string());
        mapping.insert("count".to_string(), "42".to_string());
        mapping.insert("active".to_string(), "true".to_string());

        let result = transform_single(&input, &mapping);
        assert_eq!(result["name"], "Alice");
        assert_eq!(result["title"], "Hello World");
        assert_eq!(result["count"], 42);
        assert_eq!(result["active"], true);
    }

    #[test]
    fn test_transform_mixed_path_and_literal() {
        let input = serde_json::json!({});
        let mut mapping = HashMap::new();
        mapping.insert("greeting".to_string(), "'Welcome'".to_string());
        mapping.insert("missing".to_string(), "nonexistent.path".to_string());

        let (result, missing) = transform_single_with_report(&input, &mapping);
        assert_eq!(result["greeting"], "Welcome");
        assert_eq!(missing, vec!["nonexistent.path"]);
    }
}
