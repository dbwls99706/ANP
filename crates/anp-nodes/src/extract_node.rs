use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Extract node — pattern-based data extraction from text.
///
/// Config:
/// ```json
/// { "extractors": ["emails", "urls", "numbers"], "field": "text" }
/// ```
///
/// Built-in extractors: emails, urls, numbers, dates, mentions, hashtags, ip_addresses
pub struct ExtractNode {
    field: String,
    extractors: Vec<String>,
}

const ALL_EXTRACTORS: &[&str] = &[
    "emails",
    "urls",
    "numbers",
    "dates",
    "mentions",
    "hashtags",
    "ip_addresses",
];

impl Default for ExtractNode {
    fn default() -> Self {
        Self::new()
    }
}

impl ExtractNode {
    pub fn new() -> Self {
        Self {
            field: "text".to_string(),
            extractors: ALL_EXTRACTORS.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn resolve_field<'a>(&self, input: &'a serde_json::Value) -> Option<&'a str> {
        let mut current = input;
        for part in self.field.split('.') {
            current = current.get(part)?;
        }
        current.as_str()
    }

    fn extract_emails(text: &str) -> Vec<String> {
        text.split_whitespace()
            .filter(|token| {
                let t = token.trim_matches(|c: char| {
                    !c.is_alphanumeric() && c != '@' && c != '.' && c != '_' && c != '-' && c != '+'
                });
                if let Some(at_pos) = t.find('@') {
                    let before = &t[..at_pos];
                    let after = &t[at_pos + 1..];
                    !before.is_empty() && !after.is_empty() && after.contains('.')
                } else {
                    false
                }
            })
            .map(|t| {
                t.trim_matches(|c: char| {
                    !c.is_alphanumeric() && c != '@' && c != '.' && c != '_' && c != '-' && c != '+'
                })
                .to_string()
            })
            .collect()
    }

    fn extract_urls(text: &str) -> Vec<String> {
        text.split_whitespace()
            .filter(|token| {
                let t = token.to_lowercase();
                t.starts_with("http://") || t.starts_with("https://")
            })
            .map(|t| t.to_string())
            .collect()
    }

    fn extract_numbers(text: &str) -> Vec<f64> {
        text.split(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
            .filter(|w| !w.is_empty())
            .filter_map(|w| w.parse::<f64>().ok())
            .collect()
    }

    fn extract_dates(text: &str) -> Vec<String> {
        text.split_whitespace()
            .filter_map(|token| {
                let t = token.trim_matches(|c: char| !c.is_ascii_digit());
                if t.len() == 10 {
                    let parts: Vec<&str> = t.split('-').collect();
                    if parts.len() == 3
                        && parts[0].len() == 4
                        && parts[1].len() == 2
                        && parts[2].len() == 2
                        && parts[0].chars().all(|c| c.is_ascii_digit())
                        && parts[1].chars().all(|c| c.is_ascii_digit())
                        && parts[2].chars().all(|c| c.is_ascii_digit())
                    {
                        return Some(t.to_string());
                    }
                }
                None
            })
            .collect()
    }

    fn extract_mentions(text: &str) -> Vec<String> {
        text.split_whitespace()
            .filter_map(|token| {
                if token.starts_with('@') && token.len() > 1 {
                    let name: String = token[1..]
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !name.is_empty() {
                        return Some(format!("@{}", name));
                    }
                }
                None
            })
            .collect()
    }

    fn extract_hashtags(text: &str) -> Vec<String> {
        text.split_whitespace()
            .filter_map(|token| {
                if token.starts_with('#') && token.len() > 1 {
                    let tag: String = token[1..]
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !tag.is_empty() {
                        return Some(format!("#{}", tag));
                    }
                }
                None
            })
            .collect()
    }

    fn extract_ip_addresses(text: &str) -> Vec<String> {
        text.split_whitespace()
            .filter_map(|token| {
                let t = token.trim_matches(|c: char| !c.is_ascii_digit() && c != '.');
                let parts: Vec<&str> = t.split('.').collect();
                if parts.len() == 4 {
                    let valid = parts.iter().all(|p| {
                        if let Ok(n) = p.parse::<u16>() {
                            n <= 255 && !p.is_empty()
                        } else {
                            false
                        }
                    });
                    if valid {
                        return Some(t.to_string());
                    }
                }
                None
            })
            .collect()
    }
}

#[async_trait::async_trait]
impl NodeExecutor for ExtractNode {
    fn node_type(&self) -> &str {
        "extract"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(field) = config.get("field").and_then(|v| v.as_str()) {
            self.field = field.to_string();
        }
        if let Some(arr) = config.get("extractors").and_then(|v| v.as_array()) {
            self.extractors.clear();
            for item in arr {
                let name = item.as_str().ok_or_else(|| {
                    NodeError::InvalidConfig("extractor names must be strings".into())
                })?;
                if !ALL_EXTRACTORS.contains(&name) {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown extractor: '{}'. Available: {:?}",
                        name, ALL_EXTRACTORS
                    )));
                }
                self.extractors.push(name.to_string());
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

