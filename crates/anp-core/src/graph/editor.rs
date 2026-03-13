use crate::graph::spec::{AnpGraph, Edge, Node};
use crate::node::registry::NodeRegistry;

/// A single graph mutation operation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum GraphOp {
    /// Add a new node to the graph.
    AddNode {
        id: String,
        node_type: String,
        #[serde(default)]
        config: Option<serde_json::Value>,
    },

    /// Remove a node and all its connected edges.
    RemoveNode { id: String },

    /// Connect two nodes with an edge.
    Connect {
        from_node: String,
        #[serde(default)]
        from_port: Option<String>,
        to_node: String,
        #[serde(default)]
        to_port: Option<String>,
    },

    /// Remove all edges between two nodes.
    Disconnect {
        from_node: String,
        to_node: String,
    },

    /// Update a config key on a node.
    UpdateConfig {
        node_id: String,
        key: String,
        value: serde_json::Value,
    },

    /// Insert a new node after a target, rewiring edges.
    /// target → [downstream...] becomes target → new_node → [downstream...]
    InsertAfter {
        target: String,
        new_id: String,
        node_type: String,
    },
}

/// Errors from graph editing operations.
#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("node '{0}' already exists")]
    DuplicateNode(String),

    #[error("node '{0}' not found")]
    NodeNotFound(String),

    #[error("unknown node type '{0}'")]
    UnknownNodeType(String),

    #[error("edge from '{from}' to '{to}' already exists")]
    EdgeExists { from: String, to: String },

    #[error("cannot connect node '{0}' to itself")]
    SelfLoop(String),
}

pub type EditResult<T> = Result<T, EditError>;

/// Graph editor that applies operations to an AnpGraph.
/// Uses NodeRegistry to look up node metadata (kind, ports) when adding nodes.
pub struct GraphEditor<'a> {
    graph: &'a mut AnpGraph,
    registry: &'a NodeRegistry,
    edge_counter: usize,
}

impl<'a> GraphEditor<'a> {
    pub fn new(graph: &'a mut AnpGraph, registry: &'a NodeRegistry) -> Self {
        // Start counter after the highest existing edge number
        let edge_counter = graph
            .edges
            .iter()
            .filter_map(|e| {
                e.id.strip_prefix('e')
                    .and_then(|n| n.parse::<usize>().ok())
            })
            .max()
            .unwrap_or(0);

        Self {
            graph,
            registry,
            edge_counter,
        }
    }

    /// Apply a single operation.
    pub fn apply(&mut self, op: GraphOp) -> EditResult<()> {
        match op {
            GraphOp::AddNode {
                id,
                node_type,
                config,
            } => self.add_node(id, node_type, config),
            GraphOp::RemoveNode { id } => self.remove_node(&id),
            GraphOp::Connect {
                from_node,
                from_port,
                to_node,
                to_port,
            } => self.connect(&from_node, from_port, &to_node, to_port),
            GraphOp::Disconnect {
                from_node,
                to_node,
            } => self.disconnect(&from_node, &to_node),
            GraphOp::UpdateConfig {
                node_id,
                key,
                value,
            } => self.update_config(&node_id, &key, value),
            GraphOp::InsertAfter {
                target,
                new_id,
                node_type,
            } => self.insert_after(&target, new_id, node_type),
        }
    }

    /// Apply a batch of operations in order.
    pub fn apply_all(&mut self, ops: Vec<GraphOp>) -> EditResult<()> {
        for op in ops {
            self.apply(op)?;
        }
        Ok(())
    }

    fn add_node(
        &mut self,
        id: String,
        node_type: String,
        config: Option<serde_json::Value>,
    ) -> EditResult<()> {
        // Check duplicate
        if self.graph.nodes.iter().any(|n| n.id == id) {
            return Err(EditError::DuplicateNode(id));
        }

        // Look up schema for kind + ports
        let schema = self
            .registry
            .get_schema(&node_type)
            .ok_or_else(|| EditError::UnknownNodeType(node_type.clone()))?;

        let node = Node {
            id,
            node_type,
            kind: schema.kind,
            config: config.unwrap_or_else(|| serde_json::json!({})),
            inputs: schema.inputs.clone(),
            outputs: schema.outputs.clone(),
        };

        self.graph.nodes.push(node);
        Ok(())
    }

