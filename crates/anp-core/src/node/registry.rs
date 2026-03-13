use std::collections::HashMap;

use crate::graph::spec::{NodeKind, PortSpec};
use crate::node::traits::NodeExecutor;

/// Factory function to create a node instance
pub type NodeFactory = fn() -> Box<dyn NodeExecutor>;

/// Metadata about a node type (used by LLM planner in v0.2)
#[derive(Debug, Clone)]
pub struct NodeSchema {
    pub node_type: String,
    pub kind: NodeKind,
    pub description: String,
    pub inputs: Vec<PortSpec>,
    pub outputs: Vec<PortSpec>,
    pub config_schema: serde_json::Value,
}

/// Information about a registered node type
pub struct NodeInfo {
    pub factory: NodeFactory,
    pub schema: NodeSchema,
}

/// Registry of all available node types
pub struct NodeRegistry {
    map: HashMap<String, NodeInfo>,
}

impl NodeRegistry {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Register a new node type
    pub fn register(&mut self, info: NodeInfo) {
        self.map.insert(info.schema.node_type.clone(), info);
    }

    /// Create a new instance of a node by type name
    pub fn create_node(&self, node_type: &str) -> Option<Box<dyn NodeExecutor>> {
        self.map.get(node_type).map(|info| (info.factory)())
    }

    /// Get the schema for a node type
    pub fn get_schema(&self, node_type: &str) -> Option<&NodeSchema> {
        self.map.get(node_type).map(|info| &info.schema)
    }

    /// List all registered node types
    pub fn node_types(&self) -> Vec<&str> {
        self.map.keys().map(|s| s.as_str()).collect()
    }

    /// Check if a node type is registered
    pub fn has(&self, node_type: &str) -> bool {
        self.map.contains_key(node_type)
    }
}

impl Default for NodeRegistry {
    fn default() -> Self {
        Self::new()
    }
}
