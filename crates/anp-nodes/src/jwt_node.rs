use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// JWT node — simplified JWT token creation and verification.
///
/// Config:
/// ```json
/// {
///   "operation": "sign|verify|decode",
///   "secret_env": "JWT_SECRET",
///   "algorithm": "HS256",
///   "expires_in": 3600
/// }
/// ```
///
/// WARNING: This uses a simple FNV-based hash for signatures, NOT real HMAC.
/// This is NOT suitable for production security. Use a proper JWT library for real auth.
pub struct JwtNode {
    operation: String,
    secret_env: String,
    algorithm: String,
    expires_in: u64,
}

impl Default for JwtNode {
    fn default() -> Self {
        Self::new()
    }
}

impl JwtNode {
    pub fn new() -> Self {
        Self {
            operation: "sign".to_string(),
            secret_env: "JWT_SECRET".to_string(),
            algorithm: "HS256".to_string(),
            expires_in: 3600,
        }
    }
}

/// Base64url encode bytes (A-Za-z0-9-_, no padding)
fn base64url_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::with_capacity((data.len() * 4 + 2) / 3);
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i] as u32;
        let b1 = if i + 1 < data.len() { data[i + 1] as u32 } else { 0 };
        let b2 = if i + 2 < data.len() { data[i + 2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);

        if i + 1 < data.len() {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        }
        if i + 2 < data.len() {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        }
        i += 3;
    }
    result
}

/// Base64url decode string back to bytes
fn base64url_decode(s: &str) -> Result<Vec<u8>, String> {
    fn char_to_val(c: u8) -> Result<u32, String> {
        match c {
            b'A'..=b'Z' => Ok((c - b'A') as u32),
            b'a'..=b'z' => Ok((c - b'a' + 26) as u32),
            b'0'..=b'9' => Ok((c - b'0' + 52) as u32),
            b'-' => Ok(62),
            b'_' => Ok(63),
            _ => Err(format!("invalid base64url character: {}", c as char)),
        }
    }

    let bytes = s.as_bytes();
    let mut result = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut i = 0;
    while i < bytes.len() {
        let b0 = char_to_val(bytes[i])?;
        let b1 = if i + 1 < bytes.len() { char_to_val(bytes[i + 1])? } else { 0 };
        let b2 = if i + 2 < bytes.len() { char_to_val(bytes[i + 2])? } else { 0 };
        let b3 = if i + 3 < bytes.len() { char_to_val(bytes[i + 3])? } else { 0 };

        let triple = (b0 << 18) | (b1 << 12) | (b2 << 6) | b3;

        result.push(((triple >> 16) & 0xFF) as u8);
        if i + 2 < bytes.len() {
            result.push(((triple >> 8) & 0xFF) as u8);
        }
        if i + 3 < bytes.len() {
            result.push((triple & 0xFF) as u8);
        }
        i += 4;
    }
    Ok(result)
}

