use std::collections::HashMap;
use std::sync::Arc;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;
use tokio::sync::RwLock;

/// EventEmitterNode — pub/sub event log system within graphs.
///
/// Config example:
/// ```json
/// {
///   "operation": "emit",
///   "event_name": "user_created",
///   "events": ["user_created", "order_placed"]
/// }
/// ```
///
/// Operations:
/// - `emit`: stores event data in the event log under event_name
/// - `subscribe`: registers interest in an event
/// - `unsubscribe`: removes interest in an event
/// - `list`: returns all events and their counts
pub struct EventEmitterNode {
    operation: String,
    event_name: Option<String>,
    events: Option<Vec<String>>,
    event_log: Arc<RwLock<HashMap<String, Vec<serde_json::Value>>>>,
    subscriptions: Arc<RwLock<HashMap<String, bool>>>,
}

impl Default for EventEmitterNode {
    fn default() -> Self {
        Self::new()
    }
}

impl EventEmitterNode {
    pub fn new() -> Self {
        Self {
            operation: "emit".to_string(),
            event_name: None,
            events: None,
            event_log: Arc::new(RwLock::new(HashMap::new())),
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for EventEmitterNode {
    fn node_type(&self) -> &str {
        "event_emitter"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "emit" | "subscribe" | "unsubscribe" | "list" => {
                    self.operation = op.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "invalid operation '{}': must be emit, subscribe, unsubscribe, or list",
                        other
                    )));
                }
            }
        }

        if let Some(name) = config.get("event_name").and_then(|v| v.as_str()) {
            self.event_name = Some(name.to_string());
        }

        if let Some(events) = config.get("events").and_then(|v| v.as_array()) {
            self.events = Some(
                events
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect(),
            );
        }

        // Validate that emit/subscribe/unsubscribe have event_name
        match self.operation.as_str() {
            "emit" | "subscribe" | "unsubscribe" => {
                if self.event_name.is_none() {
                    return Err(NodeError::InvalidConfig(format!(
                        "operation '{}' requires 'event_name'",
                        self.operation
                    )));
                }
            }
            _ => {}
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        match self.operation.as_str() {
            "emit" => {
                let event_name = self.event_name.as_ref().unwrap();

                // Validate against allowed events if configured
                if let Some(ref allowed) = self.events {
                    if !allowed.contains(event_name) {
                        return Err(NodeError::ExecutionFailed(format!(
                            "event '{}' is not in the allowed events list",
                            event_name
                        )));
                    }
                }

                let mut log = self.event_log.write().await;
                let entries = log.entry(event_name.clone()).or_insert_with(Vec::new);
                entries.push(input);
                let count = entries.len();

                // Count subscriptions for this event
                let subs = self.subscriptions.read().await;
                let listeners = if subs.get(event_name).copied().unwrap_or(false) {
                    1
                } else {
                    0
                };

                Ok(NodeOutput::data(serde_json::json!({
                    "emitted": true,
                    "event": event_name,
                    "listeners": listeners,
                    "event_count": count,
                })))
            }
            "subscribe" => {
                let event_name = self.event_name.as_ref().unwrap();

                let mut subs = self.subscriptions.write().await;
                subs.insert(event_name.clone(), true);

                Ok(NodeOutput::data(serde_json::json!({
                    "subscribed": true,
                    "event": event_name,
                })))
            }
            "unsubscribe" => {
                let event_name = self.event_name.as_ref().unwrap();

                let mut subs = self.subscriptions.write().await;
                subs.remove(event_name);

                Ok(NodeOutput::data(serde_json::json!({
                    "unsubscribed": true,
                    "event": event_name,
                })))
            }
            "list" => {
                let log = self.event_log.read().await;
                let events: serde_json::Map<String, serde_json::Value> = log
                    .iter()
                    .map(|(k, v)| (k.clone(), serde_json::json!(v.len())))
                    .collect();

                Ok(NodeOutput::data(serde_json::json!({
                    "events": events,
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
    async fn test_emit_event() {
        let mut node = EventEmitterNode::new();
        node.init(&serde_json::json!({
            "operation": "emit",
            "event_name": "user_created"
        }))
        .await
        .unwrap();

        let result = node
            .execute(serde_json::json!({"user_id": 42}), &ctx())
            .await
            .unwrap();
        let out = result.ports.get("default").unwrap();
        assert_eq!(out["emitted"], true);
        assert_eq!(out["event"], "user_created");
        assert_eq!(out["event_count"], 1);
    }

    #[tokio::test]
    async fn test_subscribe_and_emit() {
        let mut sub_node = EventEmitterNode::new();
        sub_node
            .init(&serde_json::json!({
                "operation": "subscribe",
                "event_name": "order"
            }))
            .await
            .unwrap();

        let shared_log = sub_node.event_log.clone();
        let shared_subs = sub_node.subscriptions.clone();

        sub_node
            .execute(serde_json::json!({}), &ctx())
            .await
            .unwrap();

        // Now emit with shared state
        let mut emit_node = EventEmitterNode::new();
        emit_node.event_log = shared_log;
        emit_node.subscriptions = shared_subs;
        emit_node
            .init(&serde_json::json!({
                "operation": "emit",
                "event_name": "order"
            }))
            .await
            .unwrap();

        let result = emit_node
            .execute(serde_json::json!({"item": "book"}), &ctx())
            .await
            .unwrap();
        let out = result.ports.get("default").unwrap();
        assert_eq!(out["listeners"], 1);
    }

    #[tokio::test]
    async fn test_list_events() {
        let mut emit_node = EventEmitterNode::new();
        emit_node
            .init(&serde_json::json!({
                "operation": "emit",
                "event_name": "click"
            }))
            .await
            .unwrap();

        let shared_log = emit_node.event_log.clone();

        emit_node
            .execute(serde_json::json!({"x": 1}), &ctx())
            .await
            .unwrap();
        emit_node
            .execute(serde_json::json!({"x": 2}), &ctx())
            .await
            .unwrap();

        let mut list_node = EventEmitterNode::new();
        list_node.event_log = shared_log;
        list_node
            .init(&serde_json::json!({ "operation": "list" }))
            .await
            .unwrap();

        let result = list_node
            .execute(serde_json::json!({}), &ctx())
            .await
            .unwrap();
        let out = result.ports.get("default").unwrap();
        assert_eq!(out["events"]["click"], 2);
    }

    #[tokio::test]
    async fn test_unsubscribe() {
        let mut node = EventEmitterNode::new();
        node.init(&serde_json::json!({
            "operation": "subscribe",
            "event_name": "test_event"
        }))
        .await
        .unwrap();

        let shared_subs = node.subscriptions.clone();

        node.execute(serde_json::json!({}), &ctx()).await.unwrap();

        // Verify subscribed
        assert!(shared_subs.read().await.contains_key("test_event"));

        let mut unsub_node = EventEmitterNode::new();
        unsub_node.subscriptions = shared_subs.clone();
        unsub_node
            .init(&serde_json::json!({
                "operation": "unsubscribe",
                "event_name": "test_event"
            }))
            .await
            .unwrap();

        let result = unsub_node
            .execute(serde_json::json!({}), &ctx())
            .await
            .unwrap();
        let out = result.ports.get("default").unwrap();
        assert_eq!(out["unsubscribed"], true);
        assert!(!shared_subs.read().await.contains_key("test_event"));
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let mut node = EventEmitterNode::new();
        let result = node
            .init(&serde_json::json!({ "operation": "invalid" }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_emit_requires_event_name() {
        let mut node = EventEmitterNode::new();
        let result = node.init(&serde_json::json!({ "operation": "emit" })).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_emit_validates_allowed_events() {
        let mut node = EventEmitterNode::new();
        node.init(&serde_json::json!({
            "operation": "emit",
            "event_name": "unknown_event",
            "events": ["allowed_event"]
        }))
        .await
        .unwrap();

        let result = node.execute(serde_json::json!({"data": 1}), &ctx()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_node_type_and_kind() {
        let node = EventEmitterNode::new();
        assert_eq!(node.node_type(), "event_emitter");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
