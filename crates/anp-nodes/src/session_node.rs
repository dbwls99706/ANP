use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;
use tokio::sync::RwLock;

/// Session node — in-memory session management with TTL expiration.
///
/// Provides a thread-safe session store keyed by session ID.
/// Each session contains arbitrary key-value data and expires
/// after a configurable TTL.
///
/// Config example:
/// ```json
/// {
///   "operation": "create",
///   "ttl_ms": 3600000,
///   "max_sessions": 10000
/// }
/// ```
///
/// Input operations:
/// ```json
/// {}                                                      // create
/// { "session_id": "abc123" }                              // get
/// { "session_id": "abc123", "key": "user", "value": {} }  // set
/// { "session_id": "abc123", "key": "user" }               // delete
/// { "session_id": "abc123" }                              // destroy
/// ```
pub struct SessionNode {
    operation: String,
    ttl_ms: u64,
    max_sessions: usize,
    sessions: Arc<RwLock<HashMap<String, Session>>>,
    counter: Arc<AtomicU64>,
}

struct Session {
    data: HashMap<String, serde_json::Value>,
    expires_at: Instant,
}

impl Default for SessionNode {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionNode {
    pub fn new() -> Self {
        Self {
            operation: "create".to_string(),
            ttl_ms: 3_600_000,
            max_sessions: 10_000,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            counter: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Generate a simple session ID from timestamp + counter.
    fn generate_session_id(&self) -> String {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let count = self.counter.fetch_add(1, Ordering::Relaxed);
        format!("{:x}{:04x}", ts, count)
    }
}

#[async_trait::async_trait]
impl NodeExecutor for SessionNode {
    fn node_type(&self) -> &str {
        "session"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "create" | "get" | "set" | "delete" | "destroy" => {
                    self.operation = op.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown session operation: '{}'. Use: create, get, set, delete, destroy",
                        other
                    )));
                }
            }
        }

        if let Some(ttl) = config.get("ttl_ms").and_then(|v| v.as_u64()) {
            if ttl == 0 {
                return Err(NodeError::InvalidConfig(
                    "ttl_ms must be greater than 0".into(),
                ));
            }
            self.ttl_ms = ttl;
        }

        if let Some(max) = config.get("max_sessions").and_then(|v| v.as_u64()) {
            if max == 0 || max > 1_000_000 {
                return Err(NodeError::InvalidConfig(
                    "max_sessions must be between 1 and 1000000".into(),
                ));
            }
            self.max_sessions = max as usize;
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let op = input
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.operation);

        match op {
            "create" => {
                let mut sessions = self.sessions.write().await;

                // Evict expired sessions if at capacity
                if sessions.len() >= self.max_sessions {
                    let now = Instant::now();
                    sessions.retain(|_, s| s.expires_at > now);
                }

                // Check capacity after eviction
                if sessions.len() >= self.max_sessions {
                    return Err(NodeError::ExecutionFailed(format!(
                        "session store is full ({} sessions). Increase max_sessions or destroy old sessions",
                        self.max_sessions
                    )));
                }

                let session_id = self.generate_session_id();
                let now = Instant::now();
                let ttl = std::time::Duration::from_millis(self.ttl_ms);

                sessions.insert(
                    session_id.clone(),
                    Session {
                        data: HashMap::new(),
                        expires_at: now + ttl,
                    },
                );

                Ok(NodeOutput::data(serde_json::json!({
                    "operation": "create",
                    "session_id": session_id,
                    "created": true,
                    "ttl_ms": self.ttl_ms
                })))
            }

            "get" => {
                let session_id = input
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        NodeError::MissingInput("'session_id' field required for get".into())
                    })?;

                let mut sessions = self.sessions.write().await;
                let now = Instant::now();

                if let Some(session) = sessions.get(session_id) {
                    if session.expires_at > now {
                        let data: serde_json::Value = serde_json::json!(session.data);
                        Ok(NodeOutput::data(serde_json::json!({
                            "operation": "get",
                            "session_id": session_id,
                            "found": true,
                            "data": data
                        })))
                    } else {
                        // Session expired — remove it
                        sessions.remove(session_id);
                        Ok(NodeOutput::data(serde_json::json!({
                            "operation": "get",
                            "session_id": session_id,
                            "found": false,
                            "expired": true
                        })))
                    }
                } else {
                    Ok(NodeOutput::data(serde_json::json!({
                        "operation": "get",
                        "session_id": session_id,
                        "found": false
                    })))
                }
            }

            "set" => {
                let session_id = input
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        NodeError::MissingInput("'session_id' field required for set".into())
                    })?;
                let key = input
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("'key' field required for set".into()))?
                    .to_string();
                let value = input
                    .get("value")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);

                let mut sessions = self.sessions.write().await;
                let now = Instant::now();

                if let Some(session) = sessions.get_mut(session_id) {
                    if session.expires_at > now {
                        session.data.insert(key.clone(), value);
                        // Refresh TTL on write
                        session.expires_at = now + std::time::Duration::from_millis(self.ttl_ms);

                        Ok(NodeOutput::data(serde_json::json!({
                            "operation": "set",
                            "session_id": session_id,
                            "key": key,
                            "success": true
                        })))
                    } else {
                        sessions.remove(session_id);
                        Err(NodeError::ExecutionFailed(format!(
                            "session '{}' has expired",
                            session_id
                        )))
                    }
                } else {
                    Err(NodeError::ExecutionFailed(format!(
                        "session '{}' not found",
                        session_id
                    )))
                }
            }

            "delete" => {
                let session_id = input
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        NodeError::MissingInput("'session_id' field required for delete".into())
                    })?;
                let key = input.get("key").and_then(|v| v.as_str()).ok_or_else(|| {
                    NodeError::MissingInput("'key' field required for delete".into())
                })?;

                let mut sessions = self.sessions.write().await;
                let now = Instant::now();

                if let Some(session) = sessions.get_mut(session_id) {
                    if session.expires_at > now {
                        let existed = session.data.remove(key).is_some();
                        Ok(NodeOutput::data(serde_json::json!({
                            "operation": "delete",
                            "session_id": session_id,
                            "key": key,
                            "deleted": existed
                        })))
                    } else {
                        sessions.remove(session_id);
                        Err(NodeError::ExecutionFailed(format!(
                            "session '{}' has expired",
                            session_id
                        )))
                    }
                } else {
                    Err(NodeError::ExecutionFailed(format!(
                        "session '{}' not found",
                        session_id
                    )))
                }
            }

            "destroy" => {
                let session_id = input
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        NodeError::MissingInput("'session_id' field required for destroy".into())
                    })?;

                let mut sessions = self.sessions.write().await;
                let existed = sessions.remove(session_id).is_some();

                Ok(NodeOutput::data(serde_json::json!({
                    "operation": "destroy",
                    "session_id": session_id,
                    "destroyed": existed
                })))
            }

            other => Err(NodeError::ExecutionFailed(format!(
                "unknown session operation: '{}'. Use: create, get, set, delete, destroy",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_session() {
        let mut node = SessionNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let result = node
            .execute(serde_json::json!({"operation": "create"}), &ctx)
            .await
            .unwrap();

        let data = &result.ports["default"];
        assert_eq!(data["created"], true);
        assert!(data["session_id"].as_str().is_some());
        assert!(!data["session_id"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_set_and_get() {
        let mut node = SessionNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({})).await.unwrap();

        // Create session
        let result = node
            .execute(serde_json::json!({"operation": "create"}), &ctx)
            .await
            .unwrap();
        let session_id = result.ports["default"]["session_id"]
            .as_str()
            .unwrap()
            .to_string();

        // Set value
        node.execute(
            serde_json::json!({
                "operation": "set",
                "session_id": session_id,
                "key": "username",
                "value": "alice"
            }),
            &ctx,
        )
        .await
        .unwrap();

        // Get session data
        let result = node
            .execute(
                serde_json::json!({
                    "operation": "get",
                    "session_id": session_id
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert_eq!(result.ports["default"]["found"], true);
        assert_eq!(result.ports["default"]["data"]["username"], "alice");
    }

    #[tokio::test]
    async fn test_delete_key() {
        let mut node = SessionNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({})).await.unwrap();

        // Create and set
        let result = node
            .execute(serde_json::json!({"operation": "create"}), &ctx)
            .await
            .unwrap();
        let session_id = result.ports["default"]["session_id"]
            .as_str()
            .unwrap()
            .to_string();

        node.execute(
            serde_json::json!({
                "operation": "set",
                "session_id": session_id,
                "key": "temp",
                "value": 42
            }),
            &ctx,
        )
        .await
        .unwrap();

        // Delete key
        let result = node
            .execute(
                serde_json::json!({
                    "operation": "delete",
                    "session_id": session_id,
                    "key": "temp"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result.ports["default"]["deleted"], true);

        // Verify key is gone
        let result = node
            .execute(
                serde_json::json!({
                    "operation": "get",
                    "session_id": session_id
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.ports["default"]["data"]
            .as_object()
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn test_destroy_session() {
        let mut node = SessionNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let result = node
            .execute(serde_json::json!({"operation": "create"}), &ctx)
            .await
            .unwrap();
        let session_id = result.ports["default"]["session_id"]
            .as_str()
            .unwrap()
            .to_string();

        let result = node
            .execute(
                serde_json::json!({
                    "operation": "destroy",
                    "session_id": session_id
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result.ports["default"]["destroyed"], true);

        // Session should no longer exist
        let result = node
            .execute(
                serde_json::json!({
                    "operation": "get",
                    "session_id": session_id
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result.ports["default"]["found"], false);
    }

    #[tokio::test]
    async fn test_get_nonexistent_session() {
        let mut node = SessionNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let result = node
            .execute(
                serde_json::json!({
                    "operation": "get",
                    "session_id": "nonexistent"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result.ports["default"]["found"], false);
    }

    #[tokio::test]
    async fn test_set_nonexistent_session_fails() {
        let mut node = SessionNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let result = node
            .execute(
                serde_json::json!({
                    "operation": "set",
                    "session_id": "nonexistent",
                    "key": "k",
                    "value": "v"
                }),
                &ctx,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let mut node = SessionNode::new();
        let result = node
            .init(&serde_json::json!({"operation": "invalid"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_unique_session_ids() {
        let mut node = SessionNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let r1 = node
            .execute(serde_json::json!({"operation": "create"}), &ctx)
            .await
            .unwrap();
        let r2 = node
            .execute(serde_json::json!({"operation": "create"}), &ctx)
            .await
            .unwrap();

        let id1 = r1.ports["default"]["session_id"].as_str().unwrap();
        let id2 = r2.ports["default"]["session_id"].as_str().unwrap();
        assert_ne!(id1, id2);
    }
}