/// Simple FNV-1a-based hash for signature (NOT cryptographically secure)
fn fnv_hash(data: &str, secret: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in data.as_bytes().iter().chain(secret.as_bytes().iter()) {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn get_current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[async_trait::async_trait]
impl NodeExecutor for JwtNode {
    fn node_type(&self) -> &str {
        "jwt"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "sign" | "verify" | "decode" => self.operation = op.to_string(),
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown operation '{}'. Use: sign, verify, decode",
                        other
                    )));
                }
            }
        }
        if let Some(env) = config.get("secret_env").and_then(|v| v.as_str()) {
            self.secret_env = env.to_string();
        }
        if let Some(alg) = config.get("algorithm").and_then(|v| v.as_str()) {
            self.algorithm = alg.to_string();
        }
        if let Some(exp) = config.get("expires_in").and_then(|v| v.as_u64()) {
            self.expires_in = exp;
        }
        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        match self.operation.as_str() {
            "sign" => {
                let secret = std::env::var(&self.secret_env).map_err(|_| {
                    NodeError::ExecutionFailed(format!(
                        "environment variable '{}' not set",
                        self.secret_env
                    ))
                })?;

                // Build header
                let header = serde_json::json!({"alg": self.algorithm, "typ": "JWT"});
                let header_encoded = base64url_encode(header.to_string().as_bytes());

                // Build payload with claims
                let now = get_current_timestamp();
                let mut payload = if input.is_object() {
                    input.as_object().unwrap().clone()
                } else {
                    let mut m = serde_json::Map::new();
                    m.insert("data".to_string(), input);
                    m
                };
                payload.insert("iat".to_string(), serde_json::json!(now));
                payload.insert("exp".to_string(), serde_json::json!(now + self.expires_in));

                let payload_json = serde_json::Value::Object(payload);
                let payload_encoded = base64url_encode(payload_json.to_string().as_bytes());

                // Signature using FNV hash (NOT cryptographically secure)
                let signing_input = format!("{}.{}", header_encoded, payload_encoded);
                let sig_hash = fnv_hash(&signing_input, &secret);
                let sig_encoded = base64url_encode(&sig_hash.to_be_bytes());

                let token = format!("{}.{}.{}", header_encoded, payload_encoded, sig_encoded);

                Ok(NodeOutput::data(serde_json::json!({
                    "token": token,
                    "_warning": "NOT production crypto — uses FNV hash, not HMAC"
                })))
            }
            "verify" => {
                let secret = std::env::var(&self.secret_env).map_err(|_| {
                    NodeError::ExecutionFailed(format!(
                        "environment variable '{}' not set",
                        self.secret_env
                    ))
                })?;

                let token = input
                    .get("token")
                    .and_then(|v| v.as_str())
                    .or_else(|| input.as_str())
                    .ok_or_else(|| {
                        NodeError::MissingInput("expected 'token' field or string input".into())
                    })?;

                let parts: Vec<&str> = token.split('.').collect();
                if parts.len() != 3 {
                    return Ok(NodeOutput::data(serde_json::json!({
                        "valid": false,
                        "payload": null,
                        "error": "invalid token format: expected 3 parts separated by dots"
                    })));
                }

                // Verify signature
                let signing_input = format!("{}.{}", parts[0], parts[1]);
                let expected_hash = fnv_hash(&signing_input, &secret);
                let expected_sig = base64url_encode(&expected_hash.to_be_bytes());

                if parts[2] != expected_sig {
                    return Ok(NodeOutput::data(serde_json::json!({
                        "valid": false,
                        "payload": null,
                        "error": "invalid signature"
                    })));
                }

                // Decode payload
                let payload_bytes = base64url_decode(parts[1]).map_err(|e| {
                    NodeError::ExecutionFailed(format!("failed to decode payload: {}", e))
                })?;
                let payload: serde_json::Value =
                    serde_json::from_slice(&payload_bytes).map_err(|e| {
                        NodeError::ExecutionFailed(format!("failed to parse payload JSON: {}", e))
                    })?;

                // Check expiration
                let now = get_current_timestamp();
                if let Some(exp) = payload.get("exp").and_then(|v| v.as_u64()) {
                    if now > exp {
                        return Ok(NodeOutput::data(serde_json::json!({
                            "valid": false,
                            "payload": payload,
                            "error": "token expired"
                        })));
                    }
                }

                Ok(NodeOutput::data(serde_json::json!({
                    "valid": true,
                    "payload": payload,
                    "error": null
                })))
            }
            "decode" => {
                let token = input
                    .get("token")
                    .and_then(|v| v.as_str())
                    .or_else(|| input.as_str())
                    .ok_or_else(|| {
                        NodeError::MissingInput("expected 'token' field or string input".into())
                    })?;

                let parts: Vec<&str> = token.split('.').collect();
                if parts.len() != 3 {
                    return Err(NodeError::ExecutionFailed(
                        "invalid token format: expected 3 parts separated by dots".into(),
                    ));
                }

                let header_bytes = base64url_decode(parts[0]).map_err(|e| {
                    NodeError::ExecutionFailed(format!("failed to decode header: {}", e))
                })?;
                let header: serde_json::Value =
                    serde_json::from_slice(&header_bytes).map_err(|e| {
                        NodeError::ExecutionFailed(format!("failed to parse header JSON: {}", e))
                    })?;

                let payload_bytes = base64url_decode(parts[1]).map_err(|e| {
                    NodeError::ExecutionFailed(format!("failed to decode payload: {}", e))
                })?;
                let payload: serde_json::Value =
                    serde_json::from_slice(&payload_bytes).map_err(|e| {
                        NodeError::ExecutionFailed(format!("failed to parse payload JSON: {}", e))
                    })?;

                Ok(NodeOutput::data(serde_json::json!({
                    "header": header,
                    "payload": payload
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
    fn test_base64url_roundtrip() {
        let data = b"hello world";
        let encoded = base64url_encode(data);
        let decoded = base64url_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_base64url_no_padding() {
        let encoded = base64url_encode(b"test");
        assert!(!encoded.contains('='));
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
    }

    #[tokio::test]
    async fn test_sign_and_verify() {
        std::env::set_var("TEST_JWT_SECRET", "my_secret_key");

        let mut sign_node = JwtNode::new();
        sign_node
            .init(&serde_json::json!({
                "operation": "sign",
                "secret_env": "TEST_JWT_SECRET",
                "expires_in": 3600
            }))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = sign_node
            .execute(serde_json::json!({"user": "alice", "role": "admin"}), &ctx)
            .await
            .unwrap();

        let token = output.ports["default"]["token"].as_str().unwrap();
        assert!(token.contains('.'));
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);

        // Verify the token
        let mut verify_node = JwtNode::new();
        verify_node
            .init(&serde_json::json!({
                "operation": "verify",
                "secret_env": "TEST_JWT_SECRET"
            }))
            .await
            .unwrap();

        let verify_output = verify_node
            .execute(serde_json::json!({"token": token}), &ctx)
            .await
            .unwrap();

        assert_eq!(verify_output.ports["default"]["valid"], true);
        assert_eq!(verify_output.ports["default"]["payload"]["user"], "alice");
    }

    #[tokio::test]
    async fn test_decode() {
        std::env::set_var("TEST_JWT_SECRET_DECODE", "secret");

        let mut sign_node = JwtNode::new();
        sign_node
            .init(&serde_json::json!({
                "operation": "sign",
                "secret_env": "TEST_JWT_SECRET_DECODE"
            }))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = sign_node
            .execute(serde_json::json!({"sub": "123"}), &ctx)
            .await
            .unwrap();
        let token = output.ports["default"]["token"].as_str().unwrap();

        let mut decode_node = JwtNode::new();
        decode_node
            .init(&serde_json::json!({"operation": "decode"}))
            .await
            .unwrap();

        let decode_output = decode_node
            .execute(serde_json::json!({"token": token}), &ctx)
            .await
            .unwrap();

        assert_eq!(decode_output.ports["default"]["header"]["typ"], "JWT");
        assert_eq!(decode_output.ports["default"]["payload"]["sub"], "123");
    }

    #[tokio::test]
    async fn test_verify_invalid_signature() {
        std::env::set_var("TEST_JWT_SECRET_INVALID", "secret");

        let mut node = JwtNode::new();
        node.init(&serde_json::json!({
            "operation": "verify",
            "secret_env": "TEST_JWT_SECRET_INVALID"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"token": "aaa.bbb.ccc"}), &ctx)
            .await
            .unwrap();

        assert_eq!(output.ports["default"]["valid"], false);
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let mut node = JwtNode::new();
        let result = node
            .init(&serde_json::json!({"operation": "invalid"}))
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_node_type_and_kind() {
        let node = JwtNode::new();
        assert_eq!(node.node_type(), "jwt");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
