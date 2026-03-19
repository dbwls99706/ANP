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