        let mut result = serde_json::Map::new();
        let mut total_count: usize = 0;

        for extractor in &self.extractors {
            match extractor.as_str() {
                "emails" => {
                    let found = Self::extract_emails(text);
                    total_count += found.len();
                    result.insert("emails".to_string(), serde_json::json!(found));
                }
                "urls" => {
                    let found = Self::extract_urls(text);
                    total_count += found.len();
                    result.insert("urls".to_string(), serde_json::json!(found));
                }
                "numbers" => {
                    let found = Self::extract_numbers(text);
                    total_count += found.len();
                    result.insert("numbers".to_string(), serde_json::json!(found));
                }
                "dates" => {
                    let found = Self::extract_dates(text);
                    total_count += found.len();
                    result.insert("dates".to_string(), serde_json::json!(found));
                }
                "mentions" => {
                    let found = Self::extract_mentions(text);
                    total_count += found.len();
                    result.insert("mentions".to_string(), serde_json::json!(found));
                }
                "hashtags" => {
                    let found = Self::extract_hashtags(text);
                    total_count += found.len();
                    result.insert("hashtags".to_string(), serde_json::json!(found));
                }
                "ip_addresses" => {
                    let found = Self::extract_ip_addresses(text);
                    total_count += found.len();
                    result.insert("ip_addresses".to_string(), serde_json::json!(found));
                }
                _ => {}
            }
        }

        result.insert(
            "extracted_count".to_string(),
            serde_json::json!(total_count),
        );
        Ok(NodeOutput::data(serde_json::Value::Object(result)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_extract_emails() {
        let mut node = ExtractNode::new();
        node.init(&serde_json::json!({"extractors": ["emails"]}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({"text": "Contact us at hello@example.com or support@test.org"}),
                &ctx,
            )
            .await
            .unwrap();
        let data = &output.ports["default"];
        let emails = data["emails"].as_array().unwrap();
        assert_eq!(emails.len(), 2);
        assert_eq!(data["extracted_count"], 2);
    }

    #[tokio::test]
    async fn test_extract_urls() {
        let mut node = ExtractNode::new();
        node.init(&serde_json::json!({"extractors": ["urls"]}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({"text": "Visit https://example.com and http://test.org"}),
                &ctx,
            )
            .await
            .unwrap();
        let urls = output.ports["default"]["urls"].as_array().unwrap();
        assert_eq!(urls.len(), 2);
    }

    #[tokio::test]
    async fn test_extract_dates() {
        let mut node = ExtractNode::new();
        node.init(&serde_json::json!({"extractors": ["dates"]}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({"text": "Born on 1990-01-15 graduated 2012-06-20"}),
                &ctx,
            )
            .await
            .unwrap();
        let dates = output.ports["default"]["dates"].as_array().unwrap();
        assert_eq!(dates.len(), 2);
    }

    #[tokio::test]
    async fn test_extract_mentions_and_hashtags() {
        let mut node = ExtractNode::new();
        node.init(&serde_json::json!({"extractors": ["mentions", "hashtags"]}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({"text": "Hey @alice and @bob check out #rust #programming"}),
                &ctx,
            )
            .await
            .unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["mentions"].as_array().unwrap().len(), 2);
        assert_eq!(data["hashtags"].as_array().unwrap().len(), 2);
        assert_eq!(data["extracted_count"], 4);
    }

    #[tokio::test]
    async fn test_extract_ip_addresses() {
        let mut node = ExtractNode::new();
        node.init(&serde_json::json!({"extractors": ["ip_addresses"]}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({"text": "Server at 192.168.1.1 and 10.0.0.1"}),
                &ctx,
            )
            .await
            .unwrap();
        let ips = output.ports["default"]["ip_addresses"].as_array().unwrap();
        assert_eq!(ips.len(), 2);
    }

    #[tokio::test]
    async fn test_invalid_extractor() {
        let mut node = ExtractNode::new();
        let result = node
            .init(&serde_json::json!({"extractors": ["phone_numbers"]}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_node_type_and_kind() {
        let node = ExtractNode::new();
        assert_eq!(node.node_type(), "extract");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
