use std::time::{SystemTime, UNIX_EPOCH};

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// TotpNode — Simplified time-based OTP generation and verification.
///
/// Uses FNV-1a hash of (secret + time_step) for code generation.
/// NOT a real TOTP implementation (RFC 6238) — suitable for demos only.
///
/// Config:
/// ```json
/// {
///   "operation": "generate",
///   "secret": "my_secret",
///   "interval": 30,
///   "digits": 6
/// }
/// ```
pub struct TotpNode {
    operation: String,
    secret: Option<String>,
    secret_env: String,
    interval: u64,
    digits: u32,
}

impl Default for TotpNode {
    fn default() -> Self {
        Self::new()
    }
}

impl TotpNode {
    pub fn new() -> Self {
        Self {
            operation: "generate".to_string(),
            secret: None,
            secret_env: "TOTP_SECRET".to_string(),
            interval: 30,
            digits: 6,
        }
    }

    fn get_secret(&self) -> Result<String, NodeError> {
        // Try env var first, then config secret
        if let Ok(val) = std::env::var(&self.secret_env) {
            return Ok(val);
        }
        if let Some(ref s) = self.secret {
            return Ok(s.clone());
        }
        Err(NodeError::ExecutionFailed(format!(
            "no secret available: set env var '{}' or provide 'secret' in config",
            self.secret_env
        )))
    }

    fn current_time_step(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now / self.interval
    }

    fn generate_code(&self, secret: &str, time_step: u64) -> String {
        let input = format!("{}{}", secret, time_step);
        let hash = fnv1a_hash(input.as_bytes());
        let modulus = 10u64.pow(self.digits);
        let code_num = hash % modulus;
        format!("{:0>width$}", code_num, width = self.digits as usize)
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

#[async_trait::async_trait]
impl NodeExecutor for TotpNode {
    fn node_type(&self) -> &str {
        "totp"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "generate" | "verify" => {
                    self.operation = op.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown operation: '{}'. Use: generate, verify",
                        other
                    )));
                }
            }
        }

        if let Some(secret) = config.get("secret").and_then(|v| v.as_str()) {
            self.secret = Some(secret.to_string());
        }

        if let Some(env_name) = config.get("secret_env").and_then(|v| v.as_str()) {
            self.secret_env = env_name.to_string();
        }

        if let Some(interval) = config.get("interval").and_then(|v| v.as_u64()) {
            if interval == 0 {
                return Err(NodeError::InvalidConfig(
                    "interval must be greater than 0".to_string(),
                ));
            }
            self.interval = interval;
        }

        if let Some(digits) = config.get("digits").and_then(|v| v.as_u64()) {
            if digits == 0 || digits > 10 {
                return Err(NodeError::InvalidConfig(
                    "digits must be between 1 and 10".to_string(),
                ));
            }
            self.digits = digits as u32;
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let secret = self.get_secret()?;

        match self.operation.as_str() {
            "generate" => {
                let time_step = self.current_time_step();
                let code = self.generate_code(&secret, time_step);

                let now_secs = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let expires_in = self.interval - (now_secs % self.interval);

                Ok(NodeOutput::data(serde_json::json!({
                    "code": code,
                    "expires_in": expires_in,
                    "time_step": time_step,
                })))
            }
            "verify" => {
                let input_code = input
                    .get("code")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        NodeError::ExecutionFailed("missing 'code' in input".to_string())
                    })?;

                let time_step = self.current_time_step();

                // Check current time step and adjacent ones (±1)
                let valid = (-1i64..=1).any(|offset| {
                    let ts = if offset < 0 {
                        time_step.wrapping_sub((-offset) as u64)
                    } else {
                        time_step.wrapping_add(offset as u64)
                    };
                    let generated = self.generate_code(&secret, ts);
                    generated == input_code
                });

                Ok(NodeOutput::data(serde_json::json!({
                    "valid": valid,
                    "code": input_code,
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
    async fn test_generate_code() {
        let mut node = TotpNode::new();
        node.init(&serde_json::json!({
            "operation": "generate",
            "secret": "test_secret",
            "interval": 30
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({}), &ctx)
            .await
            .unwrap();

        let data = &output.ports["default"];
        let code = data["code"].as_str().unwrap();
        assert_eq!(code.len(), 6);
        assert!(data["expires_in"].as_u64().is_some());
        assert!(data["time_step"].as_u64().is_some());
    }

    #[tokio::test]
    async fn test_verify_valid_code() {
        let mut gen_node = TotpNode::new();
        gen_node
            .init(&serde_json::json!({
                "operation": "generate",
                "secret": "verify_secret"
            }))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let gen_output = gen_node
            .execute(serde_json::json!({}), &ctx)
            .await
            .unwrap();
        let code = gen_output.ports["default"]["code"]
            .as_str()
            .unwrap()
            .to_string();

        let mut verify_node = TotpNode::new();
        verify_node
            .init(&serde_json::json!({
                "operation": "verify",
                "secret": "verify_secret"
            }))
            .await
            .unwrap();

        let output = verify_node
            .execute(serde_json::json!({"code": code}), &ctx)
            .await
            .unwrap();

        assert_eq!(output.ports["default"]["valid"], true);
    }

    #[tokio::test]
    async fn test_verify_invalid_code() {
        let mut node = TotpNode::new();
        node.init(&serde_json::json!({
            "operation": "verify",
            "secret": "test_secret"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"code": "000000"}), &ctx)
            .await
            .unwrap();

        // Might be valid by coincidence, but very unlikely
        let _valid = output.ports["default"]["valid"].as_bool().unwrap();
    }

    #[tokio::test]
    async fn test_code_is_deterministic() {
        let mut node = TotpNode::new();
        node.init(&serde_json::json!({
            "operation": "generate",
            "secret": "deterministic_test"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let out1 = node.execute(serde_json::json!({}), &ctx).await.unwrap();
        let out2 = node.execute(serde_json::json!({}), &ctx).await.unwrap();

        // Same time step should produce same code
        assert_eq!(
            out1.ports["default"]["code"],
            out2.ports["default"]["code"]
        );
    }

    #[tokio::test]
    async fn test_missing_secret() {
        let mut node = TotpNode::new();
        node.init(&serde_json::json!({
            "operation": "generate",
            "secret_env": "NONEXISTENT_TOTP_VAR"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let result = node.execute(serde_json::json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_invalid_config() {
        let mut node = TotpNode::new();
        let result = node
            .init(&serde_json::json!({"interval": 0}))
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_node_type_and_kind() {
        let node = TotpNode::new();
        assert_eq!(node.node_type(), "totp");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
