use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Enhanced WebSocket node with room management, broadcasting, and message routing.
/// Supports: connect, disconnect, send, broadcast, room management, and connection tracking.
/// Works as a message processor for WebSocket-like real-time communication patterns.
pub struct WebSocketEnhancedNode {
    operation: String,
    state: Arc<RwLock<WsState>>,
}

#[derive(Default)]
struct WsState {
    connections: HashMap<String, ConnectionInfo>,
    rooms: HashMap<String, Vec<String>>,
    message_history: Vec<WsMessage>,
    max_history: usize,
    next_id: u64,
}

#[derive(Clone)]
struct ConnectionInfo {
    id: String,
    metadata: Value,
    connected_at: u64,
    rooms: Vec<String>,
    message_count: u64,
}

#[derive(Clone)]
struct WsMessage {
    from: String,
    to: MessageTarget,
    data: Value,
    timestamp: u64,
    event_type: String,
}

#[derive(Clone)]
enum MessageTarget {
    Direct(String),
    Room(String),
    Broadcast,
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl WebSocketEnhancedNode {
    pub fn new() -> Self {
        Self {
            operation: "send".to_string(),
            state: Arc::new(RwLock::new(WsState {
                max_history: 100,
                ..Default::default()
            })),
        }
    }
}

impl Default for WebSocketEnhancedNode {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NodeExecutor for WebSocketEnhancedNode {
    fn node_type(&self) -> &str {
        "ws"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "connect" | "disconnect" | "send" | "broadcast" | "join_room" | "leave_room"
                | "room_send" | "connections" | "rooms" | "history" | "kick" => {
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
        if let Some(max) = config.get("max_history").and_then(|v| v.as_u64()) {
            let mut state = self.state.write().await;
            state.max_history = max as usize;
        }
        Ok(())
    }

    async fn execute(&self, input: Value, _ctx: &ExecutionContext) -> NodeResult<NodeOutput> {
        let op = input
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.operation);

        match op {
            "connect" => {
                let mut state = self.state.write().await;
                state.next_id += 1;
                let conn_id = input
                    .get("connection_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("conn-{}", state.next_id));

                let metadata = input.get("metadata").cloned().unwrap_or(json!({}));

                let conn = ConnectionInfo {
                    id: conn_id.clone(),
                    metadata: metadata.clone(),
                    connected_at: current_timestamp(),
                    rooms: Vec::new(),
                    message_count: 0,
                };

                state.connections.insert(conn_id.clone(), conn);

                // Auto-join default room if specified
                if let Some(room) = input.get("room").and_then(|v| v.as_str()) {
                    state
                        .rooms
                        .entry(room.to_string())
                        .or_default()
                        .push(conn_id.clone());
                    if let Some(conn) = state.connections.get_mut(&conn_id) {
                        conn.rooms.push(room.to_string());
                    }
                }

                let connection_count = state.connections.len();

                Ok(NodeOutput::data(json!({
                    "connection_id": conn_id,
                    "event": "connected",
                    "metadata": metadata,
                    "total_connections": connection_count,
                })))
            }

            "disconnect" => {
                let conn_id = input
                    .get("connection_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("connection_id required".into()))?;

                let mut state = self.state.write().await;

                // Remove from all rooms
                if let Some(conn) = state.connections.remove(conn_id) {
                    for room in &conn.rooms {
                        if let Some(members) = state.rooms.get_mut(room) {
                            members.retain(|id| id != conn_id);
                            if members.is_empty() {
                                state.rooms.remove(room);
                            }
                        }
                    }
                    Ok(NodeOutput::data(json!({
                        "connection_id": conn_id,
                        "event": "disconnected",
                        "total_connections": state.connections.len(),
                    })))
                } else {
                    Ok(NodeOutput::data(json!({
                        "connection_id": conn_id,
                        "event": "not_found",
                    })))
                }
            }

            "send" => {
                let from = input
                    .get("from")
                    .and_then(|v| v.as_str())
                    .unwrap_or("system");
                let to = input
                    .get("to")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("to (connection_id) required".into()))?;
                let data = input.get("data").cloned().unwrap_or(json!({}));
                let event_type = input
                    .get("event_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("message");

                let mut state = self.state.write().await;

                let exists = state.connections.contains_key(to);
                if exists {
                    if let Some(conn) = state.connections.get_mut(to) {
                        conn.message_count += 1;
                    }
                }

                let msg = WsMessage {
                    from: from.to_string(),
                    to: MessageTarget::Direct(to.to_string()),
                    data: data.clone(),
                    timestamp: current_timestamp(),
                    event_type: event_type.to_string(),
                };
                state.message_history.push(msg);
                if state.message_history.len() > state.max_history {
                    state.message_history.remove(0);
                }

                Ok(NodeOutput::data(json!({
                    "event": "message_sent",
                    "from": from,
                    "to": to,
                    "event_type": event_type,
                    "data": data,
                    "delivered": exists,
                })))
            }

            "broadcast" => {
                let from = input
                    .get("from")
                    .and_then(|v| v.as_str())
                    .unwrap_or("system");
                let data = input.get("data").cloned().unwrap_or(json!({}));
                let event_type = input
                    .get("event_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("broadcast");
                let exclude = input
                    .get("exclude")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let mut state = self.state.write().await;

                let recipients: Vec<String> = state
                    .connections
                    .keys()
                    .filter(|id| id.as_str() != exclude)
                    .cloned()
                    .collect();

                for id in &recipients {
                    if let Some(conn) = state.connections.get_mut(id) {
                        conn.message_count += 1;
                    }
                }

                let msg = WsMessage {
                    from: from.to_string(),
                    to: MessageTarget::Broadcast,
                    data: data.clone(),
                    timestamp: current_timestamp(),
                    event_type: event_type.to_string(),
                };
                state.message_history.push(msg);
                if state.message_history.len() > state.max_history {
                    state.message_history.remove(0);
                }

                Ok(NodeOutput::data(json!({
                    "event": "broadcast",
                    "from": from,
                    "event_type": event_type,
                    "data": data,
                    "recipients": recipients.len(),
                })))
            }

            "join_room" => {
                let conn_id = input
                    .get("connection_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("connection_id required".into()))?;
                let room = input
                    .get("room")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("room required".into()))?;

                let mut state = self.state.write().await;

                if !state.connections.contains_key(conn_id) {
                    return Err(NodeError::ExecutionFailed(format!(
                        "connection '{}' not found",
                        conn_id
                    )));
                }

                let members = state.rooms.entry(room.to_string()).or_default();
                if !members.contains(&conn_id.to_string()) {
                    members.push(conn_id.to_string());
                }
                let member_count = members.len();

                if let Some(conn) = state.connections.get_mut(conn_id) {
                    if !conn.rooms.contains(&room.to_string()) {
                        conn.rooms.push(room.to_string());
                    }
                }

                Ok(NodeOutput::data(json!({
                    "connection_id": conn_id,
                    "room": room,
                    "event": "room_joined",
                    "room_size": member_count,
                })))
            }

            "leave_room" => {
                let conn_id = input
                    .get("connection_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("connection_id required".into()))?;
                let room = input
                    .get("room")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("room required".into()))?;

                let mut state = self.state.write().await;

                if let Some(members) = state.rooms.get_mut(room) {
                    members.retain(|id| id != conn_id);
                    if members.is_empty() {
                        state.rooms.remove(room);
                    }
                }

                if let Some(conn) = state.connections.get_mut(conn_id) {
                    conn.rooms.retain(|r| r != room);
                }

                Ok(NodeOutput::data(json!({
                    "connection_id": conn_id,
                    "room": room,
                    "event": "room_left",
                })))
            }

            "room_send" => {
                let room = input
                    .get("room")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("room required".into()))?;
                let from = input
                    .get("from")
                    .and_then(|v| v.as_str())
                    .unwrap_or("system");
                let data = input.get("data").cloned().unwrap_or(json!({}));
                let event_type = input
                    .get("event_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("room_message");
                let exclude = input
                    .get("exclude")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let mut state = self.state.write().await;

                let recipients: Vec<String> = state
                    .rooms
                    .get(room)
                    .map(|members| {
                        members
                            .iter()
                            .filter(|id| id.as_str() != exclude)
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();

                for id in &recipients {
                    if let Some(conn) = state.connections.get_mut(id) {
                        conn.message_count += 1;
                    }
                }

                let msg = WsMessage {
                    from: from.to_string(),
                    to: MessageTarget::Room(room.to_string()),
                    data: data.clone(),
                    timestamp: current_timestamp(),
                    event_type: event_type.to_string(),
                };
                state.message_history.push(msg);

                Ok(NodeOutput::data(json!({
                    "event": "room_message",
                    "room": room,
                    "from": from,
                    "event_type": event_type,
                    "data": data,
                    "recipients": recipients.len(),
                })))
            }

            "connections" => {
                let state = self.state.read().await;
                let room_filter = input.get("room").and_then(|v| v.as_str());

                let conns: Vec<Value> = state
                    .connections
                    .values()
                    .filter(|conn| {
                        room_filter
                            .map(|r| conn.rooms.contains(&r.to_string()))
                            .unwrap_or(true)
                    })
                    .map(|conn| {
                        json!({
                            "id": conn.id,
                            "metadata": conn.metadata,
                            "connected_at": conn.connected_at,
                            "rooms": conn.rooms,
                            "message_count": conn.message_count,
                        })
                    })
                    .collect();

                Ok(NodeOutput::data(json!({
                    "connections": conns,
                    "count": conns.len(),
                })))
            }

            "rooms" => {
                let state = self.state.read().await;
                let rooms: Vec<Value> = state
                    .rooms
                    .iter()
                    .map(|(name, members)| {
                        json!({
                            "room": name,
                            "members": members,
                            "size": members.len(),
                        })
                    })
                    .collect();

                Ok(NodeOutput::data(json!({
                    "rooms": rooms,
                    "count": rooms.len(),
                })))
            }

            "history" => {
                let state = self.state.read().await;
                let limit = input
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(50) as usize;
                let room_filter = input.get("room").and_then(|v| v.as_str());

                let messages: Vec<Value> = state
                    .message_history
                    .iter()
                    .rev()
                    .filter(|msg| {
                        if let Some(room) = room_filter {
                            matches!(&msg.to, MessageTarget::Room(r) if r == room)
                        } else {
                            true
                        }
                    })
                    .take(limit)
                    .map(|msg| {
                        let target = match &msg.to {
                            MessageTarget::Direct(id) => json!({"type": "direct", "id": id}),
                            MessageTarget::Room(name) => json!({"type": "room", "name": name}),
                            MessageTarget::Broadcast => json!({"type": "broadcast"}),
                        };
                        json!({
                            "from": msg.from,
                            "to": target,
                            "data": msg.data,
                            "event_type": msg.event_type,
                            "timestamp": msg.timestamp,
                        })
                    })
                    .collect();

                Ok(NodeOutput::data(json!({
                    "messages": messages,
                    "count": messages.len(),
                })))
            }

            "kick" => {
                let conn_id = input
                    .get("connection_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("connection_id required".into()))?;
                let reason = input
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("kicked");

                let mut state = self.state.write().await;

                if let Some(conn) = state.connections.remove(conn_id) {
                    for room in &conn.rooms {
                        if let Some(members) = state.rooms.get_mut(room) {
                            members.retain(|id| id != conn_id);
                        }
                    }
                    Ok(NodeOutput::data(json!({
                        "connection_id": conn_id,
                        "event": "kicked",
                        "reason": reason,
                        "total_connections": state.connections.len(),
                    })))
                } else {
                    Ok(NodeOutput::data(json!({
                        "connection_id": conn_id,
                        "event": "not_found",
                    })))
                }
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
    async fn test_connect_disconnect() {
        let mut node = WebSocketEnhancedNode::new();
        node.init(&json!({})).await.unwrap();

        let output = node
            .execute(
                json!({
                    "operation": "connect",
                    "connection_id": "user1",
                    "metadata": {"name": "Alice"}
                }),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["event"], "connected");
        assert_eq!(output.ports["default"]["total_connections"], 1);

        let output = node
            .execute(
                json!({"operation": "disconnect", "connection_id": "user1"}),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["event"], "disconnected");
        assert_eq!(output.ports["default"]["total_connections"], 0);
    }

    #[tokio::test]
    async fn test_rooms() {
        let mut node = WebSocketEnhancedNode::new();
        node.init(&json!({})).await.unwrap();

        // Connect two users
        node.execute(json!({"operation": "connect", "connection_id": "u1"}), &ctx())
            .await
            .unwrap();
        node.execute(json!({"operation": "connect", "connection_id": "u2"}), &ctx())
            .await
            .unwrap();

        // Join room
        node.execute(
            json!({"operation": "join_room", "connection_id": "u1", "room": "chat"}),
            &ctx(),
        )
        .await
        .unwrap();
        let output = node
            .execute(
                json!({"operation": "join_room", "connection_id": "u2", "room": "chat"}),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["room_size"], 2);

        // Room send
        let output = node
            .execute(
                json!({
                    "operation": "room_send",
                    "room": "chat",
                    "from": "u1",
                    "data": {"text": "hello"},
                    "exclude": "u1"
                }),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["recipients"], 1);
    }

    #[tokio::test]
    async fn test_broadcast() {
        let mut node = WebSocketEnhancedNode::new();
        node.init(&json!({})).await.unwrap();

        node.execute(json!({"operation": "connect", "connection_id": "a"}), &ctx())
            .await
            .unwrap();
        node.execute(json!({"operation": "connect", "connection_id": "b"}), &ctx())
            .await
            .unwrap();
        node.execute(json!({"operation": "connect", "connection_id": "c"}), &ctx())
            .await
            .unwrap();

        let output = node
            .execute(
                json!({
                    "operation": "broadcast",
                    "data": {"type": "announcement", "text": "Server restarting"},
                    "exclude": "a"
                }),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["recipients"], 2);
    }

    #[tokio::test]
    async fn test_history() {
        let mut node = WebSocketEnhancedNode::new();
        node.init(&json!({"max_history": 10})).await.unwrap();

        node.execute(json!({"operation": "connect", "connection_id": "u1"}), &ctx())
            .await
            .unwrap();
        node.execute(json!({"operation": "connect", "connection_id": "u2"}), &ctx())
            .await
            .unwrap();

        node.execute(
            json!({"operation": "send", "from": "u1", "to": "u2", "data": {"msg": "hi"}}),
            &ctx(),
        )
        .await
        .unwrap();

        let output = node
            .execute(json!({"operation": "history", "limit": 10}), &ctx())
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["count"], 1);
    }
}
