use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Base64Node — encode and decode base64
pub struct Base64Node {
    operation: String,
    url_safe: bool,
}

impl Default for Base64Node {
    fn default() -> Self {
        Self::new()
    }
}

impl Base64Node {
    pub fn new() -> Self {
        Self {
            operation: "encode".to_string(),
            url_safe: false,
        }
    }
}

const STANDARD_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL_SAFE_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn base64_encode(data: &[u8], url_safe: bool) -> String {
    let alphabet = if url_safe {
        URL_SAFE_ALPHABET
    } else {
        STANDARD_ALPHABET
    };

    let mut result = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };

        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(alphabet[((triple >> 18) & 0x3F) as usize] as char);
        result.push(alphabet[((triple >> 12) & 0x3F) as usize] as char);

        if chunk.len() > 1 {
            result.push(alphabet[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }

        if chunk.len() > 2 {
            result.push(alphabet[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }

    result
}

fn base64_decode(encoded: &str, url_safe: bool) -> Result<Vec<u8>, String> {
    let alphabet = if url_safe {
        URL_SAFE_ALPHABET
    } else {
        STANDARD_ALPHABET
    };

    let mut decode_table = [255u8; 256];
    for (i, &ch) in alphabet.iter().enumerate() {
        decode_table[ch as usize] = i as u8;
    }

    let clean: Vec<u8> = encoded
        .bytes()
        .filter(|&b| b != b'=' && b != b'\n' && b != b'\r' && b != b' ')
        .collect();

    let mut result = Vec::new();
    for chunk in clean.chunks(4) {
        let mut sextet = [0u32; 4];
        for (i, &byte) in chunk.iter().enumerate() {
            let val = decode_table[byte as usize];
            if val == 255 {
                return Err(format!("invalid base64 character: '{}'", byte as char));
            }
            sextet[i] = val as u32;
        }

        let triple = (sextet[0] << 18) | (sextet[1] << 12) | (sextet[2] << 6) | sextet[3];

        result.push(((triple >> 16) & 0xFF) as u8);
        if chunk.len() > 2 {
            result.push(((triple >> 8) & 0xFF) as u8);
        }
        if chunk.len() > 3 {
            result.push((triple & 0xFF) as u8);
        }
    }

    Ok(result)
}

fn extract_data(input: &serde_json::Value) -> Option<String> {
    if let Some(s) = input.as_str() {
        Some(s.to_string())
    } else if let Some(s) = input.get("data").and_then(|v| v.as_str()) {
        Some(s.to_string())
    } else {
        input
            .get("text")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }
}

#[async_trait::async_trait]
impl NodeExecutor for Base64Node {
    fn node_type(&self) -> &str {
        "base64"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "encode" | "decode" => self.operation = op.to_string(),
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown operation '{}'. Use: encode, decode",
                        other
                    )));
                }
            }
        }

        if let Some(u) = config.get("url_safe").and_then(|v| v.as_bool()) {
            self.url_safe = u;
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let data = extract_data(&input).ok_or_else(|| {
            NodeError::ExecutionFailed("input must be a string or { \"data\": \"...\" }".into())
        })?;

        let result = match self.operation.as_str() {
            "encode" => {
                let encoded = base64_encode(data.as_bytes(), self.url_safe);
                serde_json::Value::String(encoded)
            }
            "decode" => {
                let decoded_bytes = base64_decode(&data, self.url_safe).map_err(|e| {
                    NodeError::ExecutionFailed(format!("base64 decode error: {}", e))
                })?;
                let decoded_str = String::from_utf8(decoded_bytes).map_err(|e| {
                    NodeError::ExecutionFailed(format!("decoded data is not valid UTF-8: {}", e))
                })?;
                serde_json::Value::String(decoded_str)
            }
            _ => {
                return Err(NodeError::ExecutionFailed(format!(
                    "unknown operation: {}",
                    self.operation
                )));
            }
        };

        Ok(NodeOutput::data(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anp_core::node::traits::NodeExecutor;
    use anp_core::runtime::context::ExecutionContext;
    use serde_json::json;

    async fn make_node(operation: &str, url_safe: bool) -> Base64Node {
        let mut node = Base64Node::new();
        node.init(&json!({ "operation": operation, "url_safe": url_safe }))
            .await
            .unwrap();
        node
    }

    #[tokio::test]
    async fn test_encode_hello() {
        let node = make_node("encode", false).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!("Hello, World!"), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], json!("SGVsbG8sIFdvcmxkIQ=="));
    }

    #[tokio::test]
    async fn test_decode_hello() {
        let node = make_node("decode", false).await;
        let ctx = ExecutionContext::new();
        let output = node
            .execute(json!("SGVsbG8sIFdvcmxkIQ=="), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"], json!("Hello, World!"));
    }

    #[tokio::test]
    async fn test_encode_empty_string() {
        let node = make_node("encode", false).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!(""), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], json!(""));
    }

    #[tokio::test]
    async fn test_decode_empty_string() {
        let node = make_node("decode", false).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!(""), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], json!(""));
    }

    #[tokio::test]
    async fn test_encode_url_safe() {
        let node = make_node("encode", true).await;
        let ctx = ExecutionContext::new();
        // Bytes that would produce +/ in standard base64
        let output = node.execute(json!("subjects?_d"), &ctx).await.unwrap();
        let encoded = output.ports["default"].as_str().unwrap();
        // URL-safe should not contain + or /
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
    }

    #[tokio::test]
    async fn test_roundtrip_standard() {
        let ctx = ExecutionContext::new();
        let original = "The quick brown fox jumps over the lazy dog!";
        let enc_node = make_node("encode", false).await;
        let enc_output = enc_node.execute(json!(original), &ctx).await.unwrap();
        let encoded = enc_output.ports["default"].as_str().unwrap().to_string();

        let dec_node = make_node("decode", false).await;
        let dec_output = dec_node.execute(json!(encoded), &ctx).await.unwrap();
        assert_eq!(dec_output.ports["default"], json!(original));
    }

    #[tokio::test]
    async fn test_roundtrip_url_safe() {
        let ctx = ExecutionContext::new();
        let original = "special chars: +/= and more???";
        let enc_node = make_node("encode", true).await;
        let enc_output = enc_node.execute(json!(original), &ctx).await.unwrap();
        let encoded = enc_output.ports["default"].as_str().unwrap().to_string();

        let dec_node = make_node("decode", true).await;
        let dec_output = dec_node.execute(json!(encoded), &ctx).await.unwrap();
        assert_eq!(dec_output.ports["default"], json!(original));
    }

    #[tokio::test]
    async fn test_encode_special_characters() {
        let node = make_node("encode", false).await;
        let ctx = ExecutionContext::new();
        let output = node
            .execute(json!("line1\nline2\ttab"), &ctx)
            .await
            .unwrap();
        // Just verify it produces a valid base64 string (no error)
        assert!(output.ports["default"].is_string());
    }

    #[tokio::test]
    async fn test_input_from_data_field() {
        let node = make_node("encode", false).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!({"data": "test"}), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], json!("dGVzdA=="));
    }
}
