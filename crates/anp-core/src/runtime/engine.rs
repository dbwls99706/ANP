use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use tracing::{error, info};

use crate::graph::loader;
use crate::graph::spec::{AnpGraph, NodeKind};
use crate::graph::validate;
use crate::node::registry::NodeRegistry;
use crate::node::traits::NodeExecutor;
use crate::runtime::context::ExecutionContext;
use crate::runtime::executor;
use crate::runtime::scheduler;

/// Errors from the engine
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("failed to load graph: {0}")]
    Load(#[from] loader::LoadError),

    #[error("graph validation failed: {0:?}")]
    Validation(Vec<validate::ValidationError>),

    #[error("scheduling failed: {0}")]
    Schedule(#[from] scheduler::ScheduleError),

    #[error("execution failed: {0}")]
    Execution(#[from] executor::ExecutionError),

    #[error("unknown node type: {0}")]
    UnknownNodeType(String),

    #[error("node init failed for '{node_id}': {message}")]
    NodeInit { node_id: String, message: String },
}

/// The main ANP engine
pub struct AnpEngine {
    registry: NodeRegistry,
}

impl AnpEngine {
    pub fn new(registry: NodeRegistry) -> Self {
        Self { registry }
    }

    /// Load and validate a graph from a file
    pub fn load_graph(&self, path: &Path) -> Result<AnpGraph, EngineError> {
        let graph = loader::load_from_file(path)?;
        validate::validate(&graph).map_err(EngineError::Validation)?;
        Ok(graph)
    }

    /// Load and validate a graph from a JSON string
    pub fn load_graph_str(&self, json: &str) -> Result<AnpGraph, EngineError> {
        let graph = loader::load_from_str(json)?;
        validate::validate(&graph).map_err(EngineError::Validation)?;
        Ok(graph)
    }

    /// Run a graph — auto-detects batch vs service mode
    pub async fn run(&self, graph: &AnpGraph) -> Result<(), EngineError> {
        info!(name = %graph.metadata.name, "starting ANP engine");

        // Create and init node instances
        let mut nodes: HashMap<String, Box<dyn NodeExecutor>> = HashMap::new();
        for node_spec in &graph.nodes {
            let mut node = self
                .registry
                .create_node(&node_spec.node_type)
                .ok_or_else(|| EngineError::UnknownNodeType(node_spec.node_type.clone()))?;

            node.init(&node_spec.config)
                .await
                .map_err(|e| EngineError::NodeInit {
                    node_id: node_spec.id.clone(),
                    message: e.to_string(),
                })?;

            nodes.insert(node_spec.id.clone(), node);
        }

        // Create execution plan
        let plan = scheduler::schedule(graph)?;
        info!(
            stages = plan.stages.len(),
            entry_nodes = ?plan.entry_nodes,
            "execution plan created"
        );

        // Detect mode: if any Service node exists → service mode
        let has_service = graph.nodes.iter().any(|n| n.kind == NodeKind::Service);

        let ctx = ExecutionContext::new();

        if has_service {
            // Service mode: wrap in Arc for shared ownership
            let plan = Arc::new(plan);
            let nodes = Arc::new(nodes);
            let ctx = Arc::new(ctx);

            executor::execute_service(graph, plan, Arc::clone(&nodes), Arc::clone(&ctx)).await?;

            // Shutdown all nodes after Ctrl+C
            // We need to get nodes out of Arc — use Arc::try_unwrap
            match Arc::try_unwrap(nodes) {
                Ok(nodes) => {
                    self.shutdown_nodes(nodes).await;
                }
                Err(arc_nodes) => {
                    // Still referenced — shutdown via shared ref (can't call &mut shutdown)
                    warn_shutdown_shared(&arc_nodes);
                }
            }
        } else {
            // Batch mode
            executor::execute_batch(graph, &plan, &nodes, &ctx).await?;
            self.shutdown_nodes(nodes).await;
        }

        info!("ANP engine finished");
        Ok(())
    }

    /// Run a graph from a file path
    pub async fn run_file(&self, path: &Path) -> Result<(), EngineError> {
        let graph = self.load_graph(path)?;
        self.run(&graph).await
    }

    /// Access the registry
    pub fn registry(&self) -> &NodeRegistry {
        &self.registry
    }

    /// Shutdown all nodes in order
    async fn shutdown_nodes(&self, nodes: HashMap<String, Box<dyn NodeExecutor>>) {
        // Shutdown order: service → processor → sink → source
        let mut ordered: Vec<(String, Box<dyn NodeExecutor>)> = nodes.into_iter().collect();
        ordered.sort_by_key(|(_, node)| match node.node_kind() {
            NodeKind::Service => 0,
            NodeKind::Processor => 1,
            NodeKind::Sink => 2,
            NodeKind::Source => 3,
        });

        for (id, mut node) in ordered {
            if let Err(e) = node.shutdown().await {
                error!(node_id = %id, error = %e, "node shutdown failed");
            }
        }
    }
}

fn warn_shutdown_shared(nodes: &HashMap<String, Box<dyn NodeExecutor>>) {
    for id in nodes.keys() {
        error!(node_id = %id, "cannot shutdown node — still referenced");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::spec::*;
    use crate::node::registry::{NodeInfo, NodeSchema};
    use crate::node::traits::*;

    /// Mock node for testing engine workflows
    struct MockNode {
        kind: NodeKind,
        output: serde_json::Value,
        init_fail: bool,
    }

    impl MockNode {
        fn source(output: serde_json::Value) -> Self {
            Self {
                kind: NodeKind::Source,
                output,
                init_fail: false,
            }
        }
        fn processor(output: serde_json::Value) -> Self {
            Self {
                kind: NodeKind::Processor,
                output,
                init_fail: false,
            }
        }
        fn sink() -> Self {
            Self {
                kind: NodeKind::Sink,
                output: serde_json::Value::Null,
                init_fail: false,
            }
        }
        fn failing_init() -> Self {
            Self {
                kind: NodeKind::Processor,
                output: serde_json::Value::Null,
                init_fail: true,
            }
        }
    }

    #[async_trait::async_trait]
    impl NodeExecutor for MockNode {
        fn node_type(&self) -> &str {
            "mock"
        }
        fn node_kind(&self) -> NodeKind {
            self.kind
        }
        async fn init(&mut self, _config: &serde_json::Value) -> NodeResult<()> {
            if self.init_fail {
                Err(NodeError::InitFailed("mock init failure".into()))
            } else {
                Ok(())
            }
        }
        async fn execute(
            &self,
            _input: serde_json::Value,
            _ctx: &ExecutionContext,
        ) -> NodeResult<NodeOutput> {
            Ok(NodeOutput::data(self.output.clone()))
        }
    }

    fn mock_registry() -> NodeRegistry {
        let mut registry = NodeRegistry::new();
        registry.register(NodeInfo {
            factory: || Box::new(MockNode::source(serde_json::json!("hello"))),
            schema: NodeSchema {
                node_type: "mock_source".to_string(),
                kind: NodeKind::Source,
                description: "test source".to_string(),
                inputs: vec![],
                outputs: vec![PortSpec::new("default")],
                config_schema: serde_json::json!({}),
            },
        });
        registry.register(NodeInfo {
            factory: || Box::new(MockNode::processor(serde_json::json!("processed"))),
            schema: NodeSchema {
                node_type: "mock_processor".to_string(),
                kind: NodeKind::Processor,
                description: "test processor".to_string(),
                inputs: vec![PortSpec::new("default")],
                outputs: vec![PortSpec::new("default")],
                config_schema: serde_json::json!({}),
            },
        });
        registry.register(NodeInfo {
            factory: || Box::new(MockNode::sink()),
            schema: NodeSchema {
                node_type: "mock_sink".to_string(),
                kind: NodeKind::Sink,
                description: "test sink".to_string(),
                inputs: vec![PortSpec::new("default")],
                outputs: vec![],
                config_schema: serde_json::json!({}),
            },
        });
        registry.register(NodeInfo {
            factory: || Box::new(MockNode::failing_init()),
            schema: NodeSchema {
                node_type: "mock_fail_init".to_string(),
                kind: NodeKind::Processor,
                description: "always fails init".to_string(),
                inputs: vec![PortSpec::new("default")],
                outputs: vec![PortSpec::new("default")],
                config_schema: serde_json::json!({}),
            },
        });
        registry
    }

    #[test]
    fn test_load_graph_str_valid() {
        let engine = AnpEngine::new(mock_registry());
        let json = r#"{
            "version": "0.1",
            "metadata": { "name": "test", "description": "" },
            "nodes": [
                { "id": "s", "node_type": "mock_source", "kind": "source", "config": {} }
            ],
            "edges": []
        }"#;
        let graph = engine.load_graph_str(json).unwrap();
        assert_eq!(graph.metadata.name, "test");
        assert_eq!(graph.nodes.len(), 1);
    }

    #[test]
    fn test_load_graph_str_invalid_json() {
        let engine = AnpEngine::new(mock_registry());
        let result = engine.load_graph_str("not json");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), EngineError::Load(_)));
    }

    #[test]
    fn test_load_graph_str_validation_fails() {
        let engine = AnpEngine::new(mock_registry());
        // Edge references nonexistent node → validation error
        let json = r#"{
            "version": "0.1",
            "metadata": { "name": "test", "description": "" },
            "nodes": [
                { "id": "s", "node_type": "x", "kind": "source", "config": {},
                  "inputs": [], "outputs": [{"name": "default"}] }
            ],
            "edges": [
                { "id": "e1", "from_node": "s", "to_node": "nonexistent" }
            ]
        }"#;
        let result = engine.load_graph_str(json);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), EngineError::Validation(_)));
    }

    #[tokio::test]
    async fn test_run_batch_pipeline() {
        let engine = AnpEngine::new(mock_registry());
        let json = r#"{
            "version": "0.1",
            "metadata": { "name": "batch-test", "description": "" },
            "nodes": [
                { "id": "s", "node_type": "mock_source", "kind": "source", "config": {},
                  "inputs": [], "outputs": [{"name": "default"}] },
                { "id": "p", "node_type": "mock_processor", "kind": "processor", "config": {},
                  "inputs": [{"name": "default"}], "outputs": [{"name": "default"}] },
                { "id": "k", "node_type": "mock_sink", "kind": "sink", "config": {},
                  "inputs": [{"name": "default"}], "outputs": [] }
            ],
            "edges": [
                { "id": "e1", "from_node": "s", "to_node": "p" },
                { "id": "e2", "from_node": "p", "to_node": "k" }
            ]
        }"#;
        let graph = engine.load_graph_str(json).unwrap();
        let result = engine.run(&graph).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_run_unknown_node_type() {
        let engine = AnpEngine::new(mock_registry());
        let graph = AnpGraph {
            version: GraphVersion::V0_1,
            metadata: GraphMetadata {
                name: "test".into(),
                description: String::new(),
            },
            nodes: vec![Node {
                id: "s".into(),
                node_type: "nonexistent_type".into(),
                kind: NodeKind::Source,
                config: serde_json::Value::Null,
                inputs: vec![],
                outputs: vec![PortSpec::new("default")],
            }],
            edges: vec![],
        };
        let result = engine.run(&graph).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::UnknownNodeType(t) => assert_eq!(t, "nonexistent_type"),
            e => panic!("expected UnknownNodeType, got {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_run_node_init_failure() {
        let engine = AnpEngine::new(mock_registry());
        let graph = AnpGraph {
            version: GraphVersion::V0_1,
            metadata: GraphMetadata {
                name: "test".into(),
                description: String::new(),
            },
            nodes: vec![Node {
                id: "fail".into(),
                node_type: "mock_fail_init".into(),
                kind: NodeKind::Processor,
                config: serde_json::Value::Null,
                inputs: vec![PortSpec::new("default")],
                outputs: vec![PortSpec::new("default")],
            }],
            edges: vec![],
        };
        let result = engine.run(&graph).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::NodeInit { node_id, message } => {
                assert_eq!(node_id, "fail");
                assert!(message.contains("init failure"));
            }
            e => panic!("expected NodeInit, got {:?}", e),
        }
    }

    #[test]
    fn test_registry_accessor() {
        let registry = mock_registry();
        let engine = AnpEngine::new(registry);
        assert!(engine.registry().has("mock_source"));
        assert!(engine.registry().has("mock_sink"));
        assert!(!engine.registry().has("nonexistent"));
    }

    #[tokio::test]
    async fn test_shutdown_order() {
        // Verify that shutdown completes without error for all node types
        let engine = AnpEngine::new(mock_registry());
        let json = r#"{
            "version": "0.1",
            "metadata": { "name": "shutdown-test", "description": "" },
            "nodes": [
                { "id": "s", "node_type": "mock_source", "kind": "source", "config": {},
                  "inputs": [], "outputs": [{"name": "default"}] },
                { "id": "k", "node_type": "mock_sink", "kind": "sink", "config": {},
                  "inputs": [{"name": "default"}], "outputs": [] }
            ],
            "edges": [
                { "id": "e1", "from_node": "s", "to_node": "k" }
            ]
        }"#;
        let graph = engine.load_graph_str(json).unwrap();
        assert!(engine.run(&graph).await.is_ok());
    }
}
