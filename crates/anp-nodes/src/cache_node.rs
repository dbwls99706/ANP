use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;
use tokio::sync::RwLock;

/// Cache node — in-memory key-value cache with TTL expiration.
///
/// Provides a thread-safe cache for storing values with automatic expiration.
/// Useful for caching API responses, computed results, or any data that
/// should persist across pipeline runs but expire after a configurable duration.
///
/// Config example:
/// ```json
/// {
///   "operation": "set",
///   "ttl_ms": 60000,
///   "max_entries": 1000
/// }
/// ```
///
/// Input operations:
/// ```json
/// { "key": "user:42", "value": { "name": "Alice" } }   // set
/// { "key": "user:42" }                                   // get
/// { "key": "user:42" }                                   // delete
/// {}                                                     // clear
/// { "key": "user:42" }                                   // has
/// ```
pub struct CacheNode {
    operation: String,
    ttl_ms: u64,
    max_entries: usize,
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
}

struct CacheEntry {
    value: serde_json::Value,
    expires_at: Instant,
}

impl Default for CacheNode {
    fn default() -> Self {
        Self::new()
    }
}

impl CacheNode {
    pub fn new() -> Self {
        Self {
            operation: "get".to_string(),
            ttl_ms: 60_000,
            max_entries: 1000,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for CacheNode {
    fn node_type(&self) -> &str {
        "cache"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "get" | "set" | "delete" | "clear" | "has" => {
                    self.operation = op.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown cache operation: '{}'. Use: get, set, delete, clear, has",
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

        if let Some(max) = config.get("max_entries").and_then(|v| v.as_u64()) {
            if max == 0 || max > 1_000_000 {
                return Err(NodeError::InvalidConfig(
                    "max_entries must be between 1 and 1000000".into(),
                ));
            }
            self.max_entries = max as usize;
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
            "set" => {
                let key = input
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        NodeError::MissingInput("'key' field required for set".into())
                    })?
                    .to_string();
                let value = input
                    .get("value")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);

                let ttl_ms = input
                    .get("ttl_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(self.ttl_ms);

                let mut cache = self.cache.write().await;

                // Evict expired entries if at capacity
                if cache.len() >= self.max_entries {
                    let now = Instant::now();
                    cache.retain(|_, entry| entry.expires_at > now);
                }

                // Check capacity again after eviction
                if cache.len() >= self.max_entries {
                    return Err(NodeError::ExecutionFailed(format!(
                        "cache is full ({} entries). Increase max_entries or clear the cache",
                        self.max_entries
                    )));
                }

                let expires_at =
                    Instant::now() + std::time::Duration::from_millis(ttl_ms);
                cache.insert(
                    key.clone(),
                    CacheEntry {
                        value: value.clone(),
                        expires_at,
                    },
                );

                Ok(NodeOutput::data(serde_json::json!({
                    "operation": "set",
                    "key": key,
                    "success": true,
                    "ttl_ms": ttl_ms,
                    "cache_size": cache.len()
                })))
            }

            "get" => {
                let key = input
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        NodeError::MissingInput("'key' field required for get".into())
                    })?;

                let mut cache = self.cache.write().await;
                let now = Instant::now();

                if let Some(entry) = cache.get(key) {
                    if entry.expires_at > now {
                        let value = entry.value.clone();
                        Ok(NodeOutput::data(serde_json::json!({
                            "operation": "get",
                            "key": key,
                            "hit": true,
                            "value": value
                        })))
                    } else {
                        // Entry expired — evict it
                        cache.remove(key);
                        Ok(NodeOutput::data(serde_json::json!({
                            "operation": "get",
                            "key": key,
                            "hit": false,
                            "value": null
                        })))
                    }
                } else {
                    Ok(NodeOutput::data(serde_json::json!({
                        "operation": "get",
                        "key": key,
                        "hit": false,
                        "value": null
                    })))
                }
            }

            "delete" => {
                let key = input
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        NodeError::MissingInput("'key' field required for delete".into())
                    })?;

                let mut cache = self.cache.write().await;
                let existed = cache.remove(key).is_some();

                Ok(NodeOutput::data(serde_json::json!({
                    "operation": "delete",
                    "key": key,
                    "deleted": existed
                })))
            }

            "clear" => {
                let mut cache = self.cache.write().await;
                let count = cache.len();
                cache.clear();

                Ok(NodeOutput::data(serde_json::json!({
                    "operation": "clear",
                    "cleared": count
                })))
            }

            "has" => {
                let key = input
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        NodeError::MissingInput("'key' field required for has".into())
                    })?;

                let mut cache = self.cache.write().await;
                let now = Instant::now();

                let exists = if let Some(entry) = cache.get(key) {
                    if entry.expires_at > now {
                        true
                    } else {
                        cache.remove(key);
                        false
                    }
                } else {
                    false
                };

                Ok(NodeOutput::data(serde_json::json!({
                    "operation": "has",
                    "key": key,
                    "exists": exists
                })))
            }

            other => Err(NodeError::ExecutionFailed(format!(
                "unknown cache operation: '{}'. Use: get, set, delete, clear, has",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_set_and_get() {
        let mut node = CacheNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({"operation": "set", "ttl_ms": 60000}))
            .await
            .unwrap();

        // Set a value
        let result = node
            .execute(
                serde_json::json!({"operation": "set", "key": "k1", "value": "hello"}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result.ports["default"]["success"], true);

        // Get the value
        let result = node
            .execute(
                serde_json::json!({"operation": "get", "key": "k1"}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result.ports["default"]["hit"], true);
        assert_eq!(result.ports["default"]["value"], "hello");
    }

    #[tokio::test]
    async fn test_get_miss() {
        let mut node = CacheNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let result = node
            .execute(
                serde_json::json!({"operation": "get", "key": "missing"}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result.ports["default"]["hit"], false);
        assert_eq!(result.ports["default"]["value"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn test_delete() {
        let mut node = CacheNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({})).await.unwrap();

        node.execute(
            serde_json::json!({"operation": "set", "key": "k1", "value": 42}),
            &ctx,
        )
        .await
        .unwrap();

        let result = node
            .execute(
                serde_json::json!({"operation": "delete", "key": "k1"}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result.ports["default"]["deleted"], true);

        let result = node
            .execute(
                serde_json::json!({"operation": "get", "key": "k1"}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result.ports["default"]["hit"], false);
    }

    #[tokio::test]
    async fn test_clear() {
        let mut node = CacheNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({})).await.unwrap();

        node.execute(
            serde_json::json!({"operation": "set", "key": "a", "value": 1}),
            &ctx,
        )
        .await
        .unwrap();
        node.execute(
            serde_json::json!({"operation": "set", "key": "b", "value": 2}),
            &ctx,
        )
        .await
        .unwrap();

        let result = node
            .execute(serde_json::json!({"operation": "clear"}), &ctx)
            .await
            .unwrap();
        assert_eq!(result.ports["default"]["cleared"], 2);
    }

    #[tokio::test]
    async fn test_has() {
        let mut node = CacheNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({})).await.unwrap();

        node.execute(
            serde_json::json!({"operation": "set", "key": "k1", "value": true}),
            &ctx,
        )
        .await
        .unwrap();

        let result = node
            .execute(
                serde_json::json!({"operation": "has", "key": "k1"}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result.ports["default"]["exists"], true);

        let result = node
            .execute(
                serde_json::json!({"operation": "has", "key": "missing"}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result.ports["default"]["exists"], false);
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let mut node = CacheNode::new();
        let result = node
            .init(&serde_json::json!({"operation": "invalid"}))
            .await;
        assert!(result.is_err());
    }
}
