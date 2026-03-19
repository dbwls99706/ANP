use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Translate node — simple text transformation/transliteration.
///
/// Config:
/// ```json
/// { "operation": "rot13", "field": "text" }
/// ```
///
/// Operations: reverse, pig_latin, rot13, morse, leetspeak
pub struct TranslateNode {
    operation: String,
    field: String,
}

impl Default for TranslateNode {
    fn default() -> Self {
        Self::new()
    }
}

impl TranslateNode {
    pub fn new() -> Self {
        Self {
            operation: String::new(),
            field: "text".to_string(),
        }
    }

    fn resolve_field<'a>(&self, input: &'a serde_json::Value) -> Option<&'a str> {
        let mut current = input;
        for part in self.field.split('.') {
            current = current.get(part)?;
        }
        current.as_str()
    }

    fn reverse(text: &str) -> String {
        text.chars().rev().collect()
    }

    fn pig_latin(text: &str) -> String {
        text.split_whitespace()
            .map(|word| {
                if word.is_empty() {
                    return word.to_string();
                }
                let lower = word.to_lowercase();
                let chars: Vec<char> = lower.chars().collect();
                let vowels = ['a', 'e', 'i', 'o', 'u'];
                if vowels.contains(&chars[0]) {
                    format!("{}yay", lower)
                } else {
                    // Find the first vowel position
                    let vowel_pos = chars.iter().position(|c| vowels.contains(c));
                    match vowel_pos {
                        Some(pos) => {
                            let consonant_cluster: String = chars[..pos].iter().collect();
                            let rest: String = chars[pos..].iter().collect();
                            format!("{}{}ay", rest, consonant_cluster)
                        }
                        None => {
                            // No vowel found — just append "ay"
                            format!("{}ay", lower)
                        }
                    }
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn rot13(text: &str) -> String {
        text.chars()
            .map(|c| match c {
                'a'..='m' | 'A'..='M' => (c as u8 + 13) as char,
                'n'..='z' | 'N'..='Z' => (c as u8 - 13) as char,
                _ => c,
            })
            .collect()
    }

    fn morse(text: &str) -> String {
        text.chars()
            .map(|c| match c.to_ascii_uppercase() {
                'A' => ".-",
                'B' => "-...",
                'C' => "-.-.",
                'D' => "-..",
                'E' => ".",
                'F' => "..-.",
                'G' => "--.",
                'H' => "....",
                'I' => "..",
                'J' => ".---",
                'K' => "-.-",
                'L' => ".-..",
                'M' => "--",
                'N' => "-.",
                'O' => "---",
                'P' => ".--.",
                'Q' => "--.-",
                'R' => ".-.",
                'S' => "...",
                'T' => "-",
                'U' => "..-",
                'V' => "...-",
                'W' => ".--",
                'X' => "-..-",
                'Y' => "-.--",
                'Z' => "--..",
                '0' => "-----",
                '1' => ".----",
                '2' => "..---",
                '3' => "...--",
                '4' => "....-",
                '5' => ".....",
                '6' => "-....",
                '7' => "--...",
                '8' => "---..",
                '9' => "----.",
                ' ' => "/",
                _ => "",
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn leetspeak(text: &str) -> String {
        text.chars()
            .map(|c| match c.to_ascii_lowercase() {
                'a' => '4',
                'e' => '3',
                'i' => '1',
                'o' => '0',
                's' => '5',
                't' => '7',
                _ => c,
            })
            .collect()
    }
}

#[async_trait::async_trait]
impl NodeExecutor for TranslateNode {
    fn node_type(&self) -> &str {
        "translate"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(field) = config.get("field").and_then(|v| v.as_str()) {
            self.field = field.to_string();
        }
        match config.get("operation").and_then(|v| v.as_str()) {
            Some(op @ ("reverse" | "pig_latin" | "rot13" | "morse" | "leetspeak")) => {
                self.operation = op.to_string();
            }
            Some(other) => {
                return Err(NodeError::InvalidConfig(format!(
                    "unknown translate operation: '{}'. Use: reverse, pig_latin, rot13, morse, leetspeak",
                    other
                )));
            }
            None => {
                return Err(NodeError::InvalidConfig(
                    "operation is required for translate node".into(),
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
            NodeError::ExecutionFailed(format!(
                "field '{}' not found or not a string",
                self.field
            ))
        })?;

        let result = match self.operation.as_str() {
            "reverse" => Self::reverse(text),
            "pig_latin" => Self::pig_latin(text),
            "rot13" => Self::rot13(text),
            "morse" => Self::morse(text),
            "leetspeak" => Self::leetspeak(text),
            _ => {
                return Err(NodeError::ExecutionFailed(format!(
                    "unknown operation: '{}'",
                    self.operation
                )));
            }
        };

        Ok(NodeOutput::data(serde_json::json!({
            "result": result,
            "operation": self.operation,
            "original": text,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_reverse() {
        let mut node = TranslateNode::new();
        node.init(&serde_json::json!({"operation": "reverse"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"text": "hello"}), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["result"], "olleh");
    }

    #[tokio::test]
    async fn test_rot13() {
        let mut node = TranslateNode::new();
        node.init(&serde_json::json!({"operation": "rot13"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"text": "Hello"}), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["result"], "Uryyb");
        // Double ROT13 should return original
        let output2 = node
            .execute(serde_json::json!({"text": "Uryyb"}), &ctx)
            .await
            .unwrap();
        assert_eq!(output2.ports["default"]["result"], "Hello");
    }

    #[tokio::test]
    async fn test_pig_latin() {
        let mut node = TranslateNode::new();
        node.init(&serde_json::json!({"operation": "pig_latin"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"text": "hello apple"}), &ctx)
            .await
            .unwrap();
        let result = output.ports["default"]["result"].as_str().unwrap();
        assert_eq!(result, "ellohay appleyay");
    }

    #[tokio::test]
    async fn test_leetspeak() {
        let mut node = TranslateNode::new();
        node.init(&serde_json::json!({"operation": "leetspeak"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"text": "elite"}), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["result"], "3l173");
    }

    #[tokio::test]
    async fn test_morse() {
        let mut node = TranslateNode::new();
        node.init(&serde_json::json!({"operation": "morse"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"text": "SOS"}), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["result"], "... --- ...");
    }

    #[tokio::test]
    async fn test_missing_operation() {
        let mut node = TranslateNode::new();
        let result = node.init(&serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_node_type_and_kind() {
        let node = TranslateNode::new();
        assert_eq!(node.node_type(), "translate");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
