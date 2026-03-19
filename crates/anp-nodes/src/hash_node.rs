use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// HashNode — hashing operations using FNV-1a (non-cryptographic)
pub struct HashNode {
    algorithm: String,
}

impl Default for HashNode {
    fn default() -> Self {
        Self::new()
    }
}

impl HashNode {
    pub fn new() -> Self {
        Self {
            algorithm: "fnv1a".to_string(),
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

fn to_hex(value: u64) -> String {
    format!("{:016x}", value)
}

fn extract_data(input: &serde_json::Value) -> Option<String> {
    if let Some(s) = input.as_str() {
        Some(s.to_string())
    } else if let Some(s) = input.get("data").and_then(|v| v.as_str()) {
        Some(s.to_string())
    } else if let Some(s) = input.get("text").and_then(|v| v.as_str()) {
        Some(s.to_string())
    } else {
        None
    }
}

#[async_trait::async_trait]
impl NodeExecutor for HashNode {
    fn node_type(&self) -> &str {
        "hash"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(alg) = config.get("algorithm").and_then(|v| v.as_str()) {
            match alg {
                "fnv1a" | "simple" | "hash" => {
                    self.algorithm = "fnv1a".to_string();
                }
                "sha256" | "md5" => {
                    return Err(NodeError::InvalidConfig(format!(
                        "cryptographic algorithm '{}' is not available without external dependencies. \
                         Use 'fnv1a' (or 'simple'/'hash') for non-cryptographic hashing suitable \
                         for partitioning and deduplication.",
                        alg
                    )));
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown algorithm '{}'. Available: fnv1a, simple, hash",
                        other
                    )));
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
        let data = extract_data(&input).ok_or_else(|| {
            NodeError::ExecutionFailed("input must be a string or { \"data\": \"...\" }".into())
        })?;

        let hash_value = fnv1a_hash(data.as_bytes());
        let hex_str = to_hex(hash_value);

        let result = serde_json::json!({
            "hash": hex_str,
            "algorithm": "fnv1a"
        });

        Ok(NodeOutput::data(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anp_core::node::traits::NodeExecutor;
    use anp_core::runtime::context::ExecutionContext;
    use serde_json::json;

    async fn make_node() -> HashNode {
        let mut node = HashNode::new();
        node.init(&json!({})).await.unwrap();
        node
    }

    #[tokio::test]
    async fn test_hash_string_input() {
        let node = make_node().await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!("hello"), &ctx).await.unwrap();
        let result = &output.ports["default"];
        assert_eq!(result["algorithm"], json!("fnv1a"));
        assert!(result["hash"].is_string());
        assert_eq!(result["hash"].as_str().unwrap().len(), 16); // 64-bit hex = 16 chars
    }

    #[tokio::test]
    async fn test_hash_consistent() {
        let node = make_node().await;
        let ctx = ExecutionContext::new();
        let output1 = node.execute(json!("hello"), &ctx).await.unwrap();
        let output2 = node.execute(json!("hello"), &ctx).await.unwrap();
        assert_eq!(output1.ports["default"]["hash"], output2.ports["default"]["hash"]);
    }

    #[tokio::test]
    async fn test_hash_different_inputs_differ() {
        let node = make_node().await;
        let ctx = ExecutionContext::new();
        let output1 = node.execute(json!("hello"), &ctx).await.unwrap();
        let output2 = node.execute(json!("world"), &ctx).await.unwrap();
        assert_ne!(output1.ports["default"]["hash"], output2.ports["default"]["hash"]);
    }

    #[tokio::test]
    async fn test_hash_empty_string() {
        let node = make_node().await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!(""), &ctx).await.unwrap();
        let result = &output.ports["default"];
        assert_eq!(result["algorithm"], json!("fnv1a"));
        // FNV-1a of empty string = offset basis = cbf29ce484222325
        assert_eq!(result["hash"], json!("cbf29ce484222325"));
    }

    #[tokio::test]
    async fn test_hash_data_field_input() {
        let node = make_node().await;
        let ctx = ExecutionContext::new();
        let output = node
            .execute(json!({"data": "hello"}), &ctx)
            .await
            .unwrap();
        // Should produce same hash as direct string input
        let direct_output = node.execute(json!("hello"), &ctx).await.unwrap();
        assert_eq!(
            output.ports["default"]["hash"],
            direct_output.ports["default"]["hash"]
        );
    }

    #[tokio::test]
    async fn test_hash_text_field_input() {
        let node = make_node().await;
        let ctx = ExecutionContext::new();
        let output = node
            .execute(json!({"text": "hello"}), &ctx)
            .await
            .unwrap();
        let direct_output = node.execute(json!("hello"), &ctx).await.unwrap();
        assert_eq!(
            output.ports["default"]["hash"],
            direct_output.ports["default"]["hash"]
        );
    }

    #[tokio::test]
    async fn test_hash_non_string_input_fails() {
        let node = make_node().await;
        let ctx = ExecutionContext::new();
        let result = node.execute(json!(42), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_init_rejects_sha256() {
        let mut node = HashNode::new();
        let result = node.init(&json!({"algorithm": "sha256"})).await;
        assert!(result.is_err());
    }
}
