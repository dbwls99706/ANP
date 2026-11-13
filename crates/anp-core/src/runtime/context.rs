use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;
use tokio::sync::RwLock;

/// Unique identifier for a pipeline execution run
pub type RunId = String;

/// Thread-safe execution context shared across all nodes
pub struct ExecutionContext {
    /// Per-run isolated data stores
    runs: DashMap<RunId, RunContext>,
    /// Global shared state
    state: Arc<RwLock<serde_json::Value>>,
}

/// Context for a single pipeline execution run
#[derive(Debug)]
pub struct RunContext {
    /// Node outputs: NodeId → Value
    pub data: HashMap<String, serde_json::Value>,
    /// Per-port node outputs: NodeId → PortName → Value.
    /// A node recorded here only feeds edges leaving one of these ports.
    pub ports: HashMap<String, HashMap<String, serde_json::Value>>,
    /// Run metadata for tracing
    pub metadata: RunMetadata,
    /// Per-node execution traces
    pub traces: Vec<NodeTrace>,
}

/// Metadata about a pipeline run
#[derive(Debug, Clone)]
pub struct RunMetadata {
    pub started_at: Instant,
    pub source_node: String,
}

/// Trace record for a single node execution
#[derive(Debug, Clone, serde::Serialize)]
pub struct NodeTrace {
    /// Node ID that was executed
    pub node_id: String,
    /// Node type (e.g., "llm_call", "filter")
    pub node_type: String,
    /// Execution duration in microseconds
    pub duration_us: u64,
    /// Whether execution succeeded
    pub success: bool,
    /// Error message if failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Output port names used
    pub output_ports: Vec<String>,
}

impl ExecutionContext {
    pub fn new() -> Self {
        Self {
            runs: DashMap::new(),
            state: Arc::new(RwLock::new(serde_json::Value::Object(
                serde_json::Map::new(),
            ))),
        }
    }

    /// Start a new run, returns the run_id
    pub fn start_run(&self, run_id: RunId, source_node: String) {
        self.runs.insert(
            run_id,
            RunContext {
                data: HashMap::new(),
                ports: HashMap::new(),
                metadata: RunMetadata {
                    started_at: Instant::now(),
                    source_node,
                },
                traces: Vec::new(),
            },
        );
    }

    /// Store a node's output for a specific run
    pub fn set_node_output(&self, run_id: &str, node_id: &str, output: serde_json::Value) {
        if let Some(mut run) = self.runs.get_mut(run_id) {
            run.data.insert(node_id.to_string(), output);
        }
    }

    /// Get a node's output from a specific run
    pub fn get_node_output(&self, run_id: &str, node_id: &str) -> Option<serde_json::Value> {
        self.runs
            .get(run_id)
            .and_then(|run| run.data.get(node_id).cloned())
    }

    /// Record which output ports a node emitted on in a specific run
    pub fn set_node_ports(
        &self,
        run_id: &str,
        node_id: &str,
        ports: HashMap<String, serde_json::Value>,
    ) {
        if let Some(mut run) = self.runs.get_mut(run_id) {
            run.ports.insert(node_id.to_string(), ports);
        }
    }

    /// Whether per-port outputs were recorded for a node in a specific run
    pub fn has_node_ports(&self, run_id: &str, node_id: &str) -> bool {
        self.runs
            .get(run_id)
            .is_some_and(|run| run.ports.contains_key(node_id))
    }

    /// Get the value a node emitted on one output port, if it emitted there
    pub fn get_port_output(
        &self,
        run_id: &str,
        node_id: &str,
        port: &str,
    ) -> Option<serde_json::Value> {
        self.runs
            .get(run_id)
            .and_then(|run| run.ports.get(node_id)?.get(port).cloned())
    }

    /// Finish and clean up a run (prevents memory leaks)
    pub fn finish_run(&self, run_id: &str) -> Option<RunContext> {
        self.runs.remove(run_id).map(|(_, ctx)| ctx)
    }

    /// Record a node execution trace for a run
    pub fn record_trace(&self, run_id: &str, trace: NodeTrace) {
        if let Some(mut run) = self.runs.get_mut(run_id) {
            run.traces.push(trace);
        }
    }

    /// Get all traces for a run
    pub fn get_traces(&self, run_id: &str) -> Vec<NodeTrace> {
        self.runs
            .get(run_id)
            .map(|run| run.traces.clone())
            .unwrap_or_default()
    }

    /// Access global shared state
    pub fn state(&self) -> &Arc<RwLock<serde_json::Value>> {
        &self.state
    }

    /// Number of active runs
    pub fn active_runs(&self) -> usize {
        self.runs.len()
    }
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_lifecycle() {
        let ctx = ExecutionContext::new();
        let run_id = "run-1".to_string();

        ctx.start_run(run_id.clone(), "http_server".to_string());
        assert_eq!(ctx.active_runs(), 1);

        ctx.set_node_output(&run_id, "transform", serde_json::json!({"result": 42}));
        let output = ctx.get_node_output(&run_id, "transform").unwrap();
        assert_eq!(output, serde_json::json!({"result": 42}));

        let finished = ctx.finish_run(&run_id).unwrap();
        assert_eq!(finished.metadata.source_node, "http_server");
        assert_eq!(ctx.active_runs(), 0);
    }
}
