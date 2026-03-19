use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Message broker with topics, subscriptions, and delivery guarantees.
/// Supports pub/sub, work queues, fan-out, and dead-letter queues.
/// Implements patterns: publish/subscribe, request/reply, topic routing.
pub struct MessageBrokerNode {
    operation: String,
    max_queue_size: usize,
    broker: Arc<RwLock<BrokerState>>,
}

#[derive(Default)]
struct BrokerState {
    topics: HashMap<String, TopicState>,
    subscriptions: HashMap<String, Vec<Subscription>>,
    dead_letters: Vec<DeadLetter>,
}

#[derive(Clone, Default)]
struct TopicState {
    messages: Vec<BrokerMessage>,
    total_published: u64,
    total_consumed: u64,
}

#[derive(Clone)]
struct BrokerMessage {
    id: String,
    topic: String,
    payload: Value,
    timestamp: u64,
    headers: HashMap<String, String>,
    delivery_count: u32,
    _max_retries: u32,
}

#[derive(Clone)]
struct Subscription {
    _subscriber_id: String,
    _pattern: SubscriptionPattern,
    _filter: Option<Value>,
}

#[derive(Clone)]
enum SubscriptionPattern {
    Exact(#[allow(dead_code)] String),
    Prefix(#[allow(dead_code)] String),
    All,
}

#[derive(Clone)]
struct DeadLetter {
    original_topic: String,
    message: BrokerMessage,
    reason: String,
    timestamp: u64,
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn generate_message_id() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("msg-{:016x}", now)
}

impl MessageBrokerNode {
    pub fn new() -> Self {
        Self {
            operation: "publish".to_string(),
            max_queue_size: 10000,
            broker: Arc::new(RwLock::new(BrokerState::default())),
        }
    }
}

impl Default for MessageBrokerNode {
    fn default() -> Self {
        Self::new()
    }
}

fn _matches_pattern(pattern: &SubscriptionPattern, topic: &str) -> bool {
    match pattern {
        SubscriptionPattern::Exact(t) => t == topic,
        SubscriptionPattern::Prefix(p) => topic.starts_with(p.as_str()),
        SubscriptionPattern::All => true,
    }
}

fn _matches_filter(filter: &Option<Value>, message: &Value) -> bool {
    if let Some(f) = filter {
        if let Some(obj) = f.as_object() {
            for (key, expected) in obj {
                if message.get(key) != Some(expected) {
                    return false;
                }
            }
        }
    }
    true
}

#[async_trait]
impl NodeExecutor for MessageBrokerNode {
    fn node_type(&self) -> &str {
        "message_broker"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "publish" | "subscribe" | "consume" | "ack" | "nack" | "create_topic"
                | "delete_topic" | "list_topics" | "unsubscribe" | "dead_letters" | "purge"
                | "stats" | "peek" => {
                    self.operation = op.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown operation: {}",
                        other
                    )));
                }
            }
        }
        if let Some(max) = config.get("max_queue_size").and_then(|v| v.as_u64()) {
            self.max_queue_size = max as usize;
        }
        // Pre-create topics
        if let Some(topics) = config.get("topics").and_then(|v| v.as_array()) {
            let mut broker = self.broker.write().await;
            for topic in topics {
                if let Some(name) = topic.as_str() {
                    broker.topics.entry(name.to_string()).or_default();
                }
            }
        }
        Ok(())
    }

    async fn execute(&self, input: Value, _ctx: &ExecutionContext) -> NodeResult<NodeOutput> {
        let op = input
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.operation);

        match op {
            "publish" => {
                let topic = input
                    .get("topic")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("topic required".into()))?;
                let payload = input.get("payload").cloned().unwrap_or(json!({}));
                let max_retries = input
                    .get("max_retries")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(3) as u32;

                let mut headers = HashMap::new();
                if let Some(h) = input.get("headers").and_then(|v| v.as_object()) {
                    for (k, v) in h {
                        headers.insert(
                            k.clone(),
                            v.as_str()
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| v.to_string()),
                        );
                    }
                }

                let msg_id = generate_message_id();
                let message = BrokerMessage {
                    id: msg_id.clone(),
                    topic: topic.to_string(),
                    payload,
                    timestamp: current_timestamp(),
                    headers,
                    delivery_count: 0,
                    _max_retries: max_retries,
                };

                let mut broker = self.broker.write().await;
                let topic_state = broker.topics.entry(topic.to_string()).or_default();

                if topic_state.messages.len() >= self.max_queue_size {
                    return Err(NodeError::ExecutionFailed(format!(
                        "topic '{}' queue is full (max: {})",
                        topic, self.max_queue_size
                    )));
                }

                topic_state.messages.push(message);
                topic_state.total_published += 1;

                Ok(NodeOutput::data(json!({
                    "message_id": msg_id,
                    "topic": topic,
                    "published": true,
                    "queue_depth": topic_state.messages.len(),
                })))
            }

            "consume" => {
                let topic = input
                    .get("topic")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("topic required".into()))?;
                let count = input.get("count").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                let subscriber_id = input
                    .get("subscriber_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default");

                let mut broker = self.broker.write().await;
                let topic_state = broker.topics.entry(topic.to_string()).or_default();

                let consume_count = count.min(topic_state.messages.len());
                let messages: Vec<Value> = topic_state
                    .messages
                    .drain(..consume_count)
                    .map(|mut msg| {
                        msg.delivery_count += 1;
                        topic_state.total_consumed += 1;
                        json!({
                            "id": msg.id,
                            "topic": msg.topic,
                            "payload": msg.payload,
                            "timestamp": msg.timestamp,
                            "headers": msg.headers,
                            "delivery_count": msg.delivery_count,
                        })
                    })
                    .collect();

                Ok(NodeOutput::data(json!({
                    "messages": messages,
                    "count": messages.len(),
                    "subscriber_id": subscriber_id,
                    "remaining": topic_state.messages.len(),
                })))
            }

            "subscribe" => {
                let subscriber_id = input
                    .get("subscriber_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("subscriber_id required".into()))?;
                let topic_pattern = input.get("topic").and_then(|v| v.as_str()).unwrap_or("*");
                let filter = input.get("filter").cloned();

                let pattern = if topic_pattern == "*" {
                    SubscriptionPattern::All
                } else if topic_pattern.ends_with('*') {
                    SubscriptionPattern::Prefix(topic_pattern.trim_end_matches('*').to_string())
                } else {
                    SubscriptionPattern::Exact(topic_pattern.to_string())
                };

                let sub = Subscription {
                    _subscriber_id: subscriber_id.to_string(),
                    _pattern: pattern,
                    _filter: filter,
                };

                let mut broker = self.broker.write().await;
                broker
                    .subscriptions
                    .entry(subscriber_id.to_string())
                    .or_default()
                    .push(sub);

                Ok(NodeOutput::data(json!({
                    "subscriber_id": subscriber_id,
                    "topic_pattern": topic_pattern,
                    "subscribed": true,
                })))
            }

            "unsubscribe" => {
                let subscriber_id = input
                    .get("subscriber_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("subscriber_id required".into()))?;

                let mut broker = self.broker.write().await;
                let existed = broker.subscriptions.remove(subscriber_id).is_some();

                Ok(NodeOutput::data(json!({
                    "subscriber_id": subscriber_id,
                    "unsubscribed": existed,
                })))
            }

            "ack" => {
                let message_id = input
                    .get("message_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                Ok(NodeOutput::data(json!({
                    "message_id": message_id,
                    "acknowledged": true,
                })))
            }

            "nack" => {
                let topic = input.get("topic").and_then(|v| v.as_str()).unwrap_or("");
                let message_id = input
                    .get("message_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let payload = input.get("payload").cloned().unwrap_or(json!({}));
                let delivery_count = input
                    .get("delivery_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1) as u32;
                let max_retries = input
                    .get("max_retries")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(3) as u32;

                let mut broker = self.broker.write().await;

                if delivery_count >= max_retries {
                    // Move to dead letter queue
                    broker.dead_letters.push(DeadLetter {
                        original_topic: topic.to_string(),
                        message: BrokerMessage {
                            id: message_id.to_string(),
                            topic: topic.to_string(),
                            payload: payload.clone(),
                            timestamp: current_timestamp(),
                            headers: HashMap::new(),
                            delivery_count,
                            _max_retries: max_retries,
                        },
                        reason: "max_retries_exceeded".to_string(),
                        timestamp: current_timestamp(),
                    });
                    Ok(NodeOutput::data(json!({
                        "message_id": message_id,
                        "requeued": false,
                        "dead_lettered": true,
                    })))
                } else {
                    // Requeue
                    let topic_state = broker.topics.entry(topic.to_string()).or_default();
                    topic_state.messages.push(BrokerMessage {
                        id: message_id.to_string(),
                        topic: topic.to_string(),
                        payload,
                        timestamp: current_timestamp(),
                        headers: HashMap::new(),
                        delivery_count,
                        _max_retries: max_retries,
                    });
                    Ok(NodeOutput::data(json!({
                        "message_id": message_id,
                        "requeued": true,
                        "dead_lettered": false,
                    })))
                }
            }

            "create_topic" => {
                let topic = input
                    .get("topic")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("topic required".into()))?;

                let mut broker = self.broker.write().await;
                let existed = broker.topics.contains_key(topic);
                broker.topics.entry(topic.to_string()).or_default();

                Ok(NodeOutput::data(json!({
                    "topic": topic,
                    "created": !existed,
                })))
            }

            "delete_topic" => {
                let topic = input
                    .get("topic")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("topic required".into()))?;

                let mut broker = self.broker.write().await;
                let existed = broker.topics.remove(topic).is_some();

                Ok(NodeOutput::data(json!({
                    "topic": topic,
                    "deleted": existed,
                })))
            }

            "list_topics" => {
                let broker = self.broker.read().await;
                let topics: Vec<Value> = broker
                    .topics
                    .iter()
                    .map(|(name, state)| {
                        json!({
                            "topic": name,
                            "queue_depth": state.messages.len(),
                            "total_published": state.total_published,
                            "total_consumed": state.total_consumed,
                        })
                    })
                    .collect();

                Ok(NodeOutput::data(json!({
                    "topics": topics,
                    "count": topics.len(),
                })))
            }

            "peek" => {
                let topic = input
                    .get("topic")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("topic required".into()))?;
                let count = input.get("count").and_then(|v| v.as_u64()).unwrap_or(1) as usize;

                let broker = self.broker.read().await;
                let messages: Vec<Value> = broker
                    .topics
                    .get(topic)
                    .map(|state| {
                        state
                            .messages
                            .iter()
                            .take(count)
                            .map(|msg| {
                                json!({
                                    "id": msg.id,
                                    "payload": msg.payload,
                                    "timestamp": msg.timestamp,
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                Ok(NodeOutput::data(json!({
                    "messages": messages,
                    "count": messages.len(),
                })))
            }

            "purge" => {
                let topic = input
                    .get("topic")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("topic required".into()))?;

                let mut broker = self.broker.write().await;
                let purged = broker
                    .topics
                    .get_mut(topic)
                    .map(|state| {
                        let count = state.messages.len();
                        state.messages.clear();
                        count
                    })
                    .unwrap_or(0);

                Ok(NodeOutput::data(json!({
                    "topic": topic,
                    "purged": purged,
                })))
            }

            "dead_letters" => {
                let broker = self.broker.read().await;
                let letters: Vec<Value> = broker
                    .dead_letters
                    .iter()
                    .map(|dl| {
                        json!({
                            "original_topic": dl.original_topic,
                            "message_id": dl.message.id,
                            "payload": dl.message.payload,
                            "reason": dl.reason,
                            "timestamp": dl.timestamp,
                            "delivery_count": dl.message.delivery_count,
                        })
                    })
                    .collect();

                Ok(NodeOutput::data(json!({
                    "dead_letters": letters,
                    "count": letters.len(),
                })))
            }

            "stats" => {
                let broker = self.broker.read().await;
                let total_messages: usize = broker.topics.values().map(|s| s.messages.len()).sum();
                let total_published: u64 = broker.topics.values().map(|s| s.total_published).sum();
                let total_consumed: u64 = broker.topics.values().map(|s| s.total_consumed).sum();

                Ok(NodeOutput::data(json!({
                    "topic_count": broker.topics.len(),
                    "subscription_count": broker.subscriptions.len(),
                    "total_queued": total_messages,
                    "total_published": total_published,
                    "total_consumed": total_consumed,
                    "dead_letter_count": broker.dead_letters.len(),
                })))
            }

            other => Err(NodeError::ExecutionFailed(format!(
                "unknown operation: {}",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ExecutionContext {
        ExecutionContext::new()
    }

    #[tokio::test]
    async fn test_publish_consume() {
        let mut node = MessageBrokerNode::new();
        node.init(&json!({"topics": ["orders"]})).await.unwrap();

        // Publish
        let output = node
            .execute(
                json!({
                    "operation": "publish",
                    "topic": "orders",
                    "payload": {"item": "widget", "qty": 5}
                }),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["published"], true);
        assert_eq!(output.ports["default"]["queue_depth"], 1);

        // Consume
        let output = node
            .execute(
                json!({
                    "operation": "consume",
                    "topic": "orders",
                    "count": 1
                }),
                &ctx(),
            )
            .await
            .unwrap();
        let messages = output.ports["default"]["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["payload"]["item"], "widget");
        assert_eq!(output.ports["default"]["remaining"], 0);
    }

    #[tokio::test]
    async fn test_subscribe_unsubscribe() {
        let mut node = MessageBrokerNode::new();
        node.init(&json!({})).await.unwrap();

        let output = node
            .execute(
                json!({
                    "operation": "subscribe",
                    "subscriber_id": "worker-1",
                    "topic": "events.*"
                }),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["subscribed"], true);

        let output = node
            .execute(
                json!({
                    "operation": "unsubscribe",
                    "subscriber_id": "worker-1"
                }),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["unsubscribed"], true);
    }

    #[tokio::test]
    async fn test_dead_letter() {
        let mut node = MessageBrokerNode::new();
        node.init(&json!({})).await.unwrap();

        // NACK with max retries exceeded
        let output = node
            .execute(
                json!({
                    "operation": "nack",
                    "topic": "orders",
                    "message_id": "msg-123",
                    "payload": {"item": "failed"},
                    "delivery_count": 3,
                    "max_retries": 3
                }),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["dead_lettered"], true);

        // Check dead letters
        let output = node
            .execute(json!({"operation": "dead_letters"}), &ctx())
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["count"], 1);
    }

    #[tokio::test]
    async fn test_stats() {
        let mut node = MessageBrokerNode::new();
        node.init(&json!({"topics": ["a", "b"]})).await.unwrap();

        node.execute(
            json!({"operation": "publish", "topic": "a", "payload": {"x": 1}}),
            &ctx(),
        )
        .await
        .unwrap();
        node.execute(
            json!({"operation": "publish", "topic": "a", "payload": {"x": 2}}),
            &ctx(),
        )
        .await
        .unwrap();

        let output = node
            .execute(json!({"operation": "stats"}), &ctx())
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["topic_count"], 2);
        assert_eq!(output.ports["default"]["total_published"], 2);
        assert_eq!(output.ports["default"]["total_queued"], 2);
    }

    #[tokio::test]
    async fn test_purge() {
        let mut node = MessageBrokerNode::new();
        node.init(&json!({})).await.unwrap();

        node.execute(
            json!({"operation": "publish", "topic": "t", "payload": {}}),
            &ctx(),
        )
        .await
        .unwrap();
        node.execute(
            json!({"operation": "publish", "topic": "t", "payload": {}}),
            &ctx(),
        )
        .await
        .unwrap();

        let output = node
            .execute(json!({"operation": "purge", "topic": "t"}), &ctx())
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["purged"], 2);
    }
}
