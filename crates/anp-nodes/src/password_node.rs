use std::time::{SystemTime, UNIX_EPOCH};

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// PasswordNode — Password hashing (FNV-1a based) and strength analysis.
///
/// NOT production-grade cryptography. Uses FNV-1a for hashing, suitable
/// for demonstration and non-security-critical use cases.
///
/// Config:
/// ```json
/// {
///   "operation": "hash",
///   "salt": "optional_fixed_salt"
/// }
/// ```
pub struct PasswordNode {
    operation: String,
    salt: Option<String>,
}

impl Default for PasswordNode {
    fn default() -> Self {
        Self::new()
    }
}

impl PasswordNode {
    pub fn new() -> Self {
        Self {
            operation: "hash".to_string(),
            salt: None,
        }
    }
}

/// FNV-1a 64-bit hash
fn fnv1a_hash(data: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 14695981039346656037;
    const FNV_PRIME: u64 = 1099511628211;

    let mut hash = FNV_OFFSET_BASIS;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn hash_password(password: &str, salt: &str) -> String {
    let combined = format!("{}{}", password, salt);
    let hash = fnv1a_hash(combined.as_bytes());
    format!("{:016x}", hash)
}

fn generate_salt() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:x}", timestamp)
}

fn check_strength(password: &str) -> serde_json::Value {
    let has_length = password.len() >= 8;
    let has_uppercase = password.chars().any(|c| c.is_uppercase());
    let has_lowercase = password.chars().any(|c| c.is_lowercase());
    let has_digit = password.chars().any(|c| c.is_ascii_digit());
    let has_special = password
        .chars()
        .any(|c| !c.is_alphanumeric() && !c.is_whitespace());

    let score = [has_length, has_uppercase, has_lowercase, has_digit, has_special]
        .iter()
        .filter(|&&b| b)
        .count();

    let label = match score {
        0..=1 => "weak",
        2..=3 => "moderate",
        _ => "strong",
    };

    serde_json::json!({
        "score": score,
        "label": label,
        "checks": {
            "length": has_length,
            "uppercase": has_uppercase,
            "lowercase": has_lowercase,
            "digit": has_digit,
            "special": has_special,
        }
    })
}

#[async_trait::async_trait]
impl NodeExecutor for PasswordNode {
    fn node_type(&self) -> &str {
        "password"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "hash" | "verify" | "strength" => {
                    self.operation = op.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown operation: '{}'. Use: hash, verify, strength",
                        other
                    )));
                }
            }
        }

        if let Some(salt) = config.get("salt").and_then(|v| v.as_str()) {
            self.salt = Some(salt.to_string());
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        match self.operation.as_str() {
            "hash" => {
                let password = input
                    .get("password")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        NodeError::ExecutionFailed("missing 'password' in input".to_string())
                    })?;

                let salt = self
                    .salt
                    .clone()
                    .or_else(|| input.get("salt").and_then(|v| v.as_str()).map(String::from))
                    .unwrap_or_else(generate_salt);

                let hash = hash_password(password, &salt);

                Ok(NodeOutput::data(serde_json::json!({
                    "hash": hash,
                    "salt": salt,
                    "algorithm": "fnv1a",
                })))
            }
            "verify" => {
                let password = input
                    .get("password")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        NodeError::ExecutionFailed("missing 'password' in input".to_string())
                    })?;

                let salt = input
                    .get("salt")
                    .and_then(|v| v.as_str())
                    .or_else(|| self.salt.as_deref())
                    .ok_or_else(|| {
                        NodeError::ExecutionFailed("missing 'salt' in input".to_string())
                    })?;

                let stored_hash = input
                    .get("hash")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        NodeError::ExecutionFailed("missing 'hash' in input".to_string())
                    })?;

                let computed = hash_password(password, salt);
                let valid = computed == stored_hash;

                Ok(NodeOutput::data(serde_json::json!({
                    "valid": valid,
                })))
            }
            "strength" => {
                let password = input
                    .get("password")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        NodeError::ExecutionFailed("missing 'password' in input".to_string())
                    })?;

                let result = check_strength(password);
                Ok(NodeOutput::data(result))
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
    async fn test_hash_password() {
        let mut node = PasswordNode::new();
        node.init(&serde_json::json!({"operation": "hash", "salt": "fixed_salt"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"password": "mysecret"}), &ctx)
            .await
            .unwrap();

        let data = &output.ports["default"];
        assert_eq!(data["salt"], "fixed_salt");
        assert_eq!(data["algorithm"], "fnv1a");
        assert!(data["hash"].as_str().unwrap().len() == 16);
    }

    #[tokio::test]
    async fn test_verify_correct_password() {
        // First hash
        let mut node = PasswordNode::new();
        node.init(&serde_json::json!({"operation": "hash", "salt": "test_salt"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let hash_output = node
            .execute(serde_json::json!({"password": "correct"}), &ctx)
            .await
            .unwrap();

        let hash_val = hash_output.ports["default"]["hash"].as_str().unwrap().to_string();

        // Now verify
        let mut verify_node = PasswordNode::new();
        verify_node
            .init(&serde_json::json!({"operation": "verify"}))
            .await
            .unwrap();

        let output = verify_node
            .execute(
                serde_json::json!({
                    "password": "correct",
                    "salt": "test_salt",
                    "hash": hash_val,
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert_eq!(output.ports["default"]["valid"], true);
    }

    #[tokio::test]
    async fn test_verify_wrong_password() {
        let hash = hash_password("correct", "salt");

        let mut node = PasswordNode::new();
        node.init(&serde_json::json!({"operation": "verify"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({
                    "password": "wrong",
                    "salt": "salt",
                    "hash": hash,
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert_eq!(output.ports["default"]["valid"], false);
    }

    #[tokio::test]
    async fn test_strength_strong() {
        let mut node = PasswordNode::new();
        node.init(&serde_json::json!({"operation": "strength"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"password": "MyP@ssw0rd!"}), &ctx)
            .await
            .unwrap();

        let data = &output.ports["default"];
        assert_eq!(data["score"], 5);
        assert_eq!(data["label"], "strong");
        assert_eq!(data["checks"]["length"], true);
        assert_eq!(data["checks"]["uppercase"], true);
        assert_eq!(data["checks"]["lowercase"], true);
        assert_eq!(data["checks"]["digit"], true);
        assert_eq!(data["checks"]["special"], true);
    }

    #[tokio::test]
    async fn test_strength_weak() {
        let mut node = PasswordNode::new();
        node.init(&serde_json::json!({"operation": "strength"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"password": "abc"}), &ctx)
            .await
            .unwrap();

        let data = &output.ports["default"];
        assert_eq!(data["label"], "weak");
        assert_eq!(data["checks"]["length"], false);
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let mut node = PasswordNode::new();
        let result = node.init(&serde_json::json!({"operation": "bad"})).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_node_type_and_kind() {
        let node = PasswordNode::new();
        assert_eq!(node.node_type(), "password");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
