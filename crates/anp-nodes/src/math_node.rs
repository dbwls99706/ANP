use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// MathNode — arithmetic and aggregation operations
pub struct MathNode {
    operation: String,
    value: Option<f64>,
}

impl Default for MathNode {
    fn default() -> Self {
        Self::new()
    }
}

impl MathNode {
    pub fn new() -> Self {
        Self {
            operation: "add".to_string(),
            value: None,
        }
    }
}

fn extract_number(input: &serde_json::Value) -> Option<f64> {
    if let Some(n) = input.as_f64() {
        Some(n)
    } else if let Some(n) = input.get("value").and_then(|v| v.as_f64()) {
        Some(n)
    } else {
        None
    }
}

fn extract_array(input: &serde_json::Value) -> Option<Vec<f64>> {
    if let Some(arr) = input.as_array() {
        let nums: Vec<f64> = arr.iter().filter_map(|v| v.as_f64()).collect();
        if nums.is_empty() {
            None
        } else {
            Some(nums)
        }
    } else {
        None
    }
}

#[async_trait::async_trait]
impl NodeExecutor for MathNode {
    fn node_type(&self) -> &str {
        "math"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        self.operation = config
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| NodeError::InvalidConfig("missing 'operation' in config".into()))?
            .to_string();

        let valid_ops = [
            "add", "subtract", "multiply", "divide", "modulo", "power", "abs", "floor",
            "ceil", "round", "min", "max", "sum", "avg", "count", "sqrt", "log",
        ];
        if !valid_ops.contains(&self.operation.as_str()) {
            return Err(NodeError::InvalidConfig(format!(
                "unknown operation '{}'. Valid: {}",
                self.operation,
                valid_ops.join(", ")
            )));
        }

        self.value = config.get("value").and_then(|v| v.as_f64());

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        // Aggregation operations work on arrays
        match self.operation.as_str() {
            "sum" | "avg" | "count" | "min" | "max" => {
                let nums = extract_array(&input).ok_or_else(|| {
                    NodeError::ExecutionFailed(format!(
                        "'{}' operation requires an array of numbers",
                        self.operation
                    ))
                })?;

                let result = match self.operation.as_str() {
                    "sum" => nums.iter().sum::<f64>(),
                    "avg" => nums.iter().sum::<f64>() / nums.len() as f64,
                    "count" => nums.len() as f64,
                    "min" => nums
                        .iter()
                        .copied()
                        .fold(f64::INFINITY, f64::min),
                    "max" => nums
                        .iter()
                        .copied()
                        .fold(f64::NEG_INFINITY, f64::max),
                    _ => unreachable!(),
                };

                return Ok(NodeOutput::data(serde_json::json!(result)));
            }
            _ => {}
        }

        // Unary operations
        match self.operation.as_str() {
            "abs" | "floor" | "ceil" | "round" | "sqrt" | "log" => {
                let num = extract_number(&input).ok_or_else(|| {
                    NodeError::ExecutionFailed("input must be a number or { \"value\": number }".into())
                })?;

                let result = match self.operation.as_str() {
                    "abs" => num.abs(),
                    "floor" => num.floor(),
                    "ceil" => num.ceil(),
                    "round" => num.round(),
                    "sqrt" => {
                        if num < 0.0 {
                            return Err(NodeError::ExecutionFailed(
                                "cannot take sqrt of negative number".into(),
                            ));
                        }
                        num.sqrt()
                    }
                    "log" => {
                        if num <= 0.0 {
                            return Err(NodeError::ExecutionFailed(
                                "cannot take log of non-positive number".into(),
                            ));
                        }
                        num.ln()
                    }
                    _ => unreachable!(),
                };

                return Ok(NodeOutput::data(serde_json::json!(result)));
            }
            _ => {}
        }

        // Binary operations (require config.value)
        let num = extract_number(&input).ok_or_else(|| {
            NodeError::ExecutionFailed("input must be a number or { \"value\": number }".into())
        })?;

        let operand = self.value.ok_or_else(|| {
            NodeError::ExecutionFailed(format!(
                "'{}' operation requires 'value' in config",
                self.operation
            ))
        })?;

