use std::collections::HashMap;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Conditional node — routes data to different output ports based on conditions.
///
/// Config example:
/// ```json
/// {
///   "field": "status",
///   "branches": [
///     { "op": "eq", "value": "success", "port": "success" },
///     { "op": "eq", "value": "error", "port": "error" }
///   ],
///   "default_port": "other"
/// }
/// ```
///
/// The node evaluates branches in order. The first matching branch routes
/// the input to that branch's port. If no branch matches, the input goes
/// to `default_port` (defaults to "default").
///
/// Supported operators: eq, neq, gt, gte, lt, lte, contains, exists
pub struct ConditionalNode {
    field: String,
    branches: Vec<Branch>,
    default_port: String,
}

#[derive(Debug, Clone)]
struct Branch {
    op: CondOp,
    value: serde_json::Value,
    port: String,
}

#[derive(Debug, Clone)]
enum CondOp {
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
    Contains,
    Exists,
}

impl Default for ConditionalNode {
    fn default() -> Self {
        Self::new()
    }
}

impl ConditionalNode {
    pub fn new() -> Self {
        Self {
            field: String::new(),
            branches: Vec::new(),
            default_port: "default".to_string(),
        }
    }
}

fn parse_op(s: &str) -> Result<CondOp, NodeError> {
    match s {
        "eq" => Ok(CondOp::Eq),
        "neq" => Ok(CondOp::Neq),
        "gt" => Ok(CondOp::Gt),
        "gte" => Ok(CondOp::Gte),
        "lt" => Ok(CondOp::Lt),
        "lte" => Ok(CondOp::Lte),
        "contains" => Ok(CondOp::Contains),
        "exists" => Ok(CondOp::Exists),
        other => Err(NodeError::InvalidConfig(format!(
            "unknown conditional operator: '{}'. Use: eq, neq, gt, gte, lt, lte, contains, exists",
            other
        ))),
    }
}

fn resolve_field(value: &serde_json::Value, field: &str) -> Option<serde_json::Value> {
    let mut current = value;
    for part in field.split('.') {
        current = current.get(part)?;
    }
    Some(current.clone())
}

fn evaluate(actual: &serde_json::Value, op: &CondOp, expected: &serde_json::Value) -> bool {
    match op {
        CondOp::Eq => actual == expected,
        CondOp::Neq => actual != expected,
        CondOp::Gt => compare_numeric(actual, expected, |a, b| a > b),
        CondOp::Gte => compare_numeric(actual, expected, |a, b| a >= b),
        CondOp::Lt => compare_numeric(actual, expected, |a, b| a < b),
        CondOp::Lte => compare_numeric(actual, expected, |a, b| a <= b),
        CondOp::Contains => {
            if let (Some(haystack), Some(needle)) = (actual.as_str(), expected.as_str()) {
                haystack.contains(needle)
            } else if let Some(arr) = actual.as_array() {
                arr.contains(expected)
            } else {
                false
            }
        }
        CondOp::Exists => !actual.is_null(),
    }
}

fn compare_numeric(a: &serde_json::Value, b: &serde_json::Value, f: fn(f64, f64) -> bool) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(av), Some(bv)) if !av.is_nan() && !bv.is_nan() => f(av, bv),
        _ => false,
    }
}

#[async_trait::async_trait]
impl NodeExecutor for ConditionalNode {
    fn node_type(&self) -> &str {
        "conditional"
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

        let branches = config
            .get("branches")
            .and_then(|v| v.as_array())
            .ok_or_else(|| NodeError::InvalidConfig("missing 'branches' array in config".into()))?;

        if branches.is_empty() {
            return Err(NodeError::InvalidConfig(
                "branches array must not be empty".into(),
            ));
        }

        self.branches = Vec::with_capacity(branches.len());
        for (i, branch) in branches.iter().enumerate() {
            let op_str = branch.get("op").and_then(|v| v.as_str()).ok_or_else(|| {
                NodeError::InvalidConfig(format!("branch {} missing 'op'", i))
            })?;
            let op = parse_op(op_str)?;

            let value = branch
                .get("value")
                .cloned()
                .unwrap_or(serde_json::Value::Null);

            let port = branch
                .get("port")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    NodeError::InvalidConfig(format!("branch {} missing 'port'", i))
                })?;
            if port.is_empty() {
                return Err(NodeError::InvalidConfig(format!(
                    "branch {} has empty 'port' name",
                    i
                )));
            }
            let port = port.to_string();

