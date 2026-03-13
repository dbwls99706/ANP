use std::collections::VecDeque;
use std::sync::Arc;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;
use tokio::sync::RwLock;

/// Memory node — session-based state management for stateful graphs.
///
/// Provides two capabilities:
/// 1. **Conversation history** — stores message pairs (user/assistant) for chatbot context
/// 2. **Key-value store** — persistent state across pipeline runs within the same session
///
/// Config example:
/// ```json
/// {
///   "max_history": 20,
///   "initial_state": { "counter": 0 }
/// }
/// ```
///
/// Input operations:
/// ```json
/// { "op": "append", "role": "user", "content": "Hello" }
/// { "op": "get_history" }
/// { "op": "get_history_text" }
/// { "op": "set", "key": "counter", "value": 42 }
/// { "op": "get", "key": "counter" }
/// { "op": "clear" }
/// ```
///
/// This node enables stateful AI workflows: chatbots with memory,
/// multi-turn conversations, and session-scoped data persistence.
pub struct MemoryNode {
    max_history: usize,
    history: Arc<RwLock<VecDeque<Message>>>,
    state: Arc<RwLock<serde_json::Map<String, serde_json::Value>>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Message {
    role: String,
    content: String,
}

impl Default for MemoryNode {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryNode {
    pub fn new() -> Self {
        Self {
            max_history: 50,
            history: Arc::new(RwLock::new(VecDeque::new())),
            state: Arc::new(RwLock::new(serde_json::Map::new())),
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for MemoryNode {
    fn node_type(&self) -> &str {
        "memory"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(max) = config.get("max_history").and_then(|v| v.as_u64()) {
            if max == 0 || max > 1000 {
                return Err(NodeError::InvalidConfig(
                    "max_history must be between 1 and 1000".into(),
                ));
            }
            self.max_history = max as usize;
        }

        if let Some(initial) = config.get("initial_state").and_then(|v| v.as_object()) {
            let mut state = self.state.write().await;
            for (k, v) in initial {
                state.insert(k.clone(), v.clone());
            }
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let op = input
            .get("op")
            .and_then(|v| v.as_str())
            .unwrap_or("get_history");

        match op {
            "append" => {
                let role = input
                    .get("role")
                    .and_then(|v| v.as_str())
                    .unwrap_or("user")
                    .to_string();
                let content = input
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("'content' field required for append".into()))?
                    .to_string();

                if content.is_empty() {
                    return Err(NodeError::MissingInput("content must not be empty".into()));
                }

                let mut history = self.history.write().await;
                history.push_back(Message {
                    role: role.clone(),
                    content: content.clone(),
                });

                // Enforce max_history limit
                while history.len() > self.max_history {
                    history.pop_front();
                }

                let count = history.len();
                drop(history);

                Ok(NodeOutput::data(serde_json::json!({
                    "op": "append",
                    "role": role,
                    "content": content,
                    "history_size": count
                })))
            }

            "get_history" => {
                let history = self.history.read().await;
                let messages: Vec<serde_json::Value> = history
                    .iter()
                    .map(|m| {
                        serde_json::json!({
                            "role": m.role,
                            "content": m.content
                        })
                    })
                    .collect();

                Ok(NodeOutput::data(serde_json::json!({
                    "op": "get_history",
                    "messages": messages,
                    "count": messages.len()
                })))
            }

            "get_history_text" => {
                let history = self.history.read().await;
                let text: String = history
                    .iter()
                    .map(|m| format!("{}: {}", m.role, m.content))
                    .collect::<Vec<_>>()
                    .join("\n");

                Ok(NodeOutput::data(serde_json::json!({
                    "op": "get_history_text",
                    "text": text,
                    "count": history.len()
                })))
            }

            "set" => {
                let key = input
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("'key' field required for set".into()))?
                    .to_string();
                let value = input
                    .get("value")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);

                let mut state = self.state.write().await;
                state.insert(key.clone(), value.clone());

                Ok(NodeOutput::data(serde_json::json!({
                    "op": "set",
                    "key": key,
                    "success": true
                })))
            }

            "get" => {
                let key = input
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("'key' field required for get".into()))?;

                let state = self.state.read().await;
                let value = state.get(key).cloned().unwrap_or(serde_json::Value::Null);
                let found = state.contains_key(key);

                Ok(NodeOutput::data(serde_json::json!({
                    "op": "get",
                    "key": key,
                    "value": value,
                    "found": found
                })))
            }

            "clear" => {
                let mut history = self.history.write().await;
                let history_count = history.len();
                history.clear();
                drop(history);

                let mut state = self.state.write().await;
                let state_count = state.len();
                state.clear();

                Ok(NodeOutput::data(serde_json::json!({
                    "op": "clear",
                    "cleared_messages": history_count,
                    "cleared_keys": state_count
                })))
            }

            other => Err(NodeError::ExecutionFailed(format!(
                "unknown memory operation: '{}'. Use: append, get_history, get_history_text, set, get, clear",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_append_and_get_history() {
        let mut node = MemoryNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({})).await.unwrap();

        // Append user message
        node.execute(
            serde_json::json!({"op": "append", "role": "user", "content": "Hello"}),
            &ctx,
        )
        .await
        .unwrap();

        // Append assistant message
        node.execute(
            serde_json::json!({"op": "append", "role": "assistant", "content": "Hi there!"}),
            &ctx,
        )
        .await
        .unwrap();

        // Get history
        let result = node
            .execute(serde_json::json!({"op": "get_history"}), &ctx)
            .await
            .unwrap();

        let data = &result.ports["default"];
        assert_eq!(data["count"], 2);
        assert_eq!(data["messages"][0]["role"], "user");
        assert_eq!(data["messages"][1]["content"], "Hi there!");
    }

    #[tokio::test]
    async fn test_max_history_limit() {
        let mut node = MemoryNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({"max_history": 3})).await.unwrap();

        for i in 0..5 {
            node.execute(
                serde_json::json!({"op": "append", "role": "user", "content": format!("msg{}", i)}),
                &ctx,
            )
            .await
            .unwrap();
        }

        let result = node
            .execute(serde_json::json!({"op": "get_history"}), &ctx)
            .await
            .unwrap();
        assert_eq!(result.ports["default"]["count"], 3);
        // Oldest messages should be dropped
        assert_eq!(result.ports["default"]["messages"][0]["content"], "msg2");
    }

    #[tokio::test]
    async fn test_state_set_get() {
        let mut node = MemoryNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({"initial_state": {"counter": 0}}))
            .await
            .unwrap();

        // Get initial
        let result = node
            .execute(serde_json::json!({"op": "get", "key": "counter"}), &ctx)
            .await
            .unwrap();
        assert_eq!(result.ports["default"]["value"], 0);
        assert_eq!(result.ports["default"]["found"], true);

        // Set new value
        node.execute(
            serde_json::json!({"op": "set", "key": "counter", "value": 42}),
            &ctx,
        )
        .await
        .unwrap();

        // Get updated
        let result = node
            .execute(serde_json::json!({"op": "get", "key": "counter"}), &ctx)
            .await
            .unwrap();
        assert_eq!(result.ports["default"]["value"], 42);
    }

    #[tokio::test]
    async fn test_get_nonexistent_key() {
        let mut node = MemoryNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let result = node
            .execute(serde_json::json!({"op": "get", "key": "missing"}), &ctx)
            .await
            .unwrap();
        assert_eq!(result.ports["default"]["found"], false);
        assert_eq!(result.ports["default"]["value"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn test_clear() {
        let mut node = MemoryNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({})).await.unwrap();

        node.execute(
            serde_json::json!({"op": "append", "role": "user", "content": "test"}),
            &ctx,
        )
        .await
        .unwrap();

        node.execute(
            serde_json::json!({"op": "set", "key": "k", "value": "v"}),
            &ctx,
        )
        .await
        .unwrap();

        let result = node
            .execute(serde_json::json!({"op": "clear"}), &ctx)
            .await
            .unwrap();
        assert_eq!(result.ports["default"]["cleared_messages"], 1);
        assert_eq!(result.ports["default"]["cleared_keys"], 1);
    }

    #[tokio::test]
    async fn test_get_history_text() {
        let mut node = MemoryNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({})).await.unwrap();

        node.execute(
            serde_json::json!({"op": "append", "role": "user", "content": "hi"}),
            &ctx,
        )
        .await
        .unwrap();
        node.execute(
            serde_json::json!({"op": "append", "role": "assistant", "content": "hello"}),
            &ctx,
        )
        .await
        .unwrap();

        let result = node
            .execute(serde_json::json!({"op": "get_history_text"}), &ctx)
            .await
            .unwrap();
        assert_eq!(
            result.ports["default"]["text"],
            "user: hi\nassistant: hello"
        );
    }

    #[tokio::test]
    async fn test_append_empty_content_rejected() {
        let mut node = MemoryNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let result = node
            .execute(
                serde_json::json!({"op": "append", "role": "user", "content": ""}),
                &ctx,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_unknown_op() {
        let mut node = MemoryNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let result = node
            .execute(serde_json::json!({"op": "invalid"}), &ctx)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_init_invalid_max_history() {
        let mut node = MemoryNode::new();
        let result = node.init(&serde_json::json!({"max_history": 0})).await;
        assert!(result.is_err());
    }
}