    fn remove_node(&mut self, id: &str) -> EditResult<()> {
        let pos = self
            .graph
            .nodes
            .iter()
            .position(|n| n.id == id)
            .ok_or_else(|| EditError::NodeNotFound(id.to_string()))?;

        self.graph.nodes.remove(pos);

        // Remove all edges involving this node
        self.graph
            .edges
            .retain(|e| e.from_node != id && e.to_node != id);

        Ok(())
    }

    fn connect(
        &mut self,
        from_node: &str,
        from_port: Option<String>,
        to_node: &str,
        to_port: Option<String>,
    ) -> EditResult<()> {
        // Prevent self-loops
        if from_node == to_node {
            return Err(EditError::SelfLoop(from_node.to_string()));
        }

        // Verify nodes exist
        if !self.graph.nodes.iter().any(|n| n.id == from_node) {
            return Err(EditError::NodeNotFound(from_node.to_string()));
        }
        if !self.graph.nodes.iter().any(|n| n.id == to_node) {
            return Err(EditError::NodeNotFound(to_node.to_string()));
        }

        let from_p = from_port.unwrap_or_else(|| "default".to_string());
        let to_p = to_port.unwrap_or_else(|| "default".to_string());

        // Check if edge already exists
        if self
            .graph
            .edges
            .iter()
            .any(|e| e.from_node == from_node && e.to_node == to_node)
        {
            return Err(EditError::EdgeExists {
                from: from_node.to_string(),
                to: to_node.to_string(),
            });
        }

        self.edge_counter += 1;
        let edge = Edge {
            id: format!("e{}", self.edge_counter),
            from_node: from_node.to_string(),
            from_port: from_p,
            to_node: to_node.to_string(),
            to_port: to_p,
        };

        self.graph.edges.push(edge);
        Ok(())
    }

    fn disconnect(&mut self, from_node: &str, to_node: &str) -> EditResult<()> {
        let before = self.graph.edges.len();
        self.graph
            .edges
            .retain(|e| !(e.from_node == from_node && e.to_node == to_node));

        if self.graph.edges.len() == before {
            // No edges were removed — not an error, just a no-op
        }

        Ok(())
    }

    fn update_config(
        &mut self,
        node_id: &str,
        key: &str,
        value: serde_json::Value,
    ) -> EditResult<()> {
        let node = self
            .graph
            .nodes
            .iter_mut()
            .find(|n| n.id == node_id)
            .ok_or_else(|| EditError::NodeNotFound(node_id.to_string()))?;

        if let Some(obj) = node.config.as_object_mut() {
            obj.insert(key.to_string(), value);
        }

        Ok(())
    }

    fn insert_after(
        &mut self,
        target: &str,
        new_id: String,
        node_type: String,
    ) -> EditResult<()> {
        // Verify target exists
        if !self.graph.nodes.iter().any(|n| n.id == target) {
            return Err(EditError::NodeNotFound(target.to_string()));
        }

        // Add the new node
        self.add_node(new_id.clone(), node_type, None)?;

        // Find all outgoing edges from target
        let outgoing: Vec<Edge> = self
            .graph
            .edges
            .iter()
            .filter(|e| e.from_node == target)
            .cloned()
            .collect();

        // Remove outgoing edges from target
        self.graph.edges.retain(|e| e.from_node != target);

        // Rewire: target → new_node, new_node → each original downstream
        self.connect(target, None, &new_id, None)?;

        for edge in outgoing {
            self.connect(
                &new_id,
                None,
                &edge.to_node,
                Some(edge.to_port),
            )?;
        }

        Ok(())
    }

