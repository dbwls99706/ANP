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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::traits::*;
    use crate::runtime::context::ExecutionContext;

    struct TestNode;

    #[async_trait::async_trait]
    impl NodeExecutor for TestNode {
        fn node_type(&self) -> &str {
            "test_node"
        }
        fn node_kind(&self) -> NodeKind {
            NodeKind::Processor
        }
        async fn init(&mut self, _config: &serde_json::Value) -> NodeResult<()> {
            Ok(())
        }
        async fn execute(
            &self,
            _input: serde_json::Value,
            _ctx: &ExecutionContext,
        ) -> NodeResult<NodeOutput> {
            Ok(NodeOutput::data(serde_json::json!("test")))
        }
    }

    fn test_schema(name: &str, kind: NodeKind) -> NodeSchema {
        NodeSchema {
            node_type: name.to_string(),
            kind,
            description: format!("test {}", name),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({}),
        }
    }

    #[test]
    fn test_register_and_create() {
        let mut registry = NodeRegistry::new();
        registry.register(NodeInfo {
            factory: || Box::new(TestNode),
            schema: test_schema("test_node", NodeKind::Processor),
        });

        let node = registry.create_node("test_node");
        assert!(node.is_some());
        assert_eq!(node.unwrap().node_type(), "test_node");
    }

    #[test]
    fn test_create_unknown_returns_none() {
        let registry = NodeRegistry::new();
        assert!(registry.create_node("nonexistent").is_none());
    }

    #[test]
    fn test_has() {
        let mut registry = NodeRegistry::new();
        assert!(!registry.has("test_node"));

        registry.register(NodeInfo {
            factory: || Box::new(TestNode),
            schema: test_schema("test_node", NodeKind::Processor),
        });
        assert!(registry.has("test_node"));
    }

    #[test]
    fn test_get_schema() {
        let mut registry = NodeRegistry::new();
        registry.register(NodeInfo {
            factory: || Box::new(TestNode),
            schema: test_schema("test_node", NodeKind::Processor),
        });

        let schema = registry.get_schema("test_node").unwrap();
        assert_eq!(schema.node_type, "test_node");
        assert_eq!(schema.kind, NodeKind::Processor);
        assert_eq!(schema.description, "test test_node");

        assert!(registry.get_schema("nonexistent").is_none());
    }

    #[test]
    fn test_node_types() {
        let mut registry = NodeRegistry::new();
        registry.register(NodeInfo {
            factory: || Box::new(TestNode),
            schema: test_schema("alpha", NodeKind::Processor),
        });
        registry.register(NodeInfo {
            factory: || Box::new(TestNode),
            schema: test_schema("beta", NodeKind::Source),
        });

        let types = registry.node_types();
        assert_eq!(types.len(), 2);
        assert!(types.contains(&"alpha"));
        assert!(types.contains(&"beta"));
    }

    #[test]
    fn test_register_overwrites() {
        let mut registry = NodeRegistry::new();
        registry.register(NodeInfo {
            factory: || Box::new(TestNode),
            schema: test_schema("test_node", NodeKind::Processor),
        });
        registry.register(NodeInfo {
            factory: || Box::new(TestNode),
            schema: NodeSchema {
                node_type: "test_node".to_string(),
                kind: NodeKind::Source,
                description: "overwritten".to_string(),
                inputs: vec![],
                outputs: vec![PortSpec::new("default")],
                config_schema: serde_json::json!({}),
            },
        });

        // Should have the overwritten version
        let schema = registry.get_schema("test_node").unwrap();
        assert_eq!(schema.kind, NodeKind::Source);
        assert_eq!(schema.description, "overwritten");
        assert_eq!(registry.node_types().len(), 1);
    }

    #[test]
    fn test_default() {
        let registry = NodeRegistry::default();
        assert_eq!(registry.node_types().len(), 0);
    }
}
