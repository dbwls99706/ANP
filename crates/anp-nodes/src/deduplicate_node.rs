use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;
use tokio::sync::RwLock;

/// DeduplicateNode — Deduplication by key field.
///
/// Config:
/// ```json
/// {
///   "key_field": "id",
///   "window_size": 1000,
///   "ttl_ms": 0
/// }
/// ```
pub struct DeduplicateNode {
    key_field: String,
    window_size: usize,
    ttl_ms: u64,
    seen: Arc<RwLock<HashMap<String, u64>>>,
    insertion_order: Arc<RwLock<Vec<String>>>,
}

impl Default for DeduplicateNode {
    fn default() -> Self {
        Self::new()
    }
}

impl DeduplicateNode {
    pub fn new() -> Self {
        Self {
            key_field: String::new(),
            window_size: 1000,
            ttl_ms: 0,
            seen: Arc::new(RwLock::new(HashMap::new())),
            insertion_order: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

/// Resolve a dot-path field from a JSON value, returning a string representation.
fn resolve_field(value: &serde_json::Value, field: &str) -> Option<String> {
    let mut current = value;
    for part in field.split('.') {
        current = current.get(part)?;
    }
    match current {
        serde_json::Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[async_trait::async_trait]
impl NodeExecutor for DeduplicateNode {
    fn node_type(&self) -> &str {
        "deduplicate"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        self.key_field = config
            .get("key_field")
            .and_then(|v| v.as_str())
            .ok_or_else(|| NodeError::InvalidConfig("'key_field' is required".to_string()))?
            .to_string();

        if let Some(window) = config.get("window_size").and_then(|v| v.as_u64()) {
            if window == 0 {
                return Err(NodeError::InvalidConfig(
                    "window_size must be greater than 0".to_string(),
                ));
            }
            self.window_size = window as usize;
        }

        if let Some(ttl) = config.get("ttl_ms").and_then(|v| v.as_u64()) {
            self.ttl_ms = ttl;
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let key_value = resolve_field(&input, &self.key_field).ok_or_else(|| {
            NodeError::ExecutionFailed(format!("key field '{}' not found in input", self.key_field))
        })?;

        let now = now_ms();
        let mut seen = self.seen.write().await;
        let mut order = self.insertion_order.write().await;

        // Check if key has been seen
        if let Some(&timestamp) = seen.get(&key_value) {
            if self.ttl_ms == 0 || now.saturating_sub(timestamp) < self.ttl_ms {
                // Key is still valid (not expired) — skip as duplicate
                return Ok(NodeOutput::skip());
            }
            // Key has expired — remove it and treat as new
            seen.remove(&key_value);
            order.retain(|k| k != &key_value);
        }

        // Evict expired entries if ttl_ms > 0
        if self.ttl_ms > 0 {
            let ttl = self.ttl_ms;
            order.retain(|k| {
                if let Some(&ts) = seen.get(k) {
                    if now.saturating_sub(ts) >= ttl {
                        seen.remove(k);
                        false
                    } else {
                        true
                    }
                } else {
                    false
                }
            });
        }

        // Evict oldest entries if window_size exceeded
        while order.len() >= self.window_size {
            if let Some(oldest_key) = order.first().cloned() {
                seen.remove(&oldest_key);
                order.remove(0);
            }
        }

        // Add the new key
        seen.insert(key_value.clone(), now);
        order.push(key_value);

        let seen_count = seen.len();

        let mut result = input.clone();
        if let Some(obj) = result.as_object_mut() {
            obj.insert(
                "_deduplicate".to_string(),
                serde_json::json!({
                    "is_new": true,
                    "seen_count": seen_count,
                }),
            );
        }

        Ok(NodeOutput::data(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_first_message_passes_through() {
        let mut node = DeduplicateNode::new();
        node.init(&serde_json::json!({"key_field": "id"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"id": "msg1", "data": "hello"}), &ctx)
            .await
            .unwrap();

        let data = &output.ports["default"];
        assert_eq!(data["data"], "hello");
        assert_eq!(data["_deduplicate"]["is_new"], true);
        assert_eq!(data["_deduplicate"]["seen_count"], 1);
    }

    #[tokio::test]
    async fn test_duplicate_is_skipped() {
        let mut node = DeduplicateNode::new();
        node.init(&serde_json::json!({"key_field": "id"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();

        // First message passes
        let r1 = node
            .execute(serde_json::json!({"id": "msg1"}), &ctx)
            .await
            .unwrap();
        assert!(r1.ports.contains_key("default"));

        // Duplicate is skipped
        let r2 = node
            .execute(serde_json::json!({"id": "msg1"}), &ctx)
            .await
            .unwrap();
        assert!(r2.ports.is_empty()); // skip() returns empty ports
    }

    #[tokio::test]
    async fn test_different_keys_pass() {
        let mut node = DeduplicateNode::new();
        node.init(&serde_json::json!({"key_field": "id"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();

        let r1 = node
            .execute(serde_json::json!({"id": "msg1"}), &ctx)
            .await
            .unwrap();
        assert!(r1.ports.contains_key("default"));

        let r2 = node
            .execute(serde_json::json!({"id": "msg2"}), &ctx)
            .await
            .unwrap();
        assert!(r2.ports.contains_key("default"));
        assert_eq!(r2.ports["default"]["_deduplicate"]["seen_count"], 2);
    }

    #[tokio::test]
    async fn test_window_size_eviction() {
        let mut node = DeduplicateNode::new();
        node.init(&serde_json::json!({"key_field": "id", "window_size": 2}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();

        // Insert 2 keys (fills window)
        node.execute(serde_json::json!({"id": "a"}), &ctx)
            .await
            .unwrap();
        node.execute(serde_json::json!({"id": "b"}), &ctx)
            .await
            .unwrap();

        // Insert 3rd key — should evict "a"
        node.execute(serde_json::json!({"id": "c"}), &ctx)
            .await
            .unwrap();

        // "a" should now be treated as new again
        let output = node
            .execute(serde_json::json!({"id": "a"}), &ctx)
            .await
            .unwrap();
        assert!(output.ports.contains_key("default"));
        assert_eq!(output.ports["default"]["_deduplicate"]["is_new"], true);
    }

    #[tokio::test]
    async fn test_nested_key_field() {
        let mut node = DeduplicateNode::new();
        node.init(&serde_json::json!({"key_field": "meta.id"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();

        let r1 = node
            .execute(serde_json::json!({"meta": {"id": "x1"}, "data": 1}), &ctx)
            .await
            .unwrap();
        assert!(r1.ports.contains_key("default"));

        let r2 = node
            .execute(serde_json::json!({"meta": {"id": "x1"}, "data": 2}), &ctx)
            .await
            .unwrap();
        assert!(r2.ports.is_empty()); // duplicate
    }

    #[tokio::test]
    async fn test_missing_key_field_errors() {
        let mut node = DeduplicateNode::new();
        node.init(&serde_json::json!({"key_field": "id"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let result = node
            .execute(serde_json::json!({"other": "value"}), &ctx)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_config_requires_key_field() {
        let mut node = DeduplicateNode::new();
        let result = node.init(&serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_node_type_and_kind() {
        let node = DeduplicateNode::new();
        assert_eq!(node.node_type(), "deduplicate");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