    /// Get a reference to the modified graph.
    pub fn graph(&self) -> &AnpGraph {
        self.graph
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::spec::{GraphMetadata, GraphVersion, NodeKind, PortSpec};
    use crate::node::registry::{NodeInfo, NodeSchema};
    use crate::node::traits::{NodeExecutor, NodeOutput, NodeResult};
    use crate::runtime::context::ExecutionContext;

    /// Stub node — only used so NodeRegistry has a valid factory.
    struct StubNode;

    #[async_trait::async_trait]
    impl NodeExecutor for StubNode {
        fn node_type(&self) -> &str { "stub" }
        fn node_kind(&self) -> NodeKind { NodeKind::Processor }
        async fn init(&mut self, _: &serde_json::Value) -> NodeResult<()> { Ok(()) }
        async fn execute(&self, _: serde_json::Value, _: &ExecutionContext) -> NodeResult<NodeOutput> {
            Ok(NodeOutput::data(serde_json::json!(null)))
        }
    }

    fn register_type(r: &mut NodeRegistry, name: &str, kind: NodeKind, inputs: &[&str], outputs: &[&str]) {
        r.register(NodeInfo {
            factory: || Box::new(StubNode),
            schema: NodeSchema {
                node_type: name.to_string(),
                kind,
                description: format!("Test {}", name),
                inputs: inputs.iter().map(|n| PortSpec::new(*n)).collect(),
                outputs: outputs.iter().map(|n| PortSpec::new(*n)).collect(),
                config_schema: serde_json::json!({}),
            },
        });
    }

    fn test_registry() -> NodeRegistry {
        let mut r = NodeRegistry::new();
        register_type(&mut r, "file_read", NodeKind::Source, &[], &["default"]);
        register_type(&mut r, "console", NodeKind::Sink, &["default"], &[]);
        register_type(&mut r, "transform", NodeKind::Processor, &["default"], &["default"]);
        register_type(&mut r, "filter", NodeKind::Processor, &["default"], &["default"]);
        register_type(&mut r, "database", NodeKind::Processor, &["default"], &["default"]);
        register_type(&mut r, "http_server", NodeKind::Service, &[], &["default"]);
        r
    }

    fn empty_graph() -> AnpGraph {
        AnpGraph {
            version: GraphVersion::V0_1,
            metadata: GraphMetadata {
                name: "test".to_string(),
                description: String::new(),
            },
            nodes: vec![],
            edges: vec![],
        }
    }

    fn simple_pipeline() -> AnpGraph {
        // file_read → console
        let json = r#"{
            "version": "0.1",
            "metadata": { "name": "pipeline", "description": "" },
            "nodes": [
                { "id": "src", "node_type": "file_read", "kind": "source",
                  "config": { "path": "data.txt" },
                  "outputs": [{ "name": "default" }] },
                { "id": "out", "node_type": "console", "kind": "sink",
                  "inputs": [{ "name": "default" }] }
            ],
            "edges": [
                { "id": "e1", "from_node": "src", "to_node": "out" }
            ]
        }"#;
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn test_add_node() {
        let registry = test_registry();
        let mut graph = empty_graph();
        let mut editor = GraphEditor::new(&mut graph, &registry);

        editor
            .apply(GraphOp::AddNode {
                id: "db".to_string(),
                node_type: "database".to_string(),
                config: None,
            })
            .unwrap();

        assert_eq!(editor.graph().nodes.len(), 1);
        assert_eq!(editor.graph().nodes[0].id, "db");
        assert_eq!(editor.graph().nodes[0].kind, NodeKind::Processor);
    }

    #[test]
    fn test_add_duplicate_node_fails() {
        let registry = test_registry();
        let mut graph = empty_graph();
        let mut editor = GraphEditor::new(&mut graph, &registry);

        editor
            .apply(GraphOp::AddNode {
                id: "db".to_string(),
                node_type: "database".to_string(),
                config: None,
            })
            .unwrap();

        let result = editor.apply(GraphOp::AddNode {
            id: "db".to_string(),
            node_type: "console".to_string(),
            config: None,
        });

        assert!(matches!(result, Err(EditError::DuplicateNode(_))));
    }

    #[test]
    fn test_add_unknown_type_fails() {
        let registry = test_registry();
        let mut graph = empty_graph();
        let mut editor = GraphEditor::new(&mut graph, &registry);

        let result = editor.apply(GraphOp::AddNode {
            id: "x".to_string(),
            node_type: "nonexistent".to_string(),
            config: None,
        });

        assert!(matches!(result, Err(EditError::UnknownNodeType(_))));
    }

    #[test]
    fn test_remove_node() {
        let registry = test_registry();
        let mut graph = simple_pipeline();
        let mut editor = GraphEditor::new(&mut graph, &registry);

        assert_eq!(editor.graph().nodes.len(), 2);
        assert_eq!(editor.graph().edges.len(), 1);

        editor
            .apply(GraphOp::RemoveNode {
                id: "out".to_string(),
            })
            .unwrap();

        assert_eq!(editor.graph().nodes.len(), 1);
        assert_eq!(editor.graph().edges.len(), 0); // edge removed too
    }

    #[test]
    fn test_remove_nonexistent_fails() {
        let registry = test_registry();
        let mut graph = empty_graph();
        let mut editor = GraphEditor::new(&mut graph, &registry);

        let result = editor.apply(GraphOp::RemoveNode {
            id: "nope".to_string(),
        });

        assert!(matches!(result, Err(EditError::NodeNotFound(_))));
    }

    #[test]
    fn test_connect() {
        let registry = test_registry();
        let mut graph = empty_graph();
        let mut editor = GraphEditor::new(&mut graph, &registry);

        editor
            .apply(GraphOp::AddNode {
                id: "a".to_string(),
                node_type: "file_read".to_string(),
                config: Some(serde_json::json!({"path": "x.txt"})),
            })
            .unwrap();

        editor
            .apply(GraphOp::AddNode {
                id: "b".to_string(),
                node_type: "console".to_string(),
                config: None,
            })
            .unwrap();

        editor
            .apply(GraphOp::Connect {
                from_node: "a".to_string(),
                from_port: None,
                to_node: "b".to_string(),
                to_port: None,
            })
            .unwrap();

        assert_eq!(editor.graph().edges.len(), 1);
        assert_eq!(editor.graph().edges[0].from_node, "a");
        assert_eq!(editor.graph().edges[0].to_node, "b");
        assert_eq!(editor.graph().edges[0].from_port, "default");
    }

    #[test]
    fn test_connect_nonexistent_node_fails() {
        let registry = test_registry();
        let mut graph = empty_graph();
        let mut editor = GraphEditor::new(&mut graph, &registry);

        let result = editor.apply(GraphOp::Connect {
            from_node: "ghost".to_string(),
            from_port: None,
            to_node: "phantom".to_string(),
            to_port: None,
        });

        assert!(matches!(result, Err(EditError::NodeNotFound(_))));
    }

    #[test]
    fn test_disconnect() {
        let registry = test_registry();
        let mut graph = simple_pipeline();
        let mut editor = GraphEditor::new(&mut graph, &registry);

        assert_eq!(editor.graph().edges.len(), 1);

        editor
            .apply(GraphOp::Disconnect {
                from_node: "src".to_string(),
                to_node: "out".to_string(),
            })
            .unwrap();

        assert_eq!(editor.graph().edges.len(), 0);
    }

    #[test]
    fn test_update_config() {
        let registry = test_registry();
        let mut graph = simple_pipeline();
        let mut editor = GraphEditor::new(&mut graph, &registry);

        editor
            .apply(GraphOp::UpdateConfig {
                node_id: "src".to_string(),
                key: "path".to_string(),
                value: serde_json::json!("new_data.txt"),
            })
            .unwrap();

        let node = &editor.graph().nodes[0];
        assert_eq!(node.config["path"], "new_data.txt");
    }

    #[test]
    fn test_insert_after_rewires_edges() {
        let registry = test_registry();
        let mut graph = simple_pipeline();
        // Before: src → out
        let mut editor = GraphEditor::new(&mut graph, &registry);

        editor
            .apply(GraphOp::InsertAfter {
                target: "src".to_string(),
                new_id: "mid".to_string(),
                node_type: "transform".to_string(),
            })
            .unwrap();

        // After: src → mid → out
        assert_eq!(editor.graph().nodes.len(), 3);
        assert_eq!(editor.graph().edges.len(), 2);

        let edges = &editor.graph().edges;
        assert!(edges.iter().any(|e| e.from_node == "src" && e.to_node == "mid"));
        assert!(edges.iter().any(|e| e.from_node == "mid" && e.to_node == "out"));
        // Old direct edge should be gone
        assert!(!edges.iter().any(|e| e.from_node == "src" && e.to_node == "out"));
    }

    #[test]
    fn test_insert_after_multiple_downstream() {
        // src → out1, src → out2
        // InsertAfter(src, mid) → src → mid → out1, mid → out2
        let registry = test_registry();
        let mut graph = empty_graph();
        let mut editor = GraphEditor::new(&mut graph, &registry);

        editor.apply_all(vec![
            GraphOp::AddNode {
                id: "src".to_string(),
                node_type: "file_read".to_string(),
                config: Some(serde_json::json!({"path": "x.txt"})),
            },
            GraphOp::AddNode {
                id: "out1".to_string(),
                node_type: "console".to_string(),
                config: None,
            },
            GraphOp::AddNode {
                id: "out2".to_string(),
                node_type: "console".to_string(),
                config: None,
            },
            GraphOp::Connect {
                from_node: "src".to_string(),
                from_port: None,
                to_node: "out1".to_string(),
                to_port: None,
            },
            GraphOp::Connect {
                from_node: "src".to_string(),
                from_port: None,
                to_node: "out2".to_string(),
                to_port: None,
            },
        ]).unwrap();

        assert_eq!(editor.graph().edges.len(), 2);

        editor
            .apply(GraphOp::InsertAfter {
                target: "src".to_string(),
                new_id: "mid".to_string(),
                node_type: "transform".to_string(),
            })
            .unwrap();

        // src → mid, mid → out1, mid → out2
        assert_eq!(editor.graph().nodes.len(), 4);
        assert_eq!(editor.graph().edges.len(), 3);

        let edges = &editor.graph().edges;
        assert!(edges.iter().any(|e| e.from_node == "src" && e.to_node == "mid"));
        assert!(edges.iter().any(|e| e.from_node == "mid" && e.to_node == "out1"));
        assert!(edges.iter().any(|e| e.from_node == "mid" && e.to_node == "out2"));
    }

    #[test]
    fn test_apply_all_batch() {
        let registry = test_registry();
        let mut graph = empty_graph();
        let mut editor = GraphEditor::new(&mut graph, &registry);

        editor.apply_all(vec![
            GraphOp::AddNode {
                id: "src".to_string(),
                node_type: "file_read".to_string(),
                config: Some(serde_json::json!({"path": "data.txt"})),
            },
            GraphOp::AddNode {
                id: "out".to_string(),
                node_type: "console".to_string(),
                config: None,
            },
            GraphOp::Connect {
                from_node: "src".to_string(),
                from_port: None,
                to_node: "out".to_string(),
                to_port: None,
            },
        ]).unwrap();

        assert_eq!(editor.graph().nodes.len(), 2);
        assert_eq!(editor.graph().edges.len(), 1);
    }

    #[test]
    fn test_graphop_serialization() {
        let op = GraphOp::AddNode {
            id: "db".to_string(),
            node_type: "database".to_string(),
            config: None,
        };
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains("\"op\":\"add_node\""));

        // Roundtrip
        let parsed: GraphOp = serde_json::from_str(&json).unwrap();
        if let GraphOp::AddNode { id, node_type, .. } = parsed {
            assert_eq!(id, "db");
            assert_eq!(node_type, "database");
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn test_self_loop_prevented() {
        let registry = test_registry();
        let mut graph = empty_graph();
        let mut editor = GraphEditor::new(&mut graph, &registry);

        editor
            .apply(GraphOp::AddNode {
                id: "a".to_string(),
                node_type: "transform".to_string(),
                config: None,
            })
            .unwrap();

        let result = editor.apply(GraphOp::Connect {
            from_node: "a".to_string(),
            from_port: None,
            to_node: "a".to_string(),
            to_port: None,
        });

        assert!(matches!(result, Err(EditError::SelfLoop(_))));
    }
}