        let result = match self.operation.as_str() {
            "add" => num + operand,
            "subtract" => num - operand,
            "multiply" => num * operand,
            "divide" => {
                if operand == 0.0 {
                    return Err(NodeError::ExecutionFailed("division by zero".into()));
                }
                num / operand
            }
            "modulo" => {
                if operand == 0.0 {
                    return Err(NodeError::ExecutionFailed("modulo by zero".into()));
                }
                num % operand
            }
            "power" => num.powf(operand),
            _ => {
                return Err(NodeError::ExecutionFailed(format!(
                    "unknown operation: {}",
                    self.operation
                )));
            }
        };

        Ok(NodeOutput::data(serde_json::json!(result)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_add() {
        let mut node = MathNode::new();
        node.init(&serde_json::json!({"operation": "add", "value": 5.0}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::json!(10.0), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], serde_json::json!(15.0));
    }

    #[tokio::test]
    async fn test_subtract() {
        let mut node = MathNode::new();
        node.init(&serde_json::json!({"operation": "subtract", "value": 3.0}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::json!(10.0), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], serde_json::json!(7.0));
    }

    #[tokio::test]
    async fn test_multiply() {
        let mut node = MathNode::new();
        node.init(&serde_json::json!({"operation": "multiply", "value": 4.0}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::json!(3.0), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], serde_json::json!(12.0));
    }

    #[tokio::test]
    async fn test_divide() {
        let mut node = MathNode::new();
        node.init(&serde_json::json!({"operation": "divide", "value": 4.0}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::json!(20.0), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], serde_json::json!(5.0));
    }

    #[tokio::test]
    async fn test_divide_by_zero() {
        let mut node = MathNode::new();
        node.init(&serde_json::json!({"operation": "divide", "value": 0.0}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let result = node.execute(serde_json::json!(10.0), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_modulo() {
        let mut node = MathNode::new();
        node.init(&serde_json::json!({"operation": "modulo", "value": 3.0}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::json!(10.0), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], serde_json::json!(1.0));
    }

    #[tokio::test]
    async fn test_abs() {
        let mut node = MathNode::new();
        node.init(&serde_json::json!({"operation": "abs"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::json!(-7.5), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], serde_json::json!(7.5));
    }

    #[tokio::test]
    async fn test_floor() {
        let mut node = MathNode::new();
        node.init(&serde_json::json!({"operation": "floor"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::json!(3.7), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], serde_json::json!(3.0));
    }

    #[tokio::test]
    async fn test_ceil() {
        let mut node = MathNode::new();
        node.init(&serde_json::json!({"operation": "ceil"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::json!(3.2), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], serde_json::json!(4.0));
    }

    #[tokio::test]
    async fn test_round() {
        let mut node = MathNode::new();
        node.init(&serde_json::json!({"operation": "round"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::json!(3.5), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], serde_json::json!(4.0));
    }

    #[tokio::test]
    async fn test_sum() {
        let mut node = MathNode::new();
        node.init(&serde_json::json!({"operation": "sum"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!([1.0, 2.0, 3.0, 4.0]), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"], serde_json::json!(10.0));
    }

    #[tokio::test]
    async fn test_avg() {
        let mut node = MathNode::new();
        node.init(&serde_json::json!({"operation": "avg"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!([2.0, 4.0, 6.0]), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"], serde_json::json!(4.0));
    }

    #[tokio::test]
    async fn test_sqrt() {
        let mut node = MathNode::new();
        node.init(&serde_json::json!({"operation": "sqrt"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::json!(25.0), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], serde_json::json!(5.0));
    }

    #[tokio::test]
    async fn test_sqrt_negative() {
        let mut node = MathNode::new();
        node.init(&serde_json::json!({"operation": "sqrt"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let result = node.execute(serde_json::json!(-4.0), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_input_as_object_with_value() {
        let mut node = MathNode::new();
        node.init(&serde_json::json!({"operation": "abs"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"value": -10.0}), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"], serde_json::json!(10.0));
    }
}
