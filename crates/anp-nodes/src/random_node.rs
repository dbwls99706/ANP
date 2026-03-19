use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;
use std::time::SystemTime;

/// Random node — generates random values.
///
/// Config:
/// ```json
/// {
///   "type": "integer|float|boolean|string|choice",
///   "min": 0,
///   "max": 100,
///   "length": 10,
///   "choices": ["a", "b", "c"],
///   "field": "random_value"
/// }
/// ```
///
/// - `integer`: random int in [min, max]
/// - `float`: random float in [min, max]
/// - `boolean`: random true/false
/// - `string`: random alphanumeric string of given length
/// - `choice`: random pick from choices array
///
/// If input is a non-null object, the random value is added at the `field` key.
/// If input is null (source mode), produces `{ "<field>": <random_value> }`.
pub struct RandomNode {
    random_type: String,
    min: f64,
    max: f64,
    length: usize,
    choices: Vec<serde_json::Value>,
    field: String,
}

impl Default for RandomNode {
    fn default() -> Self {
        Self::new()
    }
}

impl RandomNode {
    pub fn new() -> Self {
        Self {
            random_type: "integer".to_string(),
            min: 0.0,
            max: 100.0,
            length: 10,
            choices: Vec::new(),
            field: "random_value".to_string(),
        }
    }
}

/// Simple pseudo-random number generator using xorshift64.
/// Seeded from SystemTime nanos for basic randomness.
fn random_u64() -> u64 {
    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    // xorshift64
    let mut x = seed;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x
}

/// Generate a random f64 in [0, 1)
fn random_f64() -> f64 {
    (random_u64() % 1_000_000_000) as f64 / 1_000_000_000.0
}

/// Generate a random integer in [min, max] inclusive
fn random_int(min: i64, max: i64) -> i64 {
    if min >= max {
        return min;
    }
    let range = (max - min + 1) as u64;
    let r = random_u64() % range;
    min + r as i64
}

/// Generate a random float in [min, max]
fn random_float(min: f64, max: f64) -> f64 {
    if min >= max {
        return min;
    }
    min + random_f64() * (max - min)
}

/// Generate a random alphanumeric string
fn random_string(length: usize) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut result = String::with_capacity(length);
    for i in 0..length {
        let seed = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let mut x = seed.wrapping_add(i as u64);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let idx = (x as usize) % CHARSET.len();
        result.push(CHARSET[idx] as char);
    }
    result
}

#[async_trait::async_trait]
impl NodeExecutor for RandomNode {
    fn node_type(&self) -> &str {
        "random"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(t) = config.get("type").and_then(|v| v.as_str()) {
            self.random_type = t.to_string();
        }
        if let Some(min) = config.get("min").and_then(|v| v.as_f64()) {
            self.min = min;
        }
        if let Some(max) = config.get("max").and_then(|v| v.as_f64()) {
            self.max = max;
        }
        if let Some(len) = config.get("length").and_then(|v| v.as_u64()) {
            self.length = len as usize;
        }
        if let Some(choices) = config.get("choices").and_then(|v| v.as_array()) {
            self.choices = choices.clone();
        }
        if let Some(field) = config.get("field").and_then(|v| v.as_str()) {
            self.field = field.to_string();
        }

        // Validate config
        match self.random_type.as_str() {
            "integer" | "float" | "boolean" | "string" => {}
            "choice" => {
                if self.choices.is_empty() {
                    return Err(NodeError::InvalidConfig(
                        "random type 'choice' requires non-empty 'choices' array".into(),
                    ));
                }
            }
            other => {
                return Err(NodeError::InvalidConfig(format!(
                    "unknown random type: '{}'. Use: integer, float, boolean, string, choice",
                    other
                )));
            }
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let random_value = match self.random_type.as_str() {
            "integer" => {
                let val = random_int(self.min as i64, self.max as i64);
                serde_json::json!(val)
            }
            "float" => {
                let val = random_float(self.min, self.max);
                serde_json::json!(val)
            }
            "boolean" => {
                let val = random_u64().is_multiple_of(2);
                serde_json::json!(val)
            }
            "string" => {
                let val = random_string(self.length);
                serde_json::json!(val)
            }
            "choice" => {
                if self.choices.is_empty() {
                    return Err(NodeError::ExecutionFailed("no choices available".into()));
                }
                let idx = (random_u64() as usize) % self.choices.len();
                self.choices[idx].clone()
            }
            _ => {
                return Err(NodeError::ExecutionFailed(format!(
                    "unknown random type: '{}'",
                    self.random_type
                )));
            }
        };

        if input.is_null() {
            // Source mode
            let result = serde_json::json!({ &self.field: random_value });
            Ok(NodeOutput::data(result))
        } else if let serde_json::Value::Object(mut map) = input {
            // Processor mode: add random value to existing object
            map.insert(self.field.clone(), random_value);
            Ok(NodeOutput::data(serde_json::Value::Object(map)))
        } else {
            // Non-object input: wrap
            let result = serde_json::json!({
                &self.field: random_value,
                "value": input
            });
            Ok(NodeOutput::data(result))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_random_integer() {
        let mut node = RandomNode::new();
        node.init(&serde_json::json!({"type": "integer", "min": 1, "max": 10}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        let val = output.ports["default"]["random_value"].as_i64().unwrap();
        assert!((1..=10).contains(&val));
    }

    #[tokio::test]
    async fn test_random_float() {
        let mut node = RandomNode::new();
        node.init(&serde_json::json!({"type": "float", "min": 0.0, "max": 1.0}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        let val = output.ports["default"]["random_value"].as_f64().unwrap();
        assert!((0.0..=1.0).contains(&val));
    }

    #[tokio::test]
    async fn test_random_boolean() {
        let mut node = RandomNode::new();
        node.init(&serde_json::json!({"type": "boolean"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        assert!(output.ports["default"]["random_value"].is_boolean());
    }

    #[tokio::test]
    async fn test_random_string() {
        let mut node = RandomNode::new();
        node.init(&serde_json::json!({"type": "string", "length": 16}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        let val = output.ports["default"]["random_value"].as_str().unwrap();
        assert_eq!(val.len(), 16);
    }

    #[tokio::test]
    async fn test_random_choice() {
        let mut node = RandomNode::new();
        node.init(&serde_json::json!({"type": "choice", "choices": ["a", "b", "c"]}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        let val = output.ports["default"]["random_value"].as_str().unwrap();
        assert!(["a", "b", "c"].contains(&val));
    }

    #[tokio::test]
    async fn test_processor_mode() {
        let mut node = RandomNode::new();
        node.init(&serde_json::json!({"type": "integer", "min": 1, "max": 100, "field": "rid"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let input = serde_json::json!({"name": "test"});
        let output = node.execute(input, &ctx).await.unwrap();
        assert_eq!(output.ports["default"]["name"], "test");
        assert!(output.ports["default"]["rid"].is_number());
    }

    #[tokio::test]
    async fn test_invalid_type() {
        let mut node = RandomNode::new();
        let result = node.init(&serde_json::json!({"type": "invalid"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_choice_without_choices() {
        let mut node = RandomNode::new();
        let result = node.init(&serde_json::json!({"type": "choice"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_node_type_and_kind() {
        let node = RandomNode::new();
        assert_eq!(node.node_type(), "random");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
