use std::collections::HashMap;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Tool Router node — LLM-powered agentic routing.
///
/// The LLM decides which output port (tool) to route data to, based on the input.
/// This enables autonomous AI agent workflows where the AI chooses its own actions.
///
/// Config example:
/// ```json
/// {
///   "provider": "openai",
///   "model": "gpt-4o-mini",
///   "tools": [
///     { "name": "search", "description": "Search for information" },
///     { "name": "calculate", "description": "Perform calculations" },
///     { "name": "respond", "description": "Respond directly to the user" }
///   ],
///   "system_prompt": "You are a helpful assistant that selects the best tool.",
///   "max_tokens": 64
/// }
/// ```
///
/// The node sends the input to the LLM along with tool descriptions.
/// The LLM responds with a tool name. The input is then routed to
/// the corresponding output port.
///
/// Output ports: one per tool name + "default" for fallback.
pub struct ToolRouterNode {
    provider: String,
    model: String,
    tools: Vec<ToolDef>,
    system_prompt: String,
    max_tokens: u32,
    timeout_secs: u64,
}

#[derive(Debug, Clone)]
struct ToolDef {
    name: String,
    description: String,
}

impl Default for ToolRouterNode {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRouterNode {
    pub fn new() -> Self {
        Self {
            provider: "openai".to_string(),
            model: "gpt-4o-mini".to_string(),
            tools: Vec::new(),
            system_prompt: String::new(),
            max_tokens: 64,
            timeout_secs: 30,
        }
    }

    fn resolve_api_key(&self) -> Result<String, NodeError> {
        let var_name = match self.provider.as_str() {
            "openai" => "OPENAI_API_KEY",
            "anthropic" => "ANTHROPIC_API_KEY",
            _ => {
                return Err(NodeError::ExecutionFailed(format!(
                    "unknown provider: '{}'",
                    self.provider
                )))
            }
        };
        std::env::var(var_name).map_err(|_| NodeError::InitFailed(format!("{} not set", var_name)))
    }

    fn build_routing_prompt(&self, input: &serde_json::Value) -> String {
        let tools_desc: Vec<String> = self
            .tools
            .iter()
            .map(|t| format!("- {}: {}", t.name, t.description))
            .collect();

        let input_str = if let Some(s) = input.as_str() {
            s.to_string()
        } else if let Some(prompt) = input.get("prompt").and_then(|v| v.as_str()) {
            prompt.to_string()
        } else {
            serde_json::to_string(input).unwrap_or_default()
        };

        format!(
            "Given the following input, select exactly ONE tool to handle it.\n\
             \n\
             Available tools:\n\
             {}\n\
             \n\
             Input: {}\n\
             \n\
             Respond with ONLY the tool name, nothing else.",
            tools_desc.join("\n"),
            input_str
        )
    }

    async fn call_llm(&self, prompt: &str, api_key: &str) -> Result<String, NodeError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .build()
            .map_err(|e| {
                NodeError::ExecutionFailed(format!("failed to build HTTP client: {}", e))
            })?;

