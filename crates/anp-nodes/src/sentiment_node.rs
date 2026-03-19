use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Sentiment node — keyword-based sentiment analysis.
///
/// Config:
/// ```json
/// { "field": "text", "custom_positive": [], "custom_negative": [] }
/// ```
pub struct SentimentNode {
    field: String,
    positive_words: Vec<String>,
    negative_words: Vec<String>,
}

impl Default for SentimentNode {
    fn default() -> Self {
        Self::new()
    }
}

impl SentimentNode {
    pub fn new() -> Self {
        Self {
            field: "text".to_string(),
            positive_words: vec![
                "happy",
                "good",
                "great",
                "excellent",
                "love",
                "wonderful",
                "amazing",
                "fantastic",
                "beautiful",
                "perfect",
                "best",
                "awesome",
                "nice",
                "brilliant",
                "superb",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            negative_words: vec![
                "bad",
                "terrible",
                "awful",
                "hate",
                "horrible",
                "worst",
                "ugly",
                "poor",
                "stupid",
                "boring",
                "annoying",
                "disgusting",
                "dreadful",
                "pathetic",
                "useless",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        }
    }

    fn resolve_field<'a>(&self, input: &'a serde_json::Value) -> Option<&'a str> {
        let mut current = input;
        for part in self.field.split('.') {
            current = current.get(part)?;
        }
        current.as_str()
    }

    fn extract_words(text: &str) -> Vec<String> {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(|w| w.to_lowercase())
            .collect()
    }
}

#[async_trait::async_trait]
impl NodeExecutor for SentimentNode {
    fn node_type(&self) -> &str {
        "sentiment"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(field) = config.get("field").and_then(|v| v.as_str()) {
            self.field = field.to_string();
        }
        if let Some(arr) = config.get("custom_positive").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(word) = item.as_str() {
                    self.positive_words.push(word.to_lowercase());
                }
            }
        }
        if let Some(arr) = config.get("custom_negative").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(word) = item.as_str() {
                    self.negative_words.push(word.to_lowercase());
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
        let text = self.resolve_field(&input).ok_or_else(|| {
            NodeError::ExecutionFailed(format!("field '{}' not found or not a string", self.field))
        })?;

        let words = Self::extract_words(text);
        let word_count = words.len();

        let mut positive_count = 0u64;
        let mut negative_count = 0u64;

        for word in &words {
            if self.positive_words.contains(word) {
                positive_count += 1;
            }
            if self.negative_words.contains(word) {
                negative_count += 1;
            }
        }

        let total = positive_count + negative_count;
        let score = if total == 0 {
            0.0
        } else {
            (positive_count as f64 - negative_count as f64) / total as f64
        };

        let sentiment = if score > 0.1 {
            "positive"
        } else if score < -0.1 {
            "negative"
        } else {
            "neutral"
        };

        let result = serde_json::json!({
            "sentiment": sentiment,
            "score": score,
            "positive_count": positive_count,
            "negative_count": negative_count,
            "word_count": word_count,
        });
        Ok(NodeOutput::data(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_positive_sentiment() {
        let mut node = SentimentNode::new();
        node.init(&serde_json::json!({})).await.unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({"text": "This is a great and amazing day"}),
                &ctx,
            )
            .await
            .unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["sentiment"], "positive");
        assert!(data["score"].as_f64().unwrap() > 0.1);
        assert_eq!(data["positive_count"], 2);
        assert_eq!(data["negative_count"], 0);
    }

    #[tokio::test]
    async fn test_negative_sentiment() {
        let mut node = SentimentNode::new();
        node.init(&serde_json::json!({})).await.unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({"text": "This is terrible and awful"}),
                &ctx,
            )
            .await
            .unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["sentiment"], "negative");
        assert!(data["score"].as_f64().unwrap() < -0.1);
    }

    #[tokio::test]
    async fn test_neutral_sentiment() {
        let mut node = SentimentNode::new();
        node.init(&serde_json::json!({})).await.unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"text": "The cat sat on the mat"}), &ctx)
            .await
            .unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["sentiment"], "neutral");
        assert_eq!(data["score"].as_f64().unwrap(), 0.0);
    }

    #[tokio::test]
    async fn test_mixed_sentiment() {
        let mut node = SentimentNode::new();
        node.init(&serde_json::json!({})).await.unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"text": "great but terrible"}), &ctx)
            .await
            .unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["sentiment"], "neutral");
        assert_eq!(data["positive_count"], 1);
        assert_eq!(data["negative_count"], 1);
    }

    #[tokio::test]
    async fn test_custom_words() {
        let mut node = SentimentNode::new();
        node.init(&serde_json::json!({
            "custom_positive": ["rad"],
            "custom_negative": ["meh"]
        }))
        .await
        .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"text": "That was rad"}), &ctx)
            .await
            .unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["sentiment"], "positive");
    }

    #[tokio::test]
    async fn test_custom_field() {
        let mut node = SentimentNode::new();
        node.init(&serde_json::json!({"field": "body"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"body": "amazing wonderful"}), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["sentiment"], "positive");
    }

    #[tokio::test]
    async fn test_node_type_and_kind() {
        let node = SentimentNode::new();
        assert_eq!(node.node_type(), "sentiment");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
