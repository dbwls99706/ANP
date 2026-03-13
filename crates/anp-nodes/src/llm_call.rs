use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// LLM Call node — invokes an LLM (OpenAI or Anthropic) as a graph node.
///
/// This is the heart of "AI Native Programming": AI calling AI within a graph.
///
/// Config example:
/// ```json
/// {
///   "provider": "openai",
///   "model": "gpt-4o-mini",
///   "system_prompt": "You are a helpful assistant.",
///   "temperature": 0.7,
///   "max_tokens": 1024,
///   "timeout_secs": 60
/// }
/// ```
///
/// Input: a JSON value. If it has a "prompt" field, that's used as the user message.
/// If it's a plain string, the string is used directly.
/// Otherwise the entire input is serialized as the user message.
///
/// Output: `{ "response": "...", "model": "...", "usage": { ... } }`
///
/// Security: API keys are resolved exclusively from environment variables
/// (OPENAI_API_KEY or ANTHROPIC_API_KEY). Keys are never stored in config.
pub struct LlmCallNode {
    provider: LlmProvider,
    model: String,
    system_prompt: String,
    temperature: f64,
    max_tokens: u32,
    timeout_secs: u64,
}

#[derive(Debug, Clone)]
enum LlmProvider {
    OpenAI,
    Anthropic,
}

impl std::fmt::Display for LlmProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmProvider::OpenAI => write!(f, "openai"),
            LlmProvider::Anthropic => write!(f, "anthropic"),
        }
    }
}

impl Default for LlmCallNode {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmCallNode {
    pub fn new() -> Self {
        Self {
            provider: LlmProvider::OpenAI,
            model: "gpt-4o-mini".to_string(),
            system_prompt: String::new(),
            temperature: 0.7,
            max_tokens: 1024,
            timeout_secs: 60,
        }
    }

    /// Resolve API key from environment variables only.
    fn resolve_api_key(&self) -> Result<String, NodeError> {
        let (var_name, provider_name) = match self.provider {
            LlmProvider::OpenAI => ("OPENAI_API_KEY", "OpenAI"),
            LlmProvider::Anthropic => ("ANTHROPIC_API_KEY", "Anthropic"),
        };
        std::env::var(var_name).map_err(|_| {
            NodeError::InitFailed(format!(
                "{} not set — required for {} provider",
                var_name, provider_name
            ))
        })
    }

    fn build_client(&self) -> Result<reqwest::Client, NodeError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .build()
            .map_err(|e| NodeError::ExecutionFailed(format!("failed to build HTTP client: {}", e)))
    }

    async fn call_openai(
        &self,
        user_message: &str,
        api_key: &str,
    ) -> Result<serde_json::Value, NodeError> {
        let client = self.build_client()?;

        let mut messages = Vec::new();
        if !self.system_prompt.is_empty() {
            messages.push(serde_json::json!({
                "role": "system",
                "content": self.system_prompt
            }));
        }
        messages.push(serde_json::json!({
            "role": "user",
            "content": user_message
        }));

        let body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "temperature": self.temperature,
            "max_tokens": self.max_tokens
        });

        let resp = client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| NodeError::ExecutionFailed(format!("HTTP request failed: {}", e)))?;

        let status = resp.status();
        let resp_body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| NodeError::ExecutionFailed(format!("failed to parse response: {}", e)))?;

        if !status.is_success() {
            let err_msg = resp_body["error"]["message"]
                .as_str()
                .unwrap_or("unknown API error");
            return Err(NodeError::ExecutionFailed(format!(
                "OpenAI API error ({}): {}",
                status, err_msg
            )));
        }

        // Validate expected response structure
        let content = resp_body
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| {
                NodeError::ExecutionFailed(
                    "OpenAI response missing expected field: choices[0].message.content".into(),
                )
            })?
            .to_string();

        let usage = resp_body.get("usage").cloned().unwrap_or(serde_json::Value::Null);

        Ok(serde_json::json!({
            "response": content,
            "model": self.model,
            "usage": usage
        }))
    }

    async fn call_anthropic(
        &self,
        user_message: &str,
        api_key: &str,
    ) -> Result<serde_json::Value, NodeError> {
        let client = self.build_client()?;

        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "messages": [{
                "role": "user",
                "content": user_message
            }]
        });

        if !self.system_prompt.is_empty() {
            body["system"] = serde_json::Value::String(self.system_prompt.clone());
        }

        let resp = client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| NodeError::ExecutionFailed(format!("HTTP request failed: {}", e)))?;

        let status = resp.status();
        let resp_body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| NodeError::ExecutionFailed(format!("failed to parse response: {}", e)))?;

        if !status.is_success() {
            let err_msg = resp_body["error"]["message"]
                .as_str()
                .unwrap_or("unknown API error");
            return Err(NodeError::ExecutionFailed(format!(
                "Anthropic API error ({}): {}",
                status, err_msg
            )));
        }

        // Validate expected response structure
        let content = resp_body
            .get("content")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("text"))
            .and_then(|t| t.as_str())
            .ok_or_else(|| {
                NodeError::ExecutionFailed(
                    "Anthropic response missing expected field: content[0].text".into(),
                )
            })?
            .to_string();

        let usage = resp_body.get("usage").cloned().unwrap_or(serde_json::Value::Null);

        Ok(serde_json::json!({
            "response": content,
            "model": self.model,
            "usage": usage
        }))
    }
}

