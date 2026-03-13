use std::collections::HashMap;
use std::sync::Arc;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;
use tokio::sync::RwLock;

/// Database node — in-memory key-value store
///
/// Supports operations: get, set, delete, list
///
/// Input format:
/// ```json
/// { "op": "set", "key": "user:1", "value": { "name": "Alice" } }
/// { "op": "get", "key": "user:1" }
/// { "op": "delete", "key": "user:1" }
/// { "op": "list" }
/// ```
pub struct DatabaseNode {
    store: Arc<RwLock<HashMap<String, serde_json::Value>>>,
}

impl Default for DatabaseNode {
    fn default() -> Self {
        Self::new()
    }
}

impl DatabaseNode {
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for DatabaseNode {
    fn node_type(&self) -> &str {
        "database"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        // Pre-populate with initial data if provided
        if let Some(initial) = config.get("initial").and_then(|v| v.as_object()) {
            let mut store = self.store.write().await;
            for (key, value) in initial {
                store.insert(key.clone(), value.clone());
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
            .unwrap_or("get");

        match op {
            "set" => {
                let key = input
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("key".into()))?
                    .to_string();
                let value = input
                    .get("value")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);

                let mut store = self.store.write().await;
                store.insert(key.clone(), value.clone());

                Ok(NodeOutput::data(serde_json::json!({
                    "op": "set",
                    "key": key,
                    "success": true
                }))
                .with_log(format!("set key '{}'", key)))
            }
            "get" => {
                let key = input
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("key".into()))?;

                let store = self.store.read().await;
                let value = store.get(key).cloned().unwrap_or(serde_json::Value::Null);
                let found = store.contains_key(key);

                Ok(NodeOutput::data(serde_json::json!({
                    "op": "get",
                    "key": key,
                    "value": value,
                    "found": found
                })))
            }
            "delete" => {
                let key = input
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("key".into()))?
                    .to_string();

                let mut store = self.store.write().await;
                let existed = store.remove(&key).is_some();

                Ok(NodeOutput::data(serde_json::json!({
                    "op": "delete",
                    "key": key,
                    "existed": existed
                })))
            }
            "list" => {
                let store = self.store.read().await;
                let keys: Vec<&str> = store.keys().map(|k| k.as_str()).collect();

                Ok(NodeOutput::data(serde_json::json!({
                    "op": "list",
                    "keys": keys,
                    "count": keys.len()
                })))
            }
            other => Err(NodeError::ExecutionFailed(format!(
                "unknown database operation: '{}'. Use: get, set, delete, list",
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
        let mut db = DatabaseNode::new();
        let ctx = ExecutionContext::new();
        db.init(&serde_json::json!({})).await.unwrap();

        // Set
        let set_result = db
            .execute(
                serde_json::json!({ "op": "set", "key": "k1", "value": "hello" }),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(set_result.ports["default"]["success"], true);

        // Get
        let get_result = db
            .execute(serde_json::json!({ "op": "get", "key": "k1" }), &ctx)
            .await
            .unwrap();
        assert_eq!(get_result.ports["default"]["value"], "hello");
        assert_eq!(get_result.ports["default"]["found"], true);
    }
}
