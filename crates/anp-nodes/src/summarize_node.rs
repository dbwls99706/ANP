use std::collections::HashMap;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Summarize node — extractive text summarization.
///
/// Config:
/// ```json
/// { "operation": "sentences", "max_sentences": 3, "max_length": 200, "top_n": 10 }
/// ```
///
/// Operations:
/// - `sentences`: extract first N sentences
/// - `truncate`: truncate to max chars with "..." suffix
/// - `keywords`: extract most frequent words excluding stop words
pub struct SummarizeNode {
    operation: String,
    max_sentences: usize,
    max_length: usize,
    top_n: usize,
}

impl Default for SummarizeNode {
    fn default() -> Self {
        Self::new()
    }
}

const STOP_WORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
    "do", "does", "did", "will", "would", "could", "should", "may", "might", "must", "shall",
    "can", "to", "of", "in", "for", "on", "with", "at", "by", "from", "as", "into", "about",
    "it", "its", "this", "that", "these", "those", "and", "but", "or", "not", "no", "if", "then",
    "so", "than", "too", "very", "just", "also", "only", "up", "out", "how", "all", "each",
    "every", "both", "few", "more", "most", "other", "some", "such", "what", "which", "who",
    "when", "where", "why", "i", "me", "my", "we", "our", "you", "your", "he", "him", "his",
    "she", "her", "they", "them", "their",
];

impl SummarizeNode {
    pub fn new() -> Self {
        Self {
            operation: "sentences".to_string(),
            max_sentences: 3,
            max_length: 200,
            top_n: 10,
        }
    }

    fn extract_sentences(text: &str, max: usize) -> String {
        let mut sentences = Vec::new();
        let mut start = 0;
        let chars: Vec<char> = text.chars().collect();
        let len = chars.len();

        for i in 0..len {
            if (chars[i] == '.' || chars[i] == '!' || chars[i] == '?')
                && (i + 1 >= len || chars[i + 1].is_whitespace())
            {
                let end = i + 1;
                let sentence: String = chars[start..end].iter().collect();
                let trimmed = sentence.trim();
                if !trimmed.is_empty() {
                    sentences.push(trimmed.to_string());
                }
                start = end;
                if sentences.len() >= max {
                    break;
                }
            }
        }

        if sentences.is_empty() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                sentences.push(trimmed.to_string());
            }
        }

        sentences.join(" ")
    }

    fn truncate_text(text: &str, max_length: usize) -> String {
        if text.len() <= max_length {
            return text.to_string();
        }
        if max_length <= 3 {
            return "...".to_string();
        }
        let truncated: String = text.chars().take(max_length - 3).collect();
        format!("{}...", truncated)
    }

    fn extract_keywords(text: &str, top_n: usize) -> (Vec<(String, usize)>, usize) {
        let words: Vec<String> = text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(|w| w.to_lowercase())
            .collect();
        let total_words = words.len();

        let mut freq: HashMap<String, usize> = HashMap::new();
        for word in &words {
            if word.len() < 2 {
                continue;
            }
            if STOP_WORDS.contains(&word.as_str()) {
                continue;
            }
            *freq.entry(word.clone()).or_insert(0) += 1;
        }

        let mut sorted: Vec<(String, usize)> = freq.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        sorted.truncate(top_n);

        (sorted, total_words)
    }
}

#[async_trait::async_trait]
impl NodeExecutor for SummarizeNode {
    fn node_type(&self) -> &str {
        "summarize"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "sentences" | "truncate" | "keywords" => {
                    self.operation = op.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown summarize operation: '{}'. Use: sentences, truncate, keywords",
                        other
                    )));
                }
            }
        }
        if let Some(n) = config.get("max_sentences").and_then(|v| v.as_u64()) {
            self.max_sentences = n as usize;
        }
        if let Some(n) = config.get("max_length").and_then(|v| v.as_u64()) {
            self.max_length = n as usize;
        }
        if let Some(n) = config.get("top_n").and_then(|v| v.as_u64()) {
            self.top_n = n as usize;
        }
        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let text = input
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| NodeError::ExecutionFailed("missing 'text' field".into()))?;

        let original_length = text.len();

        match self.operation.as_str() {
            "sentences" => {
                let summary = Self::extract_sentences(text, self.max_sentences);
                let summary_length = summary.len();
                Ok(NodeOutput::data(serde_json::json!({
                    "summary": summary,
                    "original_length": original_length,
                    "summary_length": summary_length,
                })))
            }
            "truncate" => {
                let summary = Self::truncate_text(text, self.max_length);
                let summary_length = summary.len();
                Ok(NodeOutput::data(serde_json::json!({
                    "summary": summary,
                    "original_length": original_length,
                    "summary_length": summary_length,
                })))
            }
            "keywords" => {
                let (keywords, total_words) = Self::extract_keywords(text, self.top_n);
                let kw_json: Vec<serde_json::Value> = keywords
                    .into_iter()
                    .map(|(word, count)| serde_json::json!({"word": word, "count": count}))
                    .collect();
                Ok(NodeOutput::data(serde_json::json!({
                    "keywords": kw_json,
                    "total_words": total_words,
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
    async fn test_sentences_basic() {
        let mut node = SummarizeNode::new();
        node.init(&serde_json::json!({"operation": "sentences", "max_sentences": 2}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({"text": "First sentence. Second sentence. Third sentence."}),
                &ctx,
            )
            .await
            .unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["summary"], "First sentence. Second sentence.");
    }

    #[tokio::test]
    async fn test_truncate() {
        let mut node = SummarizeNode::new();
        node.init(&serde_json::json!({"operation": "truncate", "max_length": 10}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({"text": "Hello World, this is long"}),
                &ctx,
            )
            .await
            .unwrap();
        let summary = output.ports["default"]["summary"].as_str().unwrap();
        assert!(summary.ends_with("..."));
        assert!(summary.len() <= 10);
    }

    #[tokio::test]
    async fn test_truncate_short_text() {
        let mut node = SummarizeNode::new();
        node.init(&serde_json::json!({"operation": "truncate", "max_length": 100}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"text": "Short"}), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["summary"], "Short");
    }

    #[tokio::test]
    async fn test_keywords() {
        let mut node = SummarizeNode::new();
        node.init(&serde_json::json!({"operation": "keywords", "top_n": 3}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({"text": "rust is great and rust is fast and rust is safe"}),
                &ctx,
            )
            .await
            .unwrap();
        let data = &output.ports["default"];
        let keywords = data["keywords"].as_array().unwrap();
        assert!(!keywords.is_empty());
        assert_eq!(keywords[0]["word"], "rust");
    }

    #[tokio::test]
    async fn test_missing_text() {
        let mut node = SummarizeNode::new();
        node.init(&serde_json::json!({})).await.unwrap();
        let ctx = ExecutionContext::new();
        let result = node.execute(serde_json::json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let mut node = SummarizeNode::new();
        let result = node
            .init(&serde_json::json!({"operation": "compress"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_node_type_and_kind() {
        let node = SummarizeNode::new();
        assert_eq!(node.node_type(), "summarize");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