#[async_trait::async_trait]
impl NodeExecutor for LlmCallNode {
    fn node_type(&self) -> &str {
        "llm_call"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        // Provider
        let provider_str = config
            .get("provider")
            .and_then(|v| v.as_str())
            .unwrap_or("openai");
        self.provider = match provider_str {
            "openai" => LlmProvider::OpenAI,
            "anthropic" => LlmProvider::Anthropic,
            other => {
                return Err(NodeError::InvalidConfig(format!(
                    "unknown provider: '{}'. Use: openai, anthropic",
                    other
                )))
            }
        };

        // Model
        if let Some(model) = config.get("model").and_then(|v| v.as_str()) {
            if model.is_empty() {
                return Err(NodeError::InvalidConfig(
                    "model name must not be empty".into(),
                ));
            }
            self.model = model.to_string();
        } else {
            self.model = match self.provider {
                LlmProvider::OpenAI => "gpt-4o-mini".to_string(),
                LlmProvider::Anthropic => "claude-sonnet-4-20250514".to_string(),
            };
        }

        // System prompt
        if let Some(sp) = config.get("system_prompt").and_then(|v| v.as_str()) {
            self.system_prompt = sp.to_string();
        }

        // Temperature
        if let Some(t) = config.get("temperature").and_then(|v| v.as_f64()) {
            if !(0.0..=2.0).contains(&t) || t.is_nan() {
                return Err(NodeError::InvalidConfig(
                    "temperature must be between 0.0 and 2.0".into(),
                ));
            }
            self.temperature = t;
        }

        // Max tokens
        if let Some(mt) = config.get("max_tokens").and_then(|v| v.as_u64()) {
            if mt == 0 || mt > 128_000 {
                return Err(NodeError::InvalidConfig(
                    "max_tokens must be between 1 and 128000".into(),
                ));
            }
            self.max_tokens = mt as u32;
        }

        // Timeout
        if let Some(timeout) = config.get("timeout_secs").and_then(|v| v.as_u64()) {
            if timeout == 0 || timeout > 300 {
                return Err(NodeError::InvalidConfig(
                    "timeout_secs must be between 1 and 300".into(),
                ));
            }
            self.timeout_secs = timeout;
        }

        // Reject api_key in config — must use env vars
        if config.get("api_key").is_some() {
            return Err(NodeError::InvalidConfig(
                "api_key must not be set in config — use OPENAI_API_KEY or ANTHROPIC_API_KEY environment variables".into(),
            ));
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        // Extract user message from input
        let user_message = if let Some(prompt) = input.get("prompt").and_then(|v| v.as_str()) {
            prompt.to_string()
        } else if let Some(s) = input.as_str() {
            s.to_string()
        } else {
            serde_json::to_string(&input)
                .map_err(|e| NodeError::ExecutionFailed(format!("failed to serialize input: {}", e)))?
        };

        if user_message.is_empty() {
            return Err(NodeError::MissingInput(
                "empty prompt — provide a 'prompt' field or string input".into(),
            ));
        }

        let api_key = self.resolve_api_key()?;

        let result = match self.provider {
            LlmProvider::OpenAI => self.call_openai(&user_message, &api_key).await?,
            LlmProvider::Anthropic => self.call_anthropic(&user_message, &api_key).await?,
        };

        Ok(NodeOutput::data(result).with_log(format!(
            "LLM call to {} model={}",
            self.provider, self.model
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_init_defaults() {
        let mut node = LlmCallNode::new();
        node.init(&serde_json::json!({})).await.unwrap();
        assert_eq!(node.model, "gpt-4o-mini");
        assert_eq!(node.timeout_secs, 60);
    }

    #[tokio::test]
    async fn test_init_anthropic() {
        let mut node = LlmCallNode::new();
        node.init(&serde_json::json!({
            "provider": "anthropic",
            "model": "claude-sonnet-4-20250514",
            "system_prompt": "You are a pirate.",
            "temperature": 0.5,
            "max_tokens": 512
        }))
        .await
        .unwrap();
        assert_eq!(node.model, "claude-sonnet-4-20250514");
        assert_eq!(node.system_prompt, "You are a pirate.");
        assert_eq!(node.max_tokens, 512);
    }

    #[tokio::test]
    async fn test_init_invalid_provider() {
        let mut node = LlmCallNode::new();
        let result = node.init(&serde_json::json!({ "provider": "gemini" })).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_init_invalid_temperature() {
        let mut node = LlmCallNode::new();
        let result = node.init(&serde_json::json!({ "temperature": 3.0 })).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_init_invalid_max_tokens() {
        let mut node = LlmCallNode::new();
        let result = node.init(&serde_json::json!({ "max_tokens": 0 })).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_init_rejects_api_key_in_config() {
        let mut node = LlmCallNode::new();
        let result = node
            .init(&serde_json::json!({ "api_key": "sk-secret" }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_init_empty_model_rejected() {
        let mut node = LlmCallNode::new();
        let result = node.init(&serde_json::json!({ "model": "" })).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_init_invalid_timeout() {
        let mut node = LlmCallNode::new();
        let result = node.init(&serde_json::json!({ "timeout_secs": 0 })).await;
        assert!(result.is_err());
        let result = node.init(&serde_json::json!({ "timeout_secs": 999 })).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_provider_display() {
        assert_eq!(format!("{}", LlmProvider::OpenAI), "openai");
        assert_eq!(format!("{}", LlmProvider::Anthropic), "anthropic");
    }
}
