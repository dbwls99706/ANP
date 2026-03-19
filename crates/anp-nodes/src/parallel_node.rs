use std::collections::HashMap;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// ParallelNode — fan-out to multiple ports simultaneously.
///
/// Config example:
/// ```json
/// {
///   "ports": ["port_a", "port_b", "port_c"]
/// }
/// ```
///
/// Takes input and sends the SAME data to ALL configured output ports
/// simultaneously. This enables parallel execution paths in the graph.
pub struct ParallelNode {
    ports: Vec<String>,
}

impl Default for ParallelNode {
    fn default() -> Self {
        Self::new()
    }
}

impl ParallelNode {
    pub fn new() -> Self {
        Self { ports: Vec::new() }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for ParallelNode {
    fn node_type(&self) -> &str {
        "parallel"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        let ports_arr = config
            .get("ports")
            .and_then(|v| v.as_array())
            .ok_or_else(|| NodeError::InvalidConfig("missing 'ports' array in config".into()))?;

        if ports_arr.is_empty() {
            return Err(NodeError::InvalidConfig(
                "ports array must not be empty".into(),
            ));
        }

        self.ports.clear();
        for (i, port) in ports_arr.iter().enumerate() {
            let name = port.as_str().ok_or_else(|| {
                NodeError::InvalidConfig(format!("port at index {} must be a string", i))
            })?;
            if name.is_empty() {
                return Err(NodeError::InvalidConfig(format!(
                    "port at index {} has empty name",
                    i
                )));
            }
            self.ports.push(name.to_string());
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let mut ports = HashMap::new();
        for port_name in &self.ports {
            ports.insert(port_name.clone(), input.clone());
        }

        Ok(NodeOutput::multi(ports).with_log(format!(
            "fan-out to {} ports: {}",
            self.ports.len(),
            self.ports.join(", ")
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fan_out_to_all_ports() {
        let mut node = ParallelNode::new();
        node.init(&serde_json::json!({
            "ports": ["port_a", "port_b", "port_c"]
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"data": "hello"}), &ctx)
            .await
            .unwrap();

        assert_eq!(output.ports.len(), 3);
        assert!(output.ports.contains_key("port_a"));
        assert!(output.ports.contains_key("port_b"));
        assert!(output.ports.contains_key("port_c"));
        // All ports get the same data
        assert_eq!(output.ports["port_a"]["data"], "hello");
        assert_eq!(output.ports["port_b"]["data"], "hello");
        assert_eq!(output.ports["port_c"]["data"], "hello");
    }

    #[tokio::test]
    async fn test_single_port() {
        let mut node = ParallelNode::new();
        node.init(&serde_json::json!({
            "ports": ["only"]
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::json!(42), &ctx).await.unwrap();

        assert_eq!(output.ports.len(), 1);
        assert!(output.ports.contains_key("only"));
        assert_eq!(output.ports["only"], 42);
    }

    #[tokio::test]
    async fn test_init_missing_ports() {
        let mut node = ParallelNode::new();
        let result = node.init(&serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_init_empty_ports() {
        let mut node = ParallelNode::new();
        let result = node.init(&serde_json::json!({"ports": []})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_init_empty_port_name() {
        let mut node = ParallelNode::new();
        let result = node
            .init(&serde_json::json!({"ports": ["valid", ""]}))
            .await;
        assert!(result.is_err());
    }
}