        let response_text = match self.provider.as_str() {
            "openai" => {
                let mut messages = Vec::new();
                if !self.system_prompt.is_empty() {
                    messages.push(serde_json::json!({
                        "role": "system",
                        "content": self.system_prompt
                    }));
                }
                messages.push(serde_json::json!({
                    "role": "user",
                    "content": prompt
                }));

                let body = serde_json::json!({
                    "model": self.model,
                    "messages": messages,
                    "temperature": 0.0,
                    "max_tokens": self.max_tokens
                });

                let resp = client
                    .post("https://api.openai.com/v1/chat/completions")
                    .header("Authorization", format!("Bearer {}", api_key))
                    .header("Content-Type", "application/json")
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| {
                        NodeError::ExecutionFailed(format!("HTTP request failed: {}", e))
                    })?;

                let status = resp.status();
                let resp_body: serde_json::Value = resp
                    .json()
                    .await
                    .map_err(|e| NodeError::ExecutionFailed(format!("parse error: {}", e)))?;

                if !status.is_success() {
                    let err_msg = resp_body["error"]["message"]
                        .as_str()
                        .unwrap_or("unknown API error");
                    return Err(NodeError::ExecutionFailed(format!(
                        "OpenAI API error ({}): {}",
                        status, err_msg
                    )));
                }

                resp_body
                    .get("choices")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("message"))
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_str())
                    .ok_or_else(|| {
                        NodeError::ExecutionFailed(
                            "OpenAI response missing choices[0].message.content".into(),
                        )
                    })?
                    .to_string()
            }
            "anthropic" => {
                let mut body = serde_json::json!({
                    "model": self.model,
                    "max_tokens": self.max_tokens,
                    "messages": [{"role": "user", "content": prompt}]
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
                    .map_err(|e| {
                        NodeError::ExecutionFailed(format!("HTTP request failed: {}", e))
                    })?;

                let status = resp.status();
                let resp_body: serde_json::Value = resp
                    .json()
                    .await
                    .map_err(|e| NodeError::ExecutionFailed(format!("parse error: {}", e)))?;

                if !status.is_success() {
                    let err_msg = resp_body["error"]["message"]
                        .as_str()
                        .unwrap_or("unknown API error");
                    return Err(NodeError::ExecutionFailed(format!(
                        "Anthropic API error ({}): {}",
                        status, err_msg
                    )));
                }

                resp_body
                    .get("content")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("text"))
                    .and_then(|t| t.as_str())
                    .ok_or_else(|| {
                        NodeError::ExecutionFailed(
                            "Anthropic response missing content[0].text".into(),
                        )
                    })?
                    .to_string()
            }
            other => {
                return Err(NodeError::ExecutionFailed(format!(
                    "unknown provider: '{}'",
                    other
                )));
            }
        };

        Ok(response_text.trim().to_string())
    }
}

