use async_trait::async_trait;
use flate2::read::{GzDecoder, GzEncoder};
use flate2::Compression;
use serde_json::{json, Value};
use std::io::Read;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Gzip compression/decompression node.
/// Compresses text/JSON for HTTP responses or decompresses incoming data.
/// Uses flate2 (zlib) for production-grade compression.
pub struct GzipNode {
    operation: String,
    level: u32,
}

impl GzipNode {
    pub fn new() -> Self {
        Self {
            operation: "compress".to_string(),
            level: 6, // Default compression level (1-9)
        }
    }
}

impl Default for GzipNode {
    fn default() -> Self {
        Self::new()
    }
}

fn to_base64(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i] as u32;
        let b1 = if i + 1 < data.len() {
            data[i + 1] as u32
        } else {
            0
        };
        let b2 = if i + 2 < data.len() {
            data[i + 2] as u32
        } else {
            0
        };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if i + 1 < data.len() {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if i + 2 < data.len() {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        i += 3;
    }
    result
}

fn from_base64(s: &str) -> Result<Vec<u8>, &'static str> {
    let mut result = Vec::new();
    let bytes: Vec<u8> = s
        .bytes()
        .filter(|&b| b != b'\n' && b != b'\r' && b != b' ')
        .collect();
    if bytes.len() % 4 != 0 {
        return Err("invalid base64 length");
    }
    fn decode_char(c: u8) -> Result<u8, &'static str> {
        match c {
            b'A'..=b'Z' => Ok(c - b'A'),
            b'a'..=b'z' => Ok(c - b'a' + 26),
            b'0'..=b'9' => Ok(c - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            b'=' => Ok(0),
            _ => Err("invalid base64 character"),
        }
    }
    for chunk in bytes.chunks(4) {
        let a = decode_char(chunk[0])?;
        let b = decode_char(chunk[1])?;
        let c = decode_char(chunk[2])?;
        let d = decode_char(chunk[3])?;
        let triple = ((a as u32) << 18) | ((b as u32) << 12) | ((c as u32) << 6) | (d as u32);
        result.push((triple >> 16) as u8);
        if chunk[2] != b'=' {
            result.push(((triple >> 8) & 0xFF) as u8);
        }
        if chunk[3] != b'=' {
            result.push((triple & 0xFF) as u8);
        }
    }
    Ok(result)
}

#[async_trait]
impl NodeExecutor for GzipNode {
    fn node_type(&self) -> &str {
        "gzip"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "compress" | "decompress" => self.operation = op.to_string(),
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown operation '{}', expected 'compress' or 'decompress'",
                        other
                    )));
                }
            }
        }
        if let Some(level) = config.get("level").and_then(|v| v.as_u64()) {
            if level > 9 {
                return Err(NodeError::InvalidConfig(
                    "level must be between 0 and 9".into(),
                ));
            }
            self.level = level as u32;
        }
        Ok(())
    }

    async fn execute(&self, input: Value, _ctx: &ExecutionContext) -> NodeResult<NodeOutput> {
        let op = input
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.operation);

        match op {
            "compress" => {
                let data = input
                    .get("data")
                    .or_else(|| input.get("body"))
                    .or_else(|| input.get("text"))
                    .map(|v| {
                        v.as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| v.to_string())
                    })
                    .ok_or_else(|| {
                        NodeError::MissingInput("data, body, or text field required".into())
                    })?;

                let original_size = data.len();
                let level = input
                    .get("level")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(self.level as u64) as u32;

                let mut encoder = GzEncoder::new(data.as_bytes(), Compression::new(level));
                let mut compressed = Vec::new();
                encoder.read_to_end(&mut compressed).map_err(|e| {
                    NodeError::ExecutionFailed(format!("compression failed: {}", e))
                })?;

                let compressed_size = compressed.len();
                let ratio = if original_size > 0 {
                    (1.0 - (compressed_size as f64 / original_size as f64)) * 100.0
                } else {
                    0.0
                };

                let encoded = to_base64(&compressed);

                Ok(NodeOutput::data(json!({
                    "compressed": encoded,
                    "original_size": original_size,
                    "compressed_size": compressed_size,
                    "compression_ratio": format!("{:.1}%", ratio),
                    "encoding": "gzip+base64",
                    "_content_encoding": "gzip",
                })))
            }

            "decompress" => {
                let encoded = input
                    .get("compressed")
                    .or_else(|| input.get("data"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        NodeError::MissingInput("compressed or data field required".into())
                    })?;

                let compressed = from_base64(encoded)
                    .map_err(|e| NodeError::ExecutionFailed(format!("invalid base64: {}", e)))?;

                let mut decoder = GzDecoder::new(&compressed[..]);
                let mut decompressed = String::new();
                decoder.read_to_string(&mut decompressed).map_err(|e| {
                    NodeError::ExecutionFailed(format!("decompression failed: {}", e))
                })?;

                Ok(NodeOutput::data(json!({
                    "data": decompressed,
                    "compressed_size": compressed.len(),
                    "decompressed_size": decompressed.len(),
                })))
            }

            other => Err(NodeError::ExecutionFailed(format!(
                "unknown operation: {}",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ExecutionContext {
        ExecutionContext::new()
    }

    #[tokio::test]
    async fn test_compress_decompress_roundtrip() {
        let mut comp = GzipNode::new();
        comp.init(&json!({"operation": "compress"})).await.unwrap();

        let original = "Hello, World! This is a test of gzip compression in ANP. Repeated text helps compression: AAAAAAAAAA";
        let comp_out = comp
            .execute(json!({"data": original}), &ctx())
            .await
            .unwrap();
        let compressed = comp_out.ports["default"]["compressed"].as_str().unwrap();

        let mut decomp = GzipNode::new();
        decomp
            .init(&json!({"operation": "decompress"}))
            .await
            .unwrap();
        let decomp_out = decomp
            .execute(json!({"compressed": compressed}), &ctx())
            .await
            .unwrap();
        assert_eq!(decomp_out.ports["default"]["data"], original);
    }

    #[tokio::test]
    async fn test_compression_ratio() {
        let mut node = GzipNode::new();
        node.init(&json!({"operation": "compress"})).await.unwrap();

        let data = "A".repeat(1000);
        let output = node.execute(json!({"data": data}), &ctx()).await.unwrap();

        let original_size = output.ports["default"]["original_size"].as_u64().unwrap();
        let compressed_size = output.ports["default"]["compressed_size"].as_u64().unwrap();
        assert_eq!(original_size, 1000);
        assert!(compressed_size < 100); // Repeated data compresses very well
    }

    #[tokio::test]
    async fn test_compress_json() {
        let mut node = GzipNode::new();
        node.init(&json!({"operation": "compress"})).await.unwrap();

        let output = node
            .execute(json!({"body": "{\"key\": \"value\"}"}), &ctx())
            .await
            .unwrap();
        assert!(output.ports["default"]["compressed"].as_str().is_some());
        assert_eq!(output.ports["default"]["encoding"], "gzip+base64");
    }

    #[tokio::test]
    async fn test_custom_level() {
        let mut node = GzipNode::new();
        node.init(&json!({"operation": "compress", "level": 1}))
            .await
            .unwrap();

        let output = node
            .execute(json!({"data": "test data for compression"}), &ctx())
            .await
            .unwrap();
        assert!(output.ports["default"]["compressed_size"].as_u64().unwrap() > 0);
    }
}