            self.branches.push(Branch { op, value, port });
        }

        if let Some(dp) = config.get("default_port").and_then(|v| v.as_str()) {
            self.default_port = dp.to_string();
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let field_value = resolve_field(&input, &self.field);

        // Evaluate branches in order
        for branch in &self.branches {
            let matches = match &field_value {
                Some(actual) => evaluate(actual, &branch.op, &branch.value),
                None => matches!(branch.op, CondOp::Exists) && branch.value == serde_json::json!(false),
            };

            if matches {
                let mut ports = HashMap::new();
                ports.insert(branch.port.clone(), input);
                return Ok(
                    NodeOutput::multi(ports)
                        .with_log(format!("routed to port '{}'", branch.port)),
                );
            }
        }

        // No branch matched — route to default
        let mut ports = HashMap::new();
        ports.insert(self.default_port.clone(), input);
        Ok(
            NodeOutput::multi(ports)
                .with_log(format!("no branch matched, routed to default port '{}'", self.default_port)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_route_to_matching_branch() {
        let mut node = ConditionalNode::new();
        node.init(&serde_json::json!({
            "field": "status",
            "branches": [
                { "op": "eq", "value": "ok", "port": "success" },
                { "op": "eq", "value": "fail", "port": "failure" }
            ]
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"status": "ok", "data": 42}), &ctx)
            .await
            .unwrap();

        assert!(output.ports.contains_key("success"));
        assert_eq!(output.ports["success"]["data"], 42);
    }

    #[tokio::test]
    async fn test_route_to_default() {
        let mut node = ConditionalNode::new();
        node.init(&serde_json::json!({
            "field": "status",
            "branches": [
                { "op": "eq", "value": "ok", "port": "success" }
            ],
            "default_port": "other"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"status": "unknown"}), &ctx)
            .await
            .unwrap();

        assert!(output.ports.contains_key("other"));
    }

    #[tokio::test]
    async fn test_numeric_comparison() {
        let mut node = ConditionalNode::new();
        node.init(&serde_json::json!({
            "field": "score",
            "branches": [
                { "op": "gte", "value": 90, "port": "high" },
                { "op": "gte", "value": 50, "port": "medium" }
            ],
            "default_port": "low"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();

        // High score
        let out = node.execute(serde_json::json!({"score": 95}), &ctx).await.unwrap();
        assert!(out.ports.contains_key("high"));

        // Medium score
        let out = node.execute(serde_json::json!({"score": 75}), &ctx).await.unwrap();
        assert!(out.ports.contains_key("medium"));

        // Low score
        let out = node.execute(serde_json::json!({"score": 30}), &ctx).await.unwrap();
        assert!(out.ports.contains_key("low"));
    }

    #[tokio::test]
    async fn test_init_missing_field() {
        let mut node = ConditionalNode::new();
        let result = node.init(&serde_json::json!({"branches": []})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_init_empty_branches() {
        let mut node = ConditionalNode::new();
        let result = node
            .init(&serde_json::json!({"field": "x", "branches": []}))
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_evaluate_contains() {
        assert!(evaluate(
            &serde_json::json!("hello world"),
            &CondOp::Contains,
            &serde_json::json!("world")
        ));
        assert!(!evaluate(
            &serde_json::json!("hello"),
            &CondOp::Contains,
            &serde_json::json!("world")
        ));
    }

    #[test]
    fn test_evaluate_exists() {
        assert!(evaluate(
            &serde_json::json!("anything"),
            &CondOp::Exists,
            &serde_json::Value::Null
        ));
        assert!(!evaluate(
            &serde_json::Value::Null,
            &CondOp::Exists,
            &serde_json::Value::Null
        ));
    }

    #[tokio::test]
    async fn test_init_empty_port_name() {
        let mut node = ConditionalNode::new();
        let result = node
            .init(&serde_json::json!({
                "field": "x",
                "branches": [{ "op": "eq", "value": 1, "port": "" }]
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_missing_field_routes_to_default() {
        let mut node = ConditionalNode::new();
        node.init(&serde_json::json!({
            "field": "nonexistent",
            "branches": [
                { "op": "eq", "value": "x", "port": "match" }
            ],
            "default_port": "fallback"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"other": "data"}), &ctx)
            .await
            .unwrap();
        assert!(output.ports.contains_key("fallback"));
    }

    #[test]
    fn test_type_mismatch_returns_false() {
        // String vs number comparison should safely return false
        assert!(!compare_numeric(
            &serde_json::json!("not a number"),
            &serde_json::json!(42),
            |a, b| a > b
        ));
    }
}