#[async_trait::async_trait]
impl NodeExecutor for ToolRouterNode {
    fn node_type(&self) -> &str {
        "tool_router"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(provider) = config.get("provider").and_then(|v| v.as_str()) {
            match provider {
                "openai" | "anthropic" => self.provider = provider.to_string(),
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown provider: '{}'. Use: openai, anthropic",
                        other
                    )));
                }
            }
        }

        if let Some(model) = config.get("model").and_then(|v| v.as_str()) {
            if model.is_empty() {
                return Err(NodeError::InvalidConfig("model must not be empty".into()));
            }
            self.model = model.to_string();
        }

        let tools = config
            .get("tools")
            .and_then(|v| v.as_array())
            .ok_or_else(|| NodeError::InvalidConfig("missing 'tools' array in config".into()))?;

        if tools.is_empty() {
            return Err(NodeError::InvalidConfig(
                "tools array must not be empty".into(),
            ));
        }

        self.tools = Vec::with_capacity(tools.len());
        for (i, tool) in tools.iter().enumerate() {
            let name = tool
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| NodeError::InvalidConfig(format!("tool {} missing 'name'", i)))?;
            if name.is_empty() {
                return Err(NodeError::InvalidConfig(format!(
                    "tool {} has empty name",
                    i
                )));
            }

            let description = tool
                .get("description")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    NodeError::InvalidConfig(format!("tool {} missing 'description'", i))
                })?
                .to_string();

            self.tools.push(ToolDef {
                name: name.to_string(),
                description,
            });
        }

        if let Some(sp) = config.get("system_prompt").and_then(|v| v.as_str()) {
            self.system_prompt = sp.to_string();
        }

        if let Some(mt) = config.get("max_tokens").and_then(|v| v.as_u64()) {
            if mt == 0 || mt > 1024 {
                return Err(NodeError::InvalidConfig(
                    "max_tokens for tool_router must be between 1 and 1024".into(),
                ));
            }
            self.max_tokens = mt as u32;
        }

        if let Some(timeout) = config.get("timeout_secs").and_then(|v| v.as_u64()) {
            if timeout == 0 || timeout > 120 {
                return Err(NodeError::InvalidConfig(
                    "timeout_secs must be between 1 and 120".into(),
                ));
            }
            self.timeout_secs = timeout;
        }

        // Reject api_key in config — must use env vars
        if config.get("api_key").is_some() {
            return Err(NodeError::InvalidConfig(
                "api_key must not be set in config — use environment variables".into(),
            ));
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let api_key = self.resolve_api_key()?;
        let prompt = self.build_routing_prompt(&input);
        let chosen_tool = self.call_llm(&prompt, &api_key).await?;

        // Normalize: trim, lowercase, find matching tool
        // Priority: exact match first, then fallback to contains (prevents "search" matching "research")
        let chosen_lower = chosen_tool.to_lowercase();
        let matched_tool = self
            .tools
            .iter()
            .find(|t| t.name.to_lowercase() == chosen_lower)
            .or_else(|| {
                self.tools
                    .iter()
                    .find(|t| chosen_lower.contains(&t.name.to_lowercase()))
            });

        let port = match matched_tool {
            Some(tool) => tool.name.clone(),
            None => {
                // LLM returned something we don't recognize — use default
                return Ok(NodeOutput::data(serde_json::json!({
                    "routed_to": "default",
                    "llm_response": chosen_tool,
                    "data": input
                }))
                .with_log(format!(
                    "tool_router: LLM chose '{}' which didn't match any tool, using default",
                    chosen_tool
                )));
            }
        };

        let mut ports = HashMap::new();
        ports.insert(
            port.clone(),
            serde_json::json!({
                "routed_to": port,
                "data": input
            }),
        );

        Ok(NodeOutput::multi(ports).with_log(format!("tool_router: LLM chose tool '{}'", port)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_init_valid() {
        let mut node = ToolRouterNode::new();
        node.init(&serde_json::json!({
            "tools": [
                { "name": "search", "description": "Search the web" },
                { "name": "calculate", "description": "Do math" }
            ]
        }))
        .await
        .unwrap();
        assert_eq!(node.tools.len(), 2);
        assert_eq!(node.tools[0].name, "search");
    }

    #[tokio::test]
    async fn test_init_missing_tools() {
        let mut node = ToolRouterNode::new();
        let result = node.init(&serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_init_empty_tools() {
        let mut node = ToolRouterNode::new();
        let result = node.init(&serde_json::json!({"tools": []})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_init_tool_missing_name() {
        let mut node = ToolRouterNode::new();
        let result = node
            .init(&serde_json::json!({
                "tools": [{"description": "no name"}]
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_init_tool_empty_name() {
        let mut node = ToolRouterNode::new();
        let result = node
            .init(&serde_json::json!({
                "tools": [{"name": "", "description": "empty name"}]
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_init_rejects_api_key() {
        let mut node = ToolRouterNode::new();
        let result = node
            .init(&serde_json::json!({
                "tools": [{"name": "t", "description": "d"}],
                "api_key": "secret"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_init_invalid_provider() {
        let mut node = ToolRouterNode::new();
        let result = node
            .init(&serde_json::json!({
                "provider": "gemini",
                "tools": [{"name": "t", "description": "d"}]
            }))
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_build_routing_prompt() {
        let node = ToolRouterNode {
            provider: "openai".to_string(),
            model: "gpt-4o-mini".to_string(),
            tools: vec![
                ToolDef {
                    name: "search".to_string(),
                    description: "Search the web".to_string(),
                },
                ToolDef {
                    name: "calculate".to_string(),
                    description: "Do math".to_string(),
                },
            ],
            system_prompt: String::new(),
            max_tokens: 64,
            timeout_secs: 30,
        };

        let prompt = node.build_routing_prompt(&serde_json::json!({"prompt": "What is 2+2?"}));
        assert!(prompt.contains("search: Search the web"));
        assert!(prompt.contains("calculate: Do math"));
        assert!(prompt.contains("What is 2+2?"));
    }
}
