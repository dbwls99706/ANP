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

            node.init(&node_spec.config).await.map_err(|e| {
                EngineError::NodeInit {
                    node_id: node_spec.id.clone(),
                    message: e.to_string(),
                }
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
        let has_service = graph
            .nodes
            .iter()
            .any(|n| n.kind == NodeKind::Service);

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
