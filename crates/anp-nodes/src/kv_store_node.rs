use std::collections::HashMap;
use std::sync::Arc;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;
use tokio::sync::RwLock;

/// KvStoreNode — key-value store with namespaces.
///
/// Config example:
/// ```json
/// {
///   "namespace": "users",
///   "operation": "set"
/// }
/// ```
///
/// Operations:
/// - `set`: store a key-value pair
/// - `get`: retrieve a value by key
/// - `delete`: remove a key
/// - `list`: list all entries in namespace
/// - `has`: check if key exists
/// - `count`: count entries in namespace
/// - `clear`: clear all entries in namespace
pub struct KvStoreNode {
    namespace: String,
    operation: String,
    store: Arc<RwLock<HashMap<String, HashMap<String, serde_json::Value>>>>,
}

impl Default for KvStoreNode {
    fn default() -> Self {
        Self::new()
    }
}

impl KvStoreNode {
    pub fn new() -> Self {
        Self {
            namespace: "default".to_string(),
            operation: "get".to_string(),
            store: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for KvStoreNode {
    fn node_type(&self) -> &str {
        "kv_store"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(ns) = config.get("namespace").and_then(|v| v.as_str()) {
            if ns.is_empty() {
                return Err(NodeError::InvalidConfig(
                    "namespace must not be empty".into(),
                ));
            }
            self.namespace = ns.to_string();
        }

        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "get" | "set" | "delete" | "list" | "has" | "count" | "clear" => {
                    self.operation = op.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "invalid operation '{}': must be get, set, delete, list, has, count, or clear",
                        other
                    )));
                }
            }
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        match self.operation.as_str() {
            "set" => {
                let key = input
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        NodeError::ExecutionFailed("'key' field (string) is required for set".into())
                    })?
                    .to_string();

