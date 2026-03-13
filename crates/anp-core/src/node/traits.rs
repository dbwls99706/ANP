use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::graph::spec::NodeKind;
use crate::runtime::context::ExecutionContext;

/// Result type for node operations
pub type NodeResult<T> = Result<T, NodeError>;

/// Errors from node execution
#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    #[error("initialization failed: {0}")]
    InitFailed(String),

    #[error("execution failed: {0}")]
    ExecutionFailed(String),

    #[error("invalid config: {0}")]
    InvalidConfig(String),

    #[error("missing input: {0}")]
    MissingInput(String),

    #[error("{0}")]
    Other(String),
}

/// Core trait for all ANP nodes
#[async_trait::async_trait]
pub trait NodeExecutor: Send + Sync {
    /// The type identifier for this node (e.g., "console", "http_server")
    fn node_type(&self) -> &str;

    /// The kind of node (Source, Processor, Sink, Service)
    fn node_kind(&self) -> NodeKind;

    /// Initialize the node with its config
    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()>;

    /// Execute the node with explicit input, producing multi-port output
    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput>;

    /// Start a long-running service (Service nodes only).
    /// The service should spawn its event loop and call `trigger` for each event.
    /// Default implementation is a no-op, so non-Service nodes don't need to implement this.
    async fn start_service(
        &self,
        _node_id: &str,
        _trigger: Arc<dyn PipelineTrigger>,
    ) -> NodeResult<()> {
        Ok(())
    }

    /// Shutdown and clean up resources
    async fn shutdown(&mut self) -> NodeResult<()> {
        Ok(())
    }
}

/// Trait for Service nodes to trigger downstream pipeline execution.
/// The runtime provides an implementation; Service nodes call `trigger()` on each event.
#[async_trait::async_trait]
pub trait PipelineTrigger: Send + Sync {
    /// Execute the downstream pipeline from a source node's output port.
    /// Returns the final output value from the pipeline run.
    async fn trigger(
        &self,
        source_node: &str,
        port: &str,
        data: serde_json::Value,
    ) -> NodeResult<serde_json::Value>;
}

/// Output from a node execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeOutput {
    /// Data for each output port
    pub ports: HashMap<String, serde_json::Value>,
    /// Execution status
    pub status: NodeStatus,
    /// Debug logs from this execution
    #[serde(default)]
    pub logs: Vec<String>,
    /// Error details when status is Error
    #[serde(default)]
    pub error: Option<String>,
}

impl NodeOutput {
    /// Create a simple single-port output
    pub fn data(value: serde_json::Value) -> Self {
        let mut ports = HashMap::new();
        ports.insert("default".to_string(), value);
        Self {
            ports,
            status: NodeStatus::Ok,
            logs: Vec::new(),
            error: None,
        }
    }

    /// Create an output that signals to skip downstream nodes
    pub fn skip() -> Self {
        Self {
            ports: HashMap::new(),
            status: NodeStatus::Skip,
            logs: Vec::new(),
            error: None,
        }
    }

    /// Create an error output
    pub fn error(msg: impl Into<String>) -> Self {
        let msg = msg.into();
        Self {
            ports: HashMap::new(),
            status: NodeStatus::Error,
            logs: Vec::new(),
            error: Some(msg),
        }
    }

    /// Create a multi-port output
    pub fn multi(ports: HashMap<String, serde_json::Value>) -> Self {
        Self {
            ports,
            status: NodeStatus::Ok,
            logs: Vec::new(),
            error: None,
        }
    }

    /// Add a log message
    pub fn with_log(mut self, msg: impl Into<String>) -> Self {
        self.logs.push(msg.into());
        self
    }
}

/// Execution status of a node
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    /// Normal — proceed to downstream nodes
    Ok,
    /// Skip downstream nodes
    Skip,
    /// Route to error port
    Error,
}
