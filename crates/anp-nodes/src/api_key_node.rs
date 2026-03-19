use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// ApiKeyNode — API key validation, generation, and masking.
///
/// Config:
/// ```json
/// {
///   "operation": "validate",
///   "valid_keys": ["key1", "key2"],
///   "header_field": "x-api-key",
///   "key_env": "API_KEYS"
/// }
/// ```
pub struct ApiKeyNode {
    operation: String,
    valid_keys: Vec<String>,
    header_field: String,
    key_env: Option<String>,
    counter: AtomicU64,
}

impl Default for ApiKeyNode {
    fn default() -> Self {
        Self::new()
    }
}

impl ApiKeyNode {
    pub fn new() -> Self {
        Self {
            operation: "validate".to_string(),
            valid_keys: Vec::new(),
            header_field: "x-api-key".to_string(),
            key_env: None,
            counter: AtomicU64::new(0),
        }
    }

    fn mask_key(key: &str) -> String {
        if key.len() <= 8 {
            "*".repeat(key.len())
        } else {
            let first4 = &key[..4];
            let last4 = &key[key.len() - 4..];
            format!("{}***{}", first4, last4)
        }
    }

    fn generate_key(&self) -> String {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let count = self.counter.fetch_add(1, Ordering::SeqCst);
        format!("anp_{:x}{:x}", timestamp, count)
    }
}

#[async_trait::async_trait]
impl NodeExecutor for ApiKeyNode {
    fn node_type(&self) -> &str {
        "api_key"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "validate" | "generate" | "mask" => {
                    self.operation = op.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown operation: '{}'. Use: validate, generate, mask",
                        other
                    )));
                }
            }
        }

        if let Some(keys) = config.get("valid_keys").and_then(|v| v.as_array()) {
            self.valid_keys = keys
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }

        if let Some(field) = config.get("header_field").and_then(|v| v.as_str()) {
            self.header_field = field.to_string();
        }

        if let Some(env_var) = config.get("key_env").and_then(|v| v.as_str()) {
            self.key_env = Some(env_var.to_string());
            if let Ok(env_keys) = std::env::var(env_var) {
                for k in env_keys.split(',') {
                    let trimmed = k.trim().to_string();
                    if !trimmed.is_empty() {
                        self.valid_keys.push(trimmed);
                    }
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
        match self.operation.as_str() {
            "validate" => {
                let key = input
                    .get("key")
                    .or_else(|| input.get(&self.header_field))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        NodeError::ExecutionFailed(
                            "missing 'key' or header field in input".to_string(),
                        )
                    })?;

                let is_valid = self.valid_keys.iter().any(|k| k == key);
                let masked = Self::mask_key(key);

                let result = serde_json::json!({
                    "valid": is_valid,
                    "key_masked": masked,
                });

                let mut ports = HashMap::new();
                if is_valid {
                    ports.insert("valid".to_string(), result);
                } else {
                    ports.insert("invalid".to_string(), result);
                }
                Ok(NodeOutput::multi(ports))
            }
            "generate" => {
                let api_key = self.generate_key();
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                Ok(NodeOutput::data(serde_json::json!({
                    "api_key": api_key,
                    "created_at": timestamp,
                })))
            }
            "mask" => {
                let key = input.get("key").and_then(|v| v.as_str()).ok_or_else(|| {
                    NodeError::ExecutionFailed("missing 'key' in input".to_string())
                })?;

                let original_length = key.len();
                let masked = Self::mask_key(key);

                Ok(NodeOutput::data(serde_json::json!({
                    "masked": masked,
                    "original_length": original_length,
                })))
            }
            _ => Err(NodeError::ExecutionFailed(format!(
                "unknown operation: '{}'",
                self.operation
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_validate_valid_key() {
        let mut node = ApiKeyNode::new();
        node.init(&serde_json::json!({
            "operation": "validate",
            "valid_keys": ["secret123", "key456"]
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"key": "secret123"}), &ctx)
            .await
            .unwrap();

        assert!(output.ports.contains_key("valid"));
        assert_eq!(output.ports["valid"]["valid"], true);
    }

    #[tokio::test]
    async fn test_validate_invalid_key() {
        let mut node = ApiKeyNode::new();
        node.init(&serde_json::json!({
            "operation": "validate",
            "valid_keys": ["secret123"]
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"key": "wrong_key"}), &ctx)
            .await
            .unwrap();

        assert!(output.ports.contains_key("invalid"));
        assert_eq!(output.ports["invalid"]["valid"], false);
    }

    #[tokio::test]
    async fn test_generate_key() {
        let mut node = ApiKeyNode::new();
        node.init(&serde_json::json!({"operation": "generate"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::json!({}), &ctx).await.unwrap();

        let data = &output.ports["default"];
        let api_key = data["api_key"].as_str().unwrap();
        assert!(api_key.starts_with("anp_"));
        assert!(data["created_at"].as_u64().is_some());
    }

    #[tokio::test]
    async fn test_mask_key() {
        let mut node = ApiKeyNode::new();
        node.init(&serde_json::json!({"operation": "mask"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"key": "abcdefghijklmnop"}), &ctx)
            .await
            .unwrap();

        let data = &output.ports["default"];
        assert_eq!(data["masked"], "abcd***mnop");
        assert_eq!(data["original_length"], 16);
    }

    #[tokio::test]
    async fn test_mask_short_key() {
        let mut node = ApiKeyNode::new();
        node.init(&serde_json::json!({"operation": "mask"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"key": "short"}), &ctx)
            .await
            .unwrap();

        let data = &output.ports["default"];
        assert_eq!(data["masked"], "*****");
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let mut node = ApiKeyNode::new();
        let result = node.init(&serde_json::json!({"operation": "bad"})).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_node_type_and_kind() {
        let node = ApiKeyNode::new();
        assert_eq!(node.node_type(), "api_key");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
