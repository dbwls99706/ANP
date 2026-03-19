use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Filter node — filters data based on conditions
///
/// Config example:
/// ```json
/// {
///   "field": "age",
///   "op": "gt",
///   "value": 18
/// }
/// ```
///
/// Supported operators: eq, neq, gt, gte, lt, lte, contains
pub struct FilterNode {
    field: String,
    op: FilterOp,
    value: serde_json::Value,
}

#[derive(Debug, Clone)]
enum FilterOp {
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
    Contains,
}

impl Default for FilterNode {
    fn default() -> Self {
        Self::new()
    }
}

impl FilterNode {
    pub fn new() -> Self {
        Self {
            field: String::new(),
            op: FilterOp::Eq,
            value: serde_json::Value::Null,
        }
    }
}

fn parse_op(s: &str) -> Result<FilterOp, NodeError> {
    match s {
        "eq" => Ok(FilterOp::Eq),
        "neq" => Ok(FilterOp::Neq),
        "gt" => Ok(FilterOp::Gt),
        "gte" => Ok(FilterOp::Gte),
        "lt" => Ok(FilterOp::Lt),
        "lte" => Ok(FilterOp::Lte),
        "contains" => Ok(FilterOp::Contains),
        other => Err(NodeError::InvalidConfig(format!(
            "unknown filter operator: '{}'. Use: eq, neq, gt, gte, lt, lte, contains",
            other
        ))),
    }
}

fn compare_values(actual: &serde_json::Value, op: &FilterOp, expected: &serde_json::Value) -> bool {
    match op {
        FilterOp::Eq => actual == expected,
        FilterOp::Neq => actual != expected,
        FilterOp::Gt => compare_numeric(actual, expected, |a, b| a > b),
        FilterOp::Gte => compare_numeric(actual, expected, |a, b| a >= b),
        FilterOp::Lt => compare_numeric(actual, expected, |a, b| a < b),
        FilterOp::Lte => compare_numeric(actual, expected, |a, b| a <= b),
        FilterOp::Contains => {
            if let (Some(haystack), Some(needle)) = (actual.as_str(), expected.as_str()) {
                haystack.contains(needle)
            } else if let Some(arr) = actual.as_array() {
                arr.contains(expected)
            } else {
                false
            }
        }
    }
}

fn compare_numeric(a: &serde_json::Value, b: &serde_json::Value, f: fn(f64, f64) -> bool) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(av), Some(bv)) => f(av, bv),
        _ => false,
    }
}

/// Resolve a dot-path on a JSON value
fn resolve_field(value: &serde_json::Value, field: &str) -> Option<serde_json::Value> {
    let parts: Vec<&str> = field.split('.').collect();
    let mut current = value;
    for part in parts {
        current = current.get(part)?;
    }
    Some(current.clone())
}

#[async_trait::async_trait]
impl NodeExecutor for FilterNode {
    fn node_type(&self) -> &str {
        "filter"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        self.field = config
            .get("field")
            .and_then(|v| v.as_str())
            .ok_or_else(|| NodeError::InvalidConfig("missing 'field' in config".into()))?
            .to_string();

        let op_str = config
            .get("op")
            .and_then(|v| v.as_str())
            .ok_or_else(|| NodeError::InvalidConfig("missing 'op' in config".into()))?;
        self.op = parse_op(op_str)?;

        self.value = config
            .get("value")
            .cloned()
            .ok_or_else(|| NodeError::InvalidConfig("missing 'value' in config".into()))?;

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

        // If input is an array, filter each element
        if let serde_json::Value::Array(items) = &input {
            let filtered: Vec<serde_json::Value> = items
                .iter()
                .filter(|item| {
                    resolve_field(item, &self.field)
                        .map(|v| compare_values(&v, &self.op, &self.value))
                        .unwrap_or(false)
                })
                .cloned()
                .collect();

            let count = filtered.len();
            return Ok(
                NodeOutput::data(serde_json::Value::Array(filtered)).with_log(format!(
                    "filtered {} -> {} items (field={}, op={:?})",
                    items.len(),
                    count,
                    self.field,
                    self.op
                )),
            );
        }

        // Single value: pass or skip
        let passes = resolve_field(&input, &self.field)
            .map(|v| compare_values(&v, &self.op, &self.value))
            .unwrap_or(false);

        if passes {
            Ok(NodeOutput::data(input))
        } else {
            Ok(NodeOutput::skip())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_eq() {
        assert!(compare_values(
            &serde_json::json!(42),
            &FilterOp::Eq,
            &serde_json::json!(42)
        ));
        assert!(!compare_values(
            &serde_json::json!(42),
            &FilterOp::Eq,
            &serde_json::json!(43)
        ));
    }

    #[test]
    fn test_compare_gt() {
        assert!(compare_values(
            &serde_json::json!(10),
            &FilterOp::Gt,
            &serde_json::json!(5)
        ));
        assert!(!compare_values(
            &serde_json::json!(5),
            &FilterOp::Gt,
            &serde_json::json!(10)
        ));
    }

    #[test]
    fn test_contains_string() {
        assert!(compare_values(
            &serde_json::json!("hello world"),
            &FilterOp::Contains,
            &serde_json::json!("world")
        ));
    }
}
