use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Finite State Machine (FSM) node.
/// Enables stateful, cyclic workflows with defined states, transitions,
/// guards, and actions. Overcomes the DAG limitation for iterative processes.
pub struct StateMachineNode {
    operation: String,
    states: Vec<String>,
    initial_state: String,
    transitions: Vec<Transition>,
    machines: Arc<RwLock<HashMap<String, MachineInstance>>>,
}

#[derive(Clone)]
struct Transition {
    from: String,
    to: String,
    event: String,
    guard_field: Option<String>,
    guard_op: Option<String>,
    guard_value: Option<Value>,
}

#[derive(Clone)]
struct MachineInstance {
    current_state: String,
    context: Value,
    history: Vec<StateTransition>,
    transition_count: u64,
}

#[derive(Clone)]
struct StateTransition {
    from: String,
    to: String,
    event: String,
    timestamp: u64,
}

impl StateMachineNode {
    pub fn new() -> Self {
        Self {
            operation: "transition".to_string(),
            states: Vec::new(),
            initial_state: "initial".to_string(),
            transitions: Vec::new(),
            machines: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for StateMachineNode {
    fn default() -> Self {
        Self::new()
    }
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn evaluate_guard(context: &Value, field: &str, op: &str, value: &Value) -> bool {
    let field_value = if let Some(v) = context.get(field) {
        v.clone()
    } else {
        Value::Null
    };

    match op {
        "eq" => field_value == *value,
        "neq" => field_value != *value,
        "gt" => {
            field_value.as_f64().unwrap_or(0.0) > value.as_f64().unwrap_or(0.0)
        }
        "gte" => {
            field_value.as_f64().unwrap_or(0.0) >= value.as_f64().unwrap_or(0.0)
        }
        "lt" => {
            field_value.as_f64().unwrap_or(0.0) < value.as_f64().unwrap_or(0.0)
        }
        "lte" => {
            field_value.as_f64().unwrap_or(0.0) <= value.as_f64().unwrap_or(0.0)
        }
        "exists" => !field_value.is_null(),
        "is_null" => field_value.is_null(),
        "contains" => {
            let sv = field_value.as_str().unwrap_or("");
            let pv = value.as_str().unwrap_or("");
            sv.contains(pv)
        }
        _ => true,
    }
}

#[async_trait]
impl NodeExecutor for StateMachineNode {
    fn node_type(&self) -> &str {
        "state_machine"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "create" | "transition" | "get_state" | "set_context" | "reset" | "can_transition"
                | "history" | "destroy" => {
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
        if let Some(states) = config.get("states").and_then(|v| v.as_array()) {
            self.states = states
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }
        if let Some(initial) = config.get("initial_state").and_then(|v| v.as_str()) {
            self.initial_state = initial.to_string();
        }
        if let Some(transitions) = config.get("transitions").and_then(|v| v.as_array()) {
            self.transitions = transitions
                .iter()
                .filter_map(|t| {
                    let from = t.get("from").and_then(|v| v.as_str())?.to_string();
                    let to = t.get("to").and_then(|v| v.as_str())?.to_string();
                    let event = t.get("event").and_then(|v| v.as_str())?.to_string();
                    Some(Transition {
                        from,
                        to,
                        event,
                        guard_field: t.get("guard_field").and_then(|v| v.as_str()).map(|s| s.to_string()),
                        guard_op: t.get("guard_op").and_then(|v| v.as_str()).map(|s| s.to_string()),
                        guard_value: t.get("guard_value").cloned(),
                    })
                })
                .collect();
        }
        Ok(())
    }

    async fn execute(&self, input: Value, _ctx: &ExecutionContext) -> NodeResult<NodeOutput> {
        let op = input
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.operation);
        let machine_id = input
            .get("machine_id")
            .and_then(|v| v.as_str())
            .unwrap_or("default");

        match op {
            "create" => {
                let initial = input
                    .get("initial_state")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&self.initial_state);
                let context = input.get("context").cloned().unwrap_or(json!({}));

                let instance = MachineInstance {
                    current_state: initial.to_string(),
                    context,
                    history: Vec::new(),
                    transition_count: 0,
                };

                let mut machines = self.machines.write().await;
                machines.insert(machine_id.to_string(), instance);

                Ok(NodeOutput::data(json!({
                    "machine_id": machine_id,
                    "state": initial,
                    "created": true,
                })))
            }

            "transition" => {
                let event = input
                    .get("event")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("event required".into()))?;

                let mut machines = self.machines.write().await;
                let machine = machines.get_mut(machine_id).ok_or_else(|| {
                    NodeError::ExecutionFailed(format!("machine '{}' not found", machine_id))
                })?;

                // Merge input data into context
                if let Some(data) = input.get("data") {
                    if let (Some(ctx_obj), Some(data_obj)) =
                        (machine.context.as_object_mut(), data.as_object())
                    {
                        for (k, v) in data_obj {
                            ctx_obj.insert(k.clone(), v.clone());
                        }
                    }
                }

                // Find matching transition
                let transition = self
                    .transitions
                    .iter()
                    .find(|t| {
                        (t.from == machine.current_state || t.from == "*") && t.event == event
                    })
                    .or_else(|| {
                        // Also check runtime transitions from input
                        None
                    });

                if let Some(t) = transition {
                    // Check guard condition
                    if let (Some(field), Some(op)) = (&t.guard_field, &t.guard_op) {
                        let value = t.guard_value.as_ref().cloned().unwrap_or(Value::Null);
                        if !evaluate_guard(&machine.context, field, op, &value) {
                            return Ok(NodeOutput::data(json!({
                                "machine_id": machine_id,
                                "state": machine.current_state,
                                "event": event,
                                "transitioned": false,
                                "reason": "guard_failed",
                            })));
                        }
                    }

                    let from = machine.current_state.clone();
                    let to = t.to.clone();

                    machine.history.push(StateTransition {
                        from: from.clone(),
                        to: to.clone(),
                        event: event.to_string(),
                        timestamp: current_timestamp(),
                    });
                    machine.current_state = to.clone();
                    machine.transition_count += 1;

                    // Output on a port named after the new state
                    let mut ports = HashMap::new();
                    ports.insert(
                        "default".to_string(),
                        json!({
                            "machine_id": machine_id,
                            "from": from,
                            "to": to,
                            "event": event,
                            "state": &machine.current_state,
                            "context": &machine.context,
                            "transition_count": machine.transition_count,
                            "transitioned": true,
                        }),
                    );
                    ports.insert(
                        to,
                        json!({
                            "machine_id": machine_id,
                            "event": event,
                            "context": &machine.context,
                        }),
                    );

                    Ok(NodeOutput::multi(ports))
                } else {
                    Ok(NodeOutput::data(json!({
                        "machine_id": machine_id,
                        "state": machine.current_state,
                        "event": event,
                        "transitioned": false,
                        "reason": "no_matching_transition",
                    })))
                }
            }

            "get_state" => {
                let machines = self.machines.read().await;
                let machine = machines.get(machine_id).ok_or_else(|| {
                    NodeError::ExecutionFailed(format!("machine '{}' not found", machine_id))
                })?;

                Ok(NodeOutput::data(json!({
                    "machine_id": machine_id,
                    "state": machine.current_state,
                    "context": machine.context,
                    "transition_count": machine.transition_count,
                })))
            }

            "set_context" => {
                let context = input
                    .get("context")
                    .ok_or_else(|| NodeError::MissingInput("context required".into()))?;

                let mut machines = self.machines.write().await;
                let machine = machines.get_mut(machine_id).ok_or_else(|| {
                    NodeError::ExecutionFailed(format!("machine '{}' not found", machine_id))
                })?;

                if let (Some(ctx_obj), Some(new_obj)) =
                    (machine.context.as_object_mut(), context.as_object())
                {
                    for (k, v) in new_obj {
                        ctx_obj.insert(k.clone(), v.clone());
                    }
                }

                Ok(NodeOutput::data(json!({
                    "machine_id": machine_id,
                    "state": machine.current_state,
                    "context": machine.context,
                })))
            }

            "can_transition" => {
                let event = input
                    .get("event")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("event required".into()))?;

                let machines = self.machines.read().await;
                let machine = machines.get(machine_id).ok_or_else(|| {
                    NodeError::ExecutionFailed(format!("machine '{}' not found", machine_id))
                })?;

                let can = self.transitions.iter().any(|t| {
                    (t.from == machine.current_state || t.from == "*") && t.event == event
                });

                let available: Vec<&str> = self
                    .transitions
                    .iter()
                    .filter(|t| t.from == machine.current_state || t.from == "*")
                    .map(|t| t.event.as_str())
                    .collect();

                Ok(NodeOutput::data(json!({
                    "machine_id": machine_id,
                    "state": machine.current_state,
                    "event": event,
                    "can_transition": can,
                    "available_events": available,
                })))
            }

            "history" => {
                let machines = self.machines.read().await;
                let machine = machines.get(machine_id).ok_or_else(|| {
                    NodeError::ExecutionFailed(format!("machine '{}' not found", machine_id))
                })?;

                let history: Vec<Value> = machine
                    .history
                    .iter()
                    .map(|h| {
                        json!({
                            "from": h.from,
                            "to": h.to,
                            "event": h.event,
                            "timestamp": h.timestamp,
                        })
                    })
                    .collect();

                Ok(NodeOutput::data(json!({
                    "machine_id": machine_id,
                    "state": machine.current_state,
                    "history": history,
                    "transition_count": machine.transition_count,
                })))
            }

            "reset" => {
                let initial = input
                    .get("initial_state")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&self.initial_state);

                let mut machines = self.machines.write().await;
                let machine = machines.get_mut(machine_id).ok_or_else(|| {
                    NodeError::ExecutionFailed(format!("machine '{}' not found", machine_id))
                })?;

                machine.current_state = initial.to_string();
                machine.context = json!({});
                machine.history.clear();
                machine.transition_count = 0;

                Ok(NodeOutput::data(json!({
                    "machine_id": machine_id,
                    "state": initial,
                    "reset": true,
                })))
            }

            "destroy" => {
                let mut machines = self.machines.write().await;
                let existed = machines.remove(machine_id).is_some();
                Ok(NodeOutput::data(json!({
                    "machine_id": machine_id,
                    "destroyed": existed,
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
    async fn test_create_and_transition() {
        let mut node = StateMachineNode::new();
        node.init(&json!({
            "initial_state": "idle",
            "states": ["idle", "running", "paused", "done"],
            "transitions": [
                {"from": "idle", "event": "start", "to": "running"},
                {"from": "running", "event": "pause", "to": "paused"},
                {"from": "paused", "event": "resume", "to": "running"},
                {"from": "running", "event": "finish", "to": "done"}
            ]
        }))
        .await
        .unwrap();

        // Create machine
        let output = node
            .execute(
                json!({"operation": "create", "machine_id": "m1"}),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["state"], "idle");

        // Transition: start
        let output = node
            .execute(
                json!({"operation": "transition", "machine_id": "m1", "event": "start"}),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["state"], "running");
        assert_eq!(output.ports["default"]["transitioned"], true);

        // Transition: pause
        let output = node
            .execute(
                json!({"operation": "transition", "machine_id": "m1", "event": "pause"}),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["state"], "paused");

        // Invalid transition
        let output = node
            .execute(
                json!({"operation": "transition", "machine_id": "m1", "event": "finish"}),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["transitioned"], false);
    }

    #[tokio::test]
    async fn test_guard_conditions() {
        let mut node = StateMachineNode::new();
        node.init(&json!({
            "initial_state": "pending",
            "transitions": [
                {
                    "from": "pending",
                    "event": "approve",
                    "to": "approved",
                    "guard_field": "amount",
                    "guard_op": "lte",
                    "guard_value": 1000
                }
            ]
        }))
        .await
        .unwrap();

        node.execute(
            json!({"operation": "create", "machine_id": "m1", "context": {"amount": 500}}),
            &ctx(),
        )
        .await
        .unwrap();

        // Guard passes
        let output = node
            .execute(
                json!({"operation": "transition", "machine_id": "m1", "event": "approve"}),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["transitioned"], true);

        // Now create another with amount > 1000
        node.execute(
            json!({"operation": "create", "machine_id": "m2", "context": {"amount": 2000}}),
            &ctx(),
        )
        .await
        .unwrap();

        let output = node
            .execute(
                json!({"operation": "transition", "machine_id": "m2", "event": "approve"}),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["transitioned"], false);
        assert_eq!(output.ports["default"]["reason"], "guard_failed");
    }

    #[tokio::test]
    async fn test_history() {
        let mut node = StateMachineNode::new();
        node.init(&json!({
            "initial_state": "a",
            "transitions": [
                {"from": "a", "event": "go", "to": "b"},
                {"from": "b", "event": "go", "to": "c"}
            ]
        }))
        .await
        .unwrap();

        node.execute(json!({"operation": "create", "machine_id": "h1"}), &ctx())
            .await
            .unwrap();
        node.execute(
            json!({"operation": "transition", "machine_id": "h1", "event": "go"}),
            &ctx(),
        )
        .await
        .unwrap();
        node.execute(
            json!({"operation": "transition", "machine_id": "h1", "event": "go"}),
            &ctx(),
        )
        .await
        .unwrap();

        let output = node
            .execute(
                json!({"operation": "history", "machine_id": "h1"}),
                &ctx(),
            )
            .await
            .unwrap();
        let history = output.ports["default"]["history"].as_array().unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(output.ports["default"]["state"], "c");
    }

    #[tokio::test]
    async fn test_wildcard_transition() {
        let mut node = StateMachineNode::new();
        node.init(&json!({
            "initial_state": "running",
            "transitions": [
                {"from": "*", "event": "error", "to": "failed"}
            ]
        }))
        .await
        .unwrap();

        node.execute(json!({"operation": "create", "machine_id": "w1"}), &ctx())
            .await
            .unwrap();
        let output = node
            .execute(
                json!({"operation": "transition", "machine_id": "w1", "event": "error"}),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["state"], "failed");
    }
}