                let value = input
                    .get("value")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);

                let mut store = self.store.write().await;
                let ns = store.entry(self.namespace.clone()).or_insert_with(HashMap::new);
                ns.insert(key.clone(), value.clone());

                Ok(NodeOutput::data(serde_json::json!({
                    "key": key,
                    "value": value,
                    "namespace": self.namespace,
                    "operation": "set",
                })))
            }
            "get" => {
                let key = input
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        NodeError::ExecutionFailed("'key' field (string) is required for get".into())
                    })?;

                let store = self.store.read().await;
                let (value, found) = store
                    .get(&self.namespace)
                    .and_then(|ns| ns.get(key).map(|v| (v.clone(), true)))
                    .unwrap_or((serde_json::Value::Null, false));

                Ok(NodeOutput::data(serde_json::json!({
                    "key": key,
                    "value": value,
                    "found": found,
                    "namespace": self.namespace,
                    "operation": "get",
                })))
            }
            "delete" => {
                let key = input
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        NodeError::ExecutionFailed(
                            "'key' field (string) is required for delete".into(),
                        )
                    })?
                    .to_string();

                let mut store = self.store.write().await;
                let deleted = store
                    .get_mut(&self.namespace)
                    .map(|ns| ns.remove(&key).is_some())
                    .unwrap_or(false);

                Ok(NodeOutput::data(serde_json::json!({
                    "deleted": deleted,
                    "key": key,
                    "namespace": self.namespace,
                    "operation": "delete",
                })))
            }
            "list" => {
                let store = self.store.read().await;
                let entries: Vec<serde_json::Value> = store
                    .get(&self.namespace)
                    .map(|ns| {
                        ns.iter()
                            .map(|(k, v)| {
                                serde_json::json!({
                                    "key": k,
                                    "value": v,
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let count = entries.len();

                Ok(NodeOutput::data(serde_json::json!({
                    "entries": entries,
                    "count": count,
                    "namespace": self.namespace,
                    "operation": "list",
                })))
            }
            "has" => {
                let key = input
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        NodeError::ExecutionFailed("'key' field (string) is required for has".into())
                    })?;

                let store = self.store.read().await;
                let exists = store
                    .get(&self.namespace)
                    .map(|ns| ns.contains_key(key))
                    .unwrap_or(false);

                Ok(NodeOutput::data(serde_json::json!({
                    "exists": exists,
                    "key": key,
                    "namespace": self.namespace,
                    "operation": "has",
                })))
            }
            "count" => {
                let store = self.store.read().await;
                let count = store
                    .get(&self.namespace)
                    .map(|ns| ns.len())
                    .unwrap_or(0);

                Ok(NodeOutput::data(serde_json::json!({
                    "count": count,
                    "namespace": self.namespace,
                    "operation": "count",
                })))
            }
            "clear" => {
                let mut store = self.store.write().await;
                let cleared = store
                    .get_mut(&self.namespace)
                    .map(|ns| {
                        let count = ns.len();
                        ns.clear();
                        count
                    })
                    .unwrap_or(0);

                Ok(NodeOutput::data(serde_json::json!({
                    "cleared": cleared,
                    "namespace": self.namespace,
                    "operation": "clear",
                })))
            }
            _ => Err(NodeError::ExecutionFailed(format!(
                "unknown operation: {}",
                self.operation
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ExecutionContext {
        ExecutionContext::default()
    }

    #[tokio::test]
    async fn test_set_and_get() {
        let mut set_node = KvStoreNode::new();
        set_node
            .init(&serde_json::json!({ "operation": "set", "namespace": "test" }))
            .await
            .unwrap();

        let shared = set_node.store.clone();

        set_node
            .execute(
                serde_json::json!({"key": "name", "value": "Alice"}),
                &ctx(),
            )
            .await
            .unwrap();

        let mut get_node = KvStoreNode::new();
        get_node.store = shared;
        get_node
            .init(&serde_json::json!({ "operation": "get", "namespace": "test" }))
            .await
            .unwrap();

        let r = get_node
            .execute(serde_json::json!({"key": "name"}), &ctx())
            .await
            .unwrap();
        let out = r.ports.get("default").unwrap();
        assert_eq!(out["value"], "Alice");
        assert_eq!(out["found"], true);
        assert_eq!(out["namespace"], "test");
    }

    #[tokio::test]
    async fn test_get_nonexistent_key() {
        let mut node = KvStoreNode::new();
        node.init(&serde_json::json!({ "operation": "get" }))
            .await
            .unwrap();

        let r = node
            .execute(serde_json::json!({"key": "missing"}), &ctx())
            .await
            .unwrap();
        let out = r.ports.get("default").unwrap();
        assert_eq!(out["found"], false);
        assert!(out["value"].is_null());
    }

    #[tokio::test]
    async fn test_delete() {
        let mut set_node = KvStoreNode::new();
        set_node
            .init(&serde_json::json!({ "operation": "set" }))
            .await
            .unwrap();

        let shared = set_node.store.clone();

        set_node
            .execute(serde_json::json!({"key": "k1", "value": 42}), &ctx())
            .await
            .unwrap();

        let mut del_node = KvStoreNode::new();
        del_node.store = shared;
        del_node
            .init(&serde_json::json!({ "operation": "delete" }))
            .await
            .unwrap();

        let r = del_node
            .execute(serde_json::json!({"key": "k1"}), &ctx())
            .await
            .unwrap();
        let out = r.ports.get("default").unwrap();
        assert_eq!(out["deleted"], true);
        assert_eq!(out["namespace"], "default");
    }

    #[tokio::test]
    async fn test_has_and_count() {
        let mut set_node = KvStoreNode::new();
        set_node
            .init(&serde_json::json!({ "operation": "set" }))
            .await
            .unwrap();

        let shared = set_node.store.clone();

        set_node
            .execute(serde_json::json!({"key": "a", "value": 1}), &ctx())
            .await
            .unwrap();
        set_node
            .execute(serde_json::json!({"key": "b", "value": 2}), &ctx())
            .await
            .unwrap();

        // has
        let mut has_node = KvStoreNode::new();
        has_node.store = shared.clone();
        has_node
            .init(&serde_json::json!({ "operation": "has" }))
            .await
            .unwrap();

        let r = has_node
            .execute(serde_json::json!({"key": "a"}), &ctx())
            .await
            .unwrap();
        assert_eq!(r.ports.get("default").unwrap()["exists"], true);

        let r = has_node
            .execute(serde_json::json!({"key": "z"}), &ctx())
            .await
            .unwrap();
        assert_eq!(r.ports.get("default").unwrap()["exists"], false);

        // count
        let mut count_node = KvStoreNode::new();
        count_node.store = shared;
        count_node
            .init(&serde_json::json!({ "operation": "count" }))
            .await
            .unwrap();

        let r = count_node
            .execute(serde_json::json!({}), &ctx())
            .await
            .unwrap();
        assert_eq!(r.ports.get("default").unwrap()["count"], 2);
    }

    #[tokio::test]
    async fn test_list_entries() {
        let mut set_node = KvStoreNode::new();
        set_node
            .init(&serde_json::json!({ "operation": "set", "namespace": "ns1" }))
            .await
            .unwrap();

        let shared = set_node.store.clone();

        set_node
            .execute(serde_json::json!({"key": "x", "value": 10}), &ctx())
            .await
            .unwrap();

        let mut list_node = KvStoreNode::new();
        list_node.store = shared;
        list_node
            .init(&serde_json::json!({ "operation": "list", "namespace": "ns1" }))
            .await
            .unwrap();

        let r = list_node
            .execute(serde_json::json!({}), &ctx())
            .await
            .unwrap();
        let out = r.ports.get("default").unwrap();
        assert_eq!(out["count"], 1);
        assert_eq!(out["namespace"], "ns1");
        assert_eq!(out["entries"][0]["key"], "x");
    }

    #[tokio::test]
    async fn test_clear() {
        let mut set_node = KvStoreNode::new();
        set_node
            .init(&serde_json::json!({ "operation": "set" }))
            .await
            .unwrap();

        let shared = set_node.store.clone();

        set_node
            .execute(serde_json::json!({"key": "a", "value": 1}), &ctx())
            .await
            .unwrap();
        set_node
            .execute(serde_json::json!({"key": "b", "value": 2}), &ctx())
            .await
            .unwrap();

        let mut clear_node = KvStoreNode::new();
        clear_node.store = shared;
        clear_node
            .init(&serde_json::json!({ "operation": "clear" }))
            .await
            .unwrap();

        let r = clear_node
            .execute(serde_json::json!({}), &ctx())
            .await
            .unwrap();
        assert_eq!(r.ports.get("default").unwrap()["cleared"], 2);
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let mut node = KvStoreNode::new();
        let result = node
            .init(&serde_json::json!({ "operation": "invalid" }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_empty_namespace_rejected() {
        let mut node = KvStoreNode::new();
        let result = node
            .init(&serde_json::json!({ "namespace": "" }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_node_type_and_kind() {
        let node = KvStoreNode::new();
        assert_eq!(node.node_type(), "kv_store");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
