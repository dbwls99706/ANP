use crate::error::{PlannerError, PlannerResult};
use tracing::debug;

/// Trait for LLM API providers.
#[async_trait::async_trait]
pub trait LlmClient: Send + Sync {
    /// Generate a completion from a system prompt and user message.
    async fn generate(&self, system: &str, user: &str) -> PlannerResult<String>;

    /// Provider name for logging.
    fn provider(&self) -> &str;
}

/// Claude (Anthropic) API client.
pub struct ClaudeClient {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl ClaudeClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            model: "claude-sonnet-4-20250514".to_string(),
            client: reqwest::Client::new(),
        }
    }

    pub fn with_model(mut self, model: String) -> Self {
        self.model = model;
        self
    }
}

#[async_trait::async_trait]
impl LlmClient for ClaudeClient {
    async fn generate(&self, system: &str, user: &str) -> PlannerResult<String> {
        debug!(model = %self.model, "calling Claude API");

        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 4096,
            "system": system,
            "messages": [
                { "role": "user", "content": user }
            ]
        });

        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;

        if !status.is_success() {
            return Err(PlannerError::LlmError(format!(
                "Claude API returned {}: {}",
                status, text
            )));
        }

        // Parse response to extract text content
        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| PlannerError::LlmError(e.to_string()))?;

        let content = json["content"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|block| block["text"].as_str())
            .ok_or_else(|| PlannerError::LlmError("no text in Claude response".to_string()))?;

        Ok(content.to_string())
    }

    fn provider(&self) -> &str {
        "anthropic"
    }
}

/// OpenAI API client.
pub struct OpenAiClient {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl OpenAiClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            model: "gpt-4o".to_string(),
            client: reqwest::Client::new(),
        }
    }

    pub fn with_model(mut self, model: String) -> Self {
        self.model = model;
        self
    }
}

#[async_trait::async_trait]
impl LlmClient for OpenAiClient {
    async fn generate(&self, system: &str, user: &str) -> PlannerResult<String> {
        debug!(model = %self.model, "calling OpenAI API");

        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 4096,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user }
            ]
        });

        let resp = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;

        if !status.is_success() {
            return Err(PlannerError::LlmError(format!(
                "OpenAI API returned {}: {}",
                status, text
            )));
        }

        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| PlannerError::LlmError(e.to_string()))?;

        let content = json["choices"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|choice| choice["message"]["content"].as_str())
            .ok_or_else(|| PlannerError::LlmError("no content in OpenAI response".to_string()))?;

        Ok(content.to_string())
    }

    fn provider(&self) -> &str {
        "openai"
    }
}

/// Mock LLM client for testing (returns a pre-configured response).
pub struct MockLlmClient {
    response: String,
}

impl MockLlmClient {
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
        }
    }
}

#[async_trait::async_trait]
impl LlmClient for MockLlmClient {
    async fn generate(&self, _system: &str, _user: &str) -> PlannerResult<String> {
        Ok(self.response.clone())
    }

    fn provider(&self) -> &str {
        "mock"
    }
}

/// Auto-detect LLM client from environment variables.
/// Priority: ANTHROPIC_API_KEY > OPENAI_API_KEY
pub fn auto_detect_client() -> PlannerResult<Box<dyn LlmClient>> {
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        let mut client = ClaudeClient::new(key);
        if let Ok(model) = std::env::var("ANP_MODEL") {
            client = client.with_model(model);
        }
        return Ok(Box::new(client));
    }

    if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        let mut client = OpenAiClient::new(key);
        if let Ok(model) = std::env::var("ANP_MODEL") {
            client = client.with_model(model);
        }
        return Ok(Box::new(client));
    }

    Err(PlannerError::NoApiKey)
}
