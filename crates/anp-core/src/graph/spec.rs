use serde::{Deserialize, Serialize};
use std::fmt;

/// Complete ANP Graph definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnpGraph {
    pub version: GraphVersion,
    pub metadata: GraphMetadata,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// Graph version for forward compatibility
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphVersion {
    #[serde(rename = "0.1")]
    V0_1,
}

impl fmt::Display for GraphVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphVersion::V0_1 => write!(f, "0.1"),
        }
    }
}

/// Graph metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphMetadata {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// A node in the ANP graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub node_type: String,
    pub kind: NodeKind,
    #[serde(default = "default_config")]
    pub config: serde_json::Value,
    #[serde(default)]
    pub inputs: Vec<PortSpec>,
    #[serde(default)]
    pub outputs: Vec<PortSpec>,
}

fn default_config() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

/// Node classification for runtime behavior
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// Data producer, executes once (e.g., file_read)
    Source,
    /// Data transformer (e.g., transform, filter, database)
    Processor,
    /// Data consumer (e.g., console, file_write)
    Sink,
    /// Long-running event loop (e.g., http_server)
    Service,
}

impl fmt::Display for NodeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeKind::Source => write!(f, "source"),
            NodeKind::Processor => write!(f, "processor"),
            NodeKind::Sink => write!(f, "sink"),
            NodeKind::Service => write!(f, "service"),
        }
    }
}

/// Port specification for node inputs/outputs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortSpec {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

impl PortSpec {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
        }
    }
}

/// An edge connecting two nodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub id: String,
    pub from_node: String,
    #[serde(default = "default_port")]
    pub from_port: String,
    pub to_node: String,
    #[serde(default = "default_port")]
    pub to_port: String,
}

fn default_port() -> String {
    "default".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_graph() {
        let json = r#"{
            "version": "0.1",
            "metadata": { "name": "test", "description": "A test graph" },
            "nodes": [
                {
                    "id": "n1",
                    "node_type": "console",
                    "kind": "sink",
                    "config": { "message": "hello" },
                    "outputs": []
                }
            ],
            "edges": []
        }"#;
        let graph: AnpGraph = serde_json::from_str(json).unwrap();
        assert_eq!(graph.version, GraphVersion::V0_1);
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].kind, NodeKind::Sink);
    }

    #[test]
    fn test_node_kind_serialization() {
        let kind = NodeKind::Service;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, "\"service\"");
    }

    #[test]
    fn test_edge_default_ports() {
        let json = r#"{
            "id": "e1",
            "from_node": "a",
            "to_node": "b"
        }"#;
        let edge: Edge = serde_json::from_str(json).unwrap();
        assert_eq!(edge.from_port, "default");
        assert_eq!(edge.to_port, "default");
    }
}
