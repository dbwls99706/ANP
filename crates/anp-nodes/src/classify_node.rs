use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Classify node — rule-based text classification.
///
/// Config:
/// ```json
/// {
///   "field": "text",
///   "categories": { "tech": ["api", "server"], "sports": ["goal", "team"] },
///   "threshold": 1
/// }
/// ```
pub struct ClassifyNode {
    field: String,
    categories: Vec<(String, Vec<String>)>,
    threshold: usize,
}

impl Default for ClassifyNode {
    fn default() -> Self {
        Self::new()
    }
}

impl ClassifyNode {
    pub fn new() -> Self {
        Self {
            field: "text".to_string(),
            categories: Vec::new(),
            threshold: 1,
        }
    }

    fn resolve_field<'a>(&self, input: &'a serde_json::Value) -> Option<&'a str> {
        let mut current = input;
        for part in self.field.split('.') {
            current = current.get(part)?;
        }
        current.as_str()
    }
}

#[async_trait::async_trait]
impl NodeExecutor for ClassifyNode {
    fn node_type(&self) -> &str {
        "classify"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(f) = config.get("field").and_then(|v| v.as_str()) {
            self.field = f.to_string();
        }
        if let Some(t) = config.get("threshold").and_then(|v| v.as_u64()) {
            self.threshold = t as usize;
        }
        match config.get("categories").and_then(|v| v.as_object()) {
            Some(cats) => {
                self.categories.clear();
                for (name, keywords) in cats {
                    let kws: Vec<String> = keywords
                        .as_array()
                        .unwrap_or(&Vec::new())
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_lowercase()))
                        .collect();
                    self.categories.push((name.clone(), kws));
                }
            }
            None => {
                return Err(NodeError::InvalidConfig(
                    "classify node requires 'categories' config".into(),
                ));
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

        let text_lower = text.to_lowercase();
        let words: Vec<&str> = text_lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();
        let text_length = text.len();

        let mut matches: Vec<serde_json::Value> = Vec::new();

        for (category, keywords) in &self.categories {
            let matched_keywords: Vec<String> = keywords
                .iter()
                .filter(|kw| words.contains(&kw.as_str()))
                .cloned()
                .collect();
            let count = matched_keywords.len();
            if count >= self.threshold {
                matches.push(serde_json::json!({
                    "category": category,
                    "count": count,
                    "keywords": matched_keywords,
                }));
            }
        }

        // Sort by count descending
        matches.sort_by(|a, b| {
            b["count"]
                .as_u64()
                .unwrap_or(0)
                .cmp(&a["count"].as_u64().unwrap_or(0))
        });

        let top_category = matches
            .first()
            .and_then(|m| m["category"].as_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(NodeOutput::data(serde_json::json!({
            "matches": matches,
            "top_category": top_category,
            "text_length": text_length,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_classify_single_category() {
        let mut node = ClassifyNode::new();
        node.init(&serde_json::json!({
            "categories": {
                "tech": ["api", "server", "database"]
            }
        }))
        .await
        .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({"text": "The api connects to a database"}),
                &ctx,
            )
            .await
            .unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["top_category"], "tech");
        let matches = data["matches"].as_array().unwrap();
        assert_eq!(matches[0]["count"], 2);
    }

    #[tokio::test]
    async fn test_classify_multiple_categories() {
        let mut node = ClassifyNode::new();
        node.init(&serde_json::json!({
            "categories": {
                "tech": ["api", "server"],
                "sports": ["goal", "team", "match"]
            }
        }))
        .await
        .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({"text": "The team scored a goal in the match"}),
                &ctx,
            )
            .await
            .unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["top_category"], "sports");
    }

    #[tokio::test]
    async fn test_threshold() {
        let mut node = ClassifyNode::new();
        node.init(&serde_json::json!({
            "categories": { "tech": ["api", "server"] },
            "threshold": 2
        }))
        .await
        .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"text": "Just an api call"}), &ctx)
            .await
            .unwrap();
        let data = &output.ports["default"];
        let matches = data["matches"].as_array().unwrap();
        assert!(matches.is_empty());
        assert_eq!(data["top_category"], "unknown");
    }

    #[tokio::test]
    async fn test_missing_categories() {
        let mut node = ClassifyNode::new();
        let result = node.init(&serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_missing_text_field() {
        let mut node = ClassifyNode::new();
        node.init(&serde_json::json!({"categories": {"a": ["b"]}}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let result = node.execute(serde_json::json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_node_type_and_kind() {
        let node = ClassifyNode::new();
        assert_eq!(node.node_type(), "classify");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }

    #[tokio::test]
    async fn test_case_insensitive() {
        let mut node = ClassifyNode::new();
        node.init(&serde_json::json!({
            "categories": { "tech": ["api"] }
        }))
        .await
        .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"text": "Using the API here"}), &ctx)
            .await
            .unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["top_category"], "tech");
    }
}
