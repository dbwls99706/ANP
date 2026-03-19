use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Embedding node — generates simple numeric embeddings from text.
///
/// Config:
/// ```json
/// { "operation": "embed", "dimensions": 26 }
/// ```
///
/// Operations:
/// - `embed`: character frequency analysis → 26-dimensional vector
/// - `similarity`: cosine similarity between two vectors
/// - `distance`: euclidean distance between two vectors
pub struct EmbeddingNode {
    operation: String,
    dimensions: usize,
}

impl Default for EmbeddingNode {
    fn default() -> Self {
        Self::new()
    }
}

impl EmbeddingNode {
    pub fn new() -> Self {
        Self {
            operation: "embed".to_string(),
            dimensions: 26,
        }
    }

    fn embed_text(&self, text: &str) -> Vec<f64> {
        let mut counts = vec![0u64; self.dimensions];
        let lower = text.to_lowercase();
        let mut total = 0u64;
        for ch in lower.chars() {
            if ch.is_ascii_lowercase() {
                let idx = (ch as u8 - b'a') as usize;
                if idx < self.dimensions {
                    counts[idx] += 1;
                    total += 1;
                }
            }
        }
        if total == 0 {
            return vec![0.0; self.dimensions];
        }
        counts.iter().map(|&c| c as f64 / total as f64).collect()
    }

    fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
        let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let mag_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
        let mag_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
        if mag_a == 0.0 || mag_b == 0.0 {
            return 0.0;
        }
        dot / (mag_a * mag_b)
    }

    fn euclidean_distance(a: &[f64], b: &[f64]) -> f64 {
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y) * (x - y))
            .sum::<f64>()
            .sqrt()
    }

    fn parse_vector(value: &serde_json::Value) -> Result<Vec<f64>, NodeError> {
        value
            .as_array()
            .ok_or_else(|| NodeError::ExecutionFailed("expected array for vector".into()))?
            .iter()
            .map(|v| {
                v.as_f64().ok_or_else(|| {
                    NodeError::ExecutionFailed("vector elements must be numbers".into())
                })
            })
            .collect()
    }
}

#[async_trait::async_trait]
impl NodeExecutor for EmbeddingNode {
    fn node_type(&self) -> &str {
        "embedding"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "embed" | "similarity" | "distance" => {
                    self.operation = op.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown embedding operation: '{}'. Use: embed, similarity, distance",
                        other
                    )));
                }
            }
        }
        if let Some(dims) = config.get("dimensions").and_then(|v| v.as_u64()) {
            if dims == 0 || dims > 256 {
                return Err(NodeError::InvalidConfig(
                    "dimensions must be between 1 and 256".into(),
                ));
            }
            self.dimensions = dims as usize;
        }
        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        match self.operation.as_str() {
            "embed" => {
                let text = input.get("text").and_then(|v| v.as_str()).unwrap_or("");
                let embedding = self.embed_text(text);
                let result = serde_json::json!({
                    "embedding": embedding,
                    "dimensions": self.dimensions,
                    "text": text,
                });
                Ok(NodeOutput::data(result))
            }
            "similarity" => {
                let a = Self::parse_vector(input.get("a").ok_or_else(|| {
                    NodeError::ExecutionFailed("similarity requires 'a' vector".into())
                })?)?;
                let b = Self::parse_vector(input.get("b").ok_or_else(|| {
                    NodeError::ExecutionFailed("similarity requires 'b' vector".into())
                })?)?;
                if a.len() != b.len() {
                    return Err(NodeError::ExecutionFailed(
                        "vectors must have same dimensions".into(),
                    ));
                }
                let sim = Self::cosine_similarity(&a, &b);
                Ok(NodeOutput::data(serde_json::json!({ "similarity": sim })))
            }
            "distance" => {
                let a = Self::parse_vector(input.get("a").ok_or_else(|| {
                    NodeError::ExecutionFailed("distance requires 'a' vector".into())
                })?)?;
                let b = Self::parse_vector(input.get("b").ok_or_else(|| {
                    NodeError::ExecutionFailed("distance requires 'b' vector".into())
                })?)?;
                if a.len() != b.len() {
                    return Err(NodeError::ExecutionFailed(
                        "vectors must have same dimensions".into(),
                    ));
                }
                let dist = Self::euclidean_distance(&a, &b);
                Ok(NodeOutput::data(serde_json::json!({ "distance": dist })))
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
    async fn test_embed_basic() {
        let mut node = EmbeddingNode::new();
        node.init(&serde_json::json!({"operation": "embed"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"text": "aab"}), &ctx)
            .await
            .unwrap();
        let data = &output.ports["default"];
        let emb = data["embedding"].as_array().unwrap();
        assert_eq!(emb.len(), 26);
        let a_val = emb[0].as_f64().unwrap();
        let b_val = emb[1].as_f64().unwrap();
        assert!((a_val - 2.0 / 3.0).abs() < 1e-9);
        assert!((b_val - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(data["dimensions"], 26);
    }

    #[tokio::test]
    async fn test_embed_empty() {
        let mut node = EmbeddingNode::new();
        node.init(&serde_json::json!({})).await.unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"text": ""}), &ctx)
            .await
            .unwrap();
        let emb = output.ports["default"]["embedding"].as_array().unwrap();
        assert!(emb.iter().all(|v| v.as_f64().unwrap() == 0.0));
    }

    #[tokio::test]
    async fn test_similarity_identical() {
        let mut node = EmbeddingNode::new();
        node.init(&serde_json::json!({"operation": "similarity"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({"a": [1.0, 2.0, 3.0], "b": [1.0, 2.0, 3.0]}),
                &ctx,
            )
            .await
            .unwrap();
        let sim = output.ports["default"]["similarity"].as_f64().unwrap();
        assert!((sim - 1.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn test_distance_zero() {
        let mut node = EmbeddingNode::new();
        node.init(&serde_json::json!({"operation": "distance"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"a": [1.0, 2.0], "b": [1.0, 2.0]}), &ctx)
            .await
            .unwrap();
        let dist = output.ports["default"]["distance"].as_f64().unwrap();
        assert!(dist.abs() < 1e-9);
    }

    #[tokio::test]
    async fn test_distance_known() {
        let mut node = EmbeddingNode::new();
        node.init(&serde_json::json!({"operation": "distance"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"a": [0.0, 0.0], "b": [3.0, 4.0]}), &ctx)
            .await
            .unwrap();
        let dist = output.ports["default"]["distance"].as_f64().unwrap();
        assert!((dist - 5.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let mut node = EmbeddingNode::new();
        let result = node
            .init(&serde_json::json!({"operation": "cluster"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_node_type_and_kind() {
        let node = EmbeddingNode::new();
        assert_eq!(node.node_type(), "embedding");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
