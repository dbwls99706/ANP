use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Encrypt node — simple XOR-based encryption/encoding.
///
/// Config:
/// ```json
/// {
///   "operation": "encrypt|decrypt",
///   "key_env": "ENCRYPT_KEY"
/// }
/// ```
///
/// WARNING: This uses a simple XOR cipher. This is NOT secure cryptography.
/// Do NOT use this for production security. Use a proper encryption library
/// (e.g., AES-256-GCM via the `ring` or `aes-gcm` crate) for real encryption.
pub struct EncryptNode {
    operation: String,
    key_env: String,
}

impl Default for EncryptNode {
    fn default() -> Self {
        Self::new()
    }
}

impl EncryptNode {
    pub fn new() -> Self {
        Self {
            operation: "encrypt".to_string(),
            key_env: "ENCRYPT_KEY".to_string(),
        }
    }
}

/// XOR a byte slice with a key (repeating the key as needed)
fn xor_bytes(data: &[u8], key: &[u8]) -> Vec<u8> {
    if key.is_empty() {
        return data.to_vec();
    }
    data.iter()
        .enumerate()
        .map(|(i, b)| b ^ key[i % key.len()])
        .collect()
}

/// Encode bytes as lowercase hex string
fn to_hex(data: &[u8]) -> String {
    let mut result = String::with_capacity(data.len() * 2);
    for byte in data {
        result.push_str(&format!("{:02x}", byte));
    }
    result
}

/// Decode hex string to bytes
fn from_hex(hex: &str) -> Result<Vec<u8>, String> {
    if !hex.len().is_multiple_of(2) {
        return Err("hex string must have even length".to_string());
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let chars: Vec<u8> = hex.as_bytes().to_vec();
    let mut i = 0;
    while i < chars.len() {
        let hi = hex_char_to_val(chars[i])?;
        let lo = hex_char_to_val(chars[i + 1])?;
        bytes.push((hi << 4) | lo);
        i += 2;
    }
    Ok(bytes)
}

fn hex_char_to_val(c: u8) -> Result<u8, String> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(format!("invalid hex character: {}", c as char)),
    }
}

#[async_trait::async_trait]
impl NodeExecutor for EncryptNode {
    fn node_type(&self) -> &str {
        "encrypt"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "encrypt" | "decrypt" => self.operation = op.to_string(),
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown operation '{}'. Use: encrypt, decrypt",
                        other
                    )));
                }
            }
        }
        if let Some(env) = config.get("key_env").and_then(|v| v.as_str()) {
            self.key_env = env.to_string();
        }
        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let key = std::env::var(&self.key_env).map_err(|_| {
            NodeError::ExecutionFailed(format!("environment variable '{}' not set", self.key_env))
        })?;

        let data_str = input
            .get("data")
            .and_then(|v| v.as_str())
            .or_else(|| input.as_str())
            .ok_or_else(|| {
                NodeError::MissingInput("expected string input or object with 'data' field".into())
            })?;

        match self.operation.as_str() {
            "encrypt" => {
                let encrypted = xor_bytes(data_str.as_bytes(), key.as_bytes());
                let hex_result = to_hex(&encrypted);

                Ok(NodeOutput::data(serde_json::json!({
                    "result": hex_result,
                    "algorithm": "xor",
                    "_warning": "NOT secure crypto — XOR cipher is trivially reversible and provides no real security"
                })))
            }
            "decrypt" => {
                let encrypted_bytes = from_hex(data_str).map_err(|e| {
                    NodeError::ExecutionFailed(format!("failed to decode hex input: {}", e))
                })?;
                let decrypted = xor_bytes(&encrypted_bytes, key.as_bytes());
                let result = String::from_utf8(decrypted).map_err(|e| {
                    NodeError::ExecutionFailed(format!("decrypted data is not valid UTF-8: {}", e))
                })?;

                Ok(NodeOutput::data(serde_json::json!({
                    "result": result,
                    "algorithm": "xor",
                    "_warning": "NOT secure crypto — XOR cipher is trivially reversible and provides no real security"
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

    #[test]
    fn test_xor_roundtrip() {
        let data = b"hello world";
        let key = b"secret";
        let encrypted = xor_bytes(data, key);
        let decrypted = xor_bytes(&encrypted, key);
        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_hex_roundtrip() {
        let data = vec![0x00, 0x0f, 0xff, 0xab, 0x12];
        let hex = to_hex(&data);
        assert_eq!(hex, "000fffab12");
        let decoded = from_hex(&hex).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_hex_invalid() {
        assert!(from_hex("0g").is_err());
        assert!(from_hex("abc").is_err()); // odd length
    }

    #[tokio::test]
    async fn test_encrypt_decrypt() {
        std::env::set_var("TEST_ENCRYPT_KEY", "mysecretkey");

        let mut enc_node = EncryptNode::new();
        enc_node
            .init(&serde_json::json!({
                "operation": "encrypt",
                "key_env": "TEST_ENCRYPT_KEY"
            }))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = enc_node
            .execute(serde_json::json!({"data": "hello world"}), &ctx)
            .await
            .unwrap();

        let encrypted = output.ports["default"]["result"].as_str().unwrap();
        assert!(!encrypted.is_empty());
        assert_eq!(output.ports["default"]["algorithm"], "xor");

        // Decrypt
        let mut dec_node = EncryptNode::new();
        dec_node
            .init(&serde_json::json!({
                "operation": "decrypt",
                "key_env": "TEST_ENCRYPT_KEY"
            }))
            .await
            .unwrap();

        let dec_output = dec_node
            .execute(serde_json::json!({"data": encrypted}), &ctx)
            .await
            .unwrap();

        assert_eq!(dec_output.ports["default"]["result"], "hello world");
    }

    #[tokio::test]
    async fn test_string_input() {
        std::env::set_var("TEST_ENCRYPT_KEY2", "key");

        let mut node = EncryptNode::new();
        node.init(&serde_json::json!({
            "operation": "encrypt",
            "key_env": "TEST_ENCRYPT_KEY2"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::json!("test"), &ctx).await.unwrap();

        assert!(output.ports["default"]["result"].as_str().is_some());
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let mut node = EncryptNode::new();
        let result = node
            .init(&serde_json::json!({"operation": "invalid"}))
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_node_type_and_kind() {
        let node = EncryptNode::new();
        assert_eq!(node.node_type(), "encrypt");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
