use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use tracing::{debug, error, info, warn};

use crate::graph::spec::{AnpGraph, NodeKind};
use crate::node::traits::{
    NodeError, NodeExecutor, NodeOutput, NodeResult, NodeStatus, PipelineTrigger,
};
use crate::runtime::context::{ExecutionContext, NodeTrace};
use crate::runtime::scheduler::ExecutionPlan;

/// Errors from execution
#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    #[error("node '{0}' not found in instances")]
    NodeNotFound(String),

    #[error("node '{node_id}' execution failed: {message}")]
    NodeFailed { node_id: String, message: String },

    #[error("service node '{0}' is not supported in batch mode")]
    ServiceInBatchMode(String),
}

/// Default pipeline timeout: 30 seconds per request
const DEFAULT_PIPELINE_TIMEOUT_SECS: u64 = 30;

/// Runtime implementation of PipelineTrigger.
/// Service nodes use this to execute downstream pipeline on each event.
pub struct PipelineTriggerImpl {
    pub plan: Arc<ExecutionPlan>,
    pub nodes: Arc<HashMap<String, Box<dyn NodeExecutor>>>,
    pub ctx: Arc<ExecutionContext>,
    pub pipeline_timeout_secs: u64,
}

#[async_trait::async_trait]
impl PipelineTrigger for PipelineTriggerImpl {
    async fn trigger(
        &self,
        source_node: &str,
        port: &str,
        data: serde_json::Value,
    ) -> NodeResult<serde_json::Value> {
        let timeout = std::time::Duration::from_secs(self.pipeline_timeout_secs);

        match tokio::time::timeout(timeout, self.execute_pipeline(source_node, port, data)).await {
            Ok(result) => result,
            Err(_) => {
                error!(
                    source = %source_node,
                    port = %port,
                    timeout_secs = self.pipeline_timeout_secs,
                    "pipeline timed out"
                );
                Err(NodeError::ExecutionFailed(format!(
                    "pipeline timed out after {}s",
                    self.pipeline_timeout_secs
                )))
            }
        }
    }
}

impl PipelineTriggerImpl {
    async fn execute_pipeline(
        &self,
        source_node: &str,
        port: &str,
        data: serde_json::Value,
    ) -> NodeResult<serde_json::Value> {
        let run_id = uuid::Uuid::new_v4().to_string();
        self.ctx.start_run(run_id.clone(), source_node.to_string());

        debug!(run_id = %run_id, source = %source_node, port = %port, "pipeline trigger fired");

        // Store the trigger data as the source node's output
        self.ctx.set_node_output(&run_id, source_node, data.clone());
        self.ctx.set_node_ports(
            &run_id,
            source_node,
            HashMap::from([(port.to_string(), data)]),
        );

        // Find downstream nodes reachable from source_node via the specified port
        let downstream_nodes = find_downstream_from_port(source_node, port, &self.plan);

        // Find the source node's stage index
        let source_stage = match self.plan.node_map.get(source_node).copied() {
            Some(stage) => stage,
            None => {
                warn!(node_id = %source_node, "node not found in node_map, defaulting to stage 0");
                0
            }
        };

        // Execute stages after the source node's stage
        let mut last_output = serde_json::Value::Null;
        for (stage_idx, stage) in self.plan.stages.iter().enumerate() {
            if stage_idx <= source_stage {
                continue;
            }

            // Only execute nodes that are downstream of the trigger
            let stage_nodes: Vec<&String> = stage
                .node_ids
                .iter()
                .filter(|id| downstream_nodes.contains(id.as_str()))
                .collect();

            if stage_nodes.is_empty() {
                continue;
            }

            // Execute independent nodes within a stage concurrently
            if stage_nodes.len() > 1 {
                let futures: Vec<_> = stage_nodes
                    .iter()
                    .map(|node_id| run_node(node_id, &self.plan, &self.nodes, &self.ctx, &run_id))
                    .collect();
                let results = futures::future::join_all(futures).await;
                for (i, result) in results.into_iter().enumerate() {
                    match result {
                        Ok(Some(val)) => {
                            last_output = val;
                        }
                        Ok(None) => {}
                        Err(e) => {
                            error!(node_id = %stage_nodes[i], error = %e, "node execution failed in trigger pipeline");
                            self.ctx.finish_run(&run_id);
                            return Err(e);
                        }
                    }
                }
            } else {
                for node_id in &stage_nodes {
                    match run_node(node_id, &self.plan, &self.nodes, &self.ctx, &run_id).await {
                        Ok(Some(val)) => {
                            last_output = val;
                        }
                        Ok(None) => {}
                        Err(e) => {
                            error!(node_id = %node_id, error = %e, "node execution failed in trigger pipeline");
                            self.ctx.finish_run(&run_id);
                            return Err(e);
                        }
                    }
                }
            }
        }

        // Cleanup run
        if let Some(run_ctx) = self.ctx.finish_run(&run_id) {
            let elapsed = run_ctx.metadata.started_at.elapsed();
            debug!(run_id = %run_id, elapsed_ms = elapsed.as_millis(), "trigger pipeline completed");
        }

        Ok(last_output)
    }
}

/// Find all nodes downstream from a specific node+port
fn find_downstream_from_port(
    source_node: &str,
    port: &str,
    plan: &ExecutionPlan,
) -> std::collections::HashSet<String> {
    let mut visited = std::collections::HashSet::new();
    let mut queue = std::collections::VecDeque::new();

    // Find direct targets from source_node:port
    if let Some(port_map) = plan.edge_map.get(source_node) {
        if let Some(edges) = port_map.get(port) {
            for edge in edges {
                if visited.insert(edge.to_node.clone()) {
                    queue.push_back(edge.to_node.clone());
                }
            }
        }
    }

    // BFS to find all transitive downstream nodes
    while let Some(node_id) = queue.pop_front() {
        if let Some(port_map) = plan.edge_map.get(&node_id) {
            for edges in port_map.values() {
                for edge in edges {
                    if visited.insert(edge.to_node.clone()) {
                        queue.push_back(edge.to_node.clone());
                    }
                }
            }
        }
    }

    visited
}

/// Execute a single node: gather input → execute → store output. Returns the stored output value.
/// Records a NodeTrace with timing and status for observability.
async fn execute_single_node(
    node_id: &str,
    plan: &ExecutionPlan,
    nodes: &HashMap<String, Box<dyn NodeExecutor>>,
    ctx: &ExecutionContext,
    run_id: &str,
) -> NodeResult<serde_json::Value> {
    run_node(node_id, plan, nodes, ctx, run_id)
        .await
        .map(Option::unwrap_or_default)
}

/// Like [`execute_single_node`], but returns `None` when the node produced
/// nothing: it sits on a branch that was not taken, or it returned Skip.
async fn run_node(
    node_id: &str,
    plan: &ExecutionPlan,
    nodes: &HashMap<String, Box<dyn NodeExecutor>>,
    ctx: &ExecutionContext,
    run_id: &str,
) -> NodeResult<Option<serde_json::Value>> {
    let node = nodes
        .get(node_id)
        .ok_or_else(|| NodeError::Other(format!("node '{}' not found", node_id)))?;

    // Service nodes are never executed directly
    if node.node_kind() == NodeKind::Service {
        return Ok(None);
    }

    let Some(input) = gather_input(node_id, plan, ctx, run_id) else {
        debug!(node_id = %node_id, "skipped: no upstream branch delivered data");
        return Ok(None);
    };
    let node_type = node.node_type().to_string();

    debug!(
        node_id = %node_id,
        node_type = %node_type,
        "executing node"
    );

    let start = Instant::now();

    match node.execute(input, ctx).await {
        Ok(output) => {
            let duration_us = start.elapsed().as_micros() as u64;

            for log in &output.logs {
                debug!(node_id = %node_id, "{}", log);
            }

            let output_ports: Vec<String> = output.ports.keys().cloned().collect();

            match output.status {
                NodeStatus::Ok => {
                    ctx.record_trace(
                        run_id,
                        NodeTrace {
                            node_id: node_id.to_string(),
                            node_type,
                            duration_us,
                            success: true,
                            error: None,
                            output_ports,
                        },
                    );
                    let stored = store_output(node_id, &output, plan, ctx, run_id);
                    Ok(Some(stored))
                }
                NodeStatus::Skip => {
                    ctx.record_trace(
                        run_id,
                        NodeTrace {
                            node_id: node_id.to_string(),
                            node_type,
                            duration_us,
                            success: true,
                            error: None,
                            output_ports: vec!["skip".to_string()],
                        },
                    );
                    info!(node_id = %node_id, "node returned Skip");
                    Ok(None)
                }
                NodeStatus::Error => {
                    let err_msg = output.error.clone();
                    ctx.record_trace(
                        run_id,
                        NodeTrace {
                            node_id: node_id.to_string(),
                            node_type,
                            duration_us,
                            success: false,
                            error: err_msg.clone(),
                            output_ports,
                        },
                    );
                    error!(
                        node_id = %node_id,
                        error = ?err_msg,
                        "node returned Error status"
                    );
                    store_error_output(node_id, &output, plan, ctx, run_id);
                    Ok(Some(serde_json::json!({
                        "error": err_msg,
                        "node_id": node_id
                    })))
                }
            }
        }
        Err(e) => {
            let duration_us = start.elapsed().as_micros() as u64;
            ctx.record_trace(
                run_id,
                NodeTrace {
                    node_id: node_id.to_string(),
                    node_type,
                    duration_us,
                    success: false,
                    error: Some(e.to_string()),
                    output_ports: vec![],
                },
            );
            error!(node_id = %node_id, error = %e, "node execution failed");
            Err(e)
        }
    }
}

/// Execute a batch pipeline (Source → Processor → Sink)
pub async fn execute_batch(
    _graph: &AnpGraph,
    plan: &ExecutionPlan,
    nodes: &HashMap<String, Box<dyn NodeExecutor>>,
    ctx: &ExecutionContext,
) -> Result<(), ExecutionError> {
    let run_id = uuid::Uuid::new_v4().to_string();

    let source_node = match plan.entry_nodes.first() {
        Some(node) => node.clone(),
        None => {
            return Err(ExecutionError::NodeFailed {
                node_id: "runtime".to_string(),
                message: "no entry nodes found in execution plan".to_string(),
            });
        }
    };

    ctx.start_run(run_id.clone(), source_node);

    info!(run_id = %run_id, "starting batch execution");

    for (stage_idx, stage) in plan.stages.iter().enumerate() {
        debug!(stage = stage_idx, nodes = ?stage.node_ids, "executing stage");

        // Filter out service nodes
        let executable_nodes: Vec<&String> = stage
            .node_ids
            .iter()
            .filter(|node_id| {
                nodes
                    .get(node_id.as_str())
                    .map(|n| n.node_kind() != NodeKind::Service)
                    .unwrap_or(true)
            })
            .collect();

        // Execute independent nodes within a stage concurrently
        if executable_nodes.len() > 1 {
            let futures: Vec<_> = executable_nodes
                .iter()
                .map(|node_id| execute_single_node(node_id, plan, nodes, ctx, &run_id))
                .collect();
            let results = futures::future::join_all(futures).await;
            for (i, result) in results.into_iter().enumerate() {
                match result {
                    Ok(_val) => {}
                    Err(e) => {
                        error!(node_id = %executable_nodes[i], error = %e, "node execution failed");
                        return Err(ExecutionError::NodeFailed {
                            node_id: executable_nodes[i].clone(),
                            message: e.to_string(),
                        });
                    }
                }
            }
        } else {
            for node_id in &executable_nodes {
                match execute_single_node(node_id, plan, nodes, ctx, &run_id).await {
                    Ok(_val) => {}
                    Err(e) => {
                        error!(node_id = %node_id, error = %e, "node execution failed");
                        return Err(ExecutionError::NodeFailed {
                            node_id: node_id.to_string(),
                            message: e.to_string(),
                        });
                    }
                }
            }
        }
    }

    if let Some(run_ctx) = ctx.finish_run(&run_id) {
        let elapsed = run_ctx.metadata.started_at.elapsed();
        let trace_count = run_ctx.traces.len();
        let failed_count = run_ctx.traces.iter().filter(|t| !t.success).count();
        info!(
            run_id = %run_id,
            elapsed_ms = elapsed.as_millis(),
            nodes_executed = trace_count,
            nodes_failed = failed_count,
            "batch execution completed"
        );
        for trace in &run_ctx.traces {
            debug!(
                node_id = %trace.node_id,
                node_type = %trace.node_type,
                duration_us = trace.duration_us,
                success = trace.success,
                "node trace"
            );
        }
    }

    Ok(())
}

/// Execute service mode: start Service nodes, then wait for shutdown signal
pub async fn execute_service(
    _graph: &AnpGraph,
    plan: Arc<ExecutionPlan>,
    nodes: Arc<HashMap<String, Box<dyn NodeExecutor>>>,
    ctx: Arc<ExecutionContext>,
) -> Result<(), ExecutionError> {
    info!("starting service mode execution");

    // First, execute any Source nodes (batch-style)
    for node_id in &plan.entry_nodes {
        if let Some(node) = nodes.get(node_id) {
            if node.node_kind() == NodeKind::Source {
                let run_id = uuid::Uuid::new_v4().to_string();
                ctx.start_run(run_id.clone(), node_id.clone());

                let input = gather_input(node_id, &plan, &ctx, &run_id).unwrap_or_default();
                match node.execute(input, &ctx).await {
                    Ok(output) => {
                        store_output(node_id, &output, &plan, &ctx, &run_id);
                    }
                    Err(e) => {
                        error!(node_id = %node_id, error = %e, "source node execution failed");
                        return Err(ExecutionError::NodeFailed {
                            node_id: node_id.clone(),
                            message: e.to_string(),
                        });
                    }
                }

                ctx.finish_run(&run_id);
            }
        }
    }

    // Create PipelineTrigger for Service nodes
    let trigger: Arc<dyn PipelineTrigger> = Arc::new(PipelineTriggerImpl {
        plan: Arc::clone(&plan),
        nodes: Arc::clone(&nodes),
        ctx: Arc::clone(&ctx),
        pipeline_timeout_secs: DEFAULT_PIPELINE_TIMEOUT_SECS,
    });

    // Start all Service nodes
    for node_id in &plan.entry_nodes {
        if let Some(node) = nodes.get(node_id) {
            if node.node_kind() == NodeKind::Service {
                info!(node_id = %node_id, "starting service node");
                node.start_service(node_id, Arc::clone(&trigger))
                    .await
                    .map_err(|e| ExecutionError::NodeFailed {
                        node_id: node_id.clone(),
                        message: e.to_string(),
                    })?;
            }
        }
    }

    info!("all service nodes started, waiting for shutdown signal...");

    // Wait for Ctrl+C (SIGINT) or SIGTERM for container environments
    let shutdown = async {
        let ctrl_c = tokio::signal::ctrl_c();

        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            match signal(SignalKind::terminate()) {
                Ok(mut sigterm) => {
                    tokio::select! {
                        _ = ctrl_c => info!("received SIGINT (Ctrl+C)"),
                        _ = sigterm.recv() => info!("received SIGTERM"),
                    }
                }
                Err(e) => {
                    warn!(error = %e, "failed to register SIGTERM handler, falling back to SIGINT only");
                    ctrl_c.await.ok();
                    info!("received SIGINT (Ctrl+C)");
                }
            }
        }

        #[cfg(not(unix))]
        {
            ctrl_c.await.ok();
            info!("received Ctrl+C");
        }
    };

    shutdown.await;

    info!("shutdown signal received, draining connections...");

    // Give in-flight requests a grace period to complete (5 seconds)
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Shutdown all service nodes
    // Note: nodes are behind Arc<HashMap> so we can't call &mut shutdown().
    // The tokio tasks spawned by service nodes will be dropped when the process exits.

    info!("shutdown complete");

    Ok(())
}

/// Gather input data for a node from its upstream edges.
/// Uses reverse_edge_map for O(1) lookup instead of scanning all edges.
///
/// An edge only carries data when its upstream node emitted on the edge's
/// `from_port`. Returns `None` when the node has upstream edges but none of
/// them carried data in this run — the node is on a branch that was not taken.
fn gather_input(
    node_id: &str,
    plan: &ExecutionPlan,
    ctx: &ExecutionContext,
    run_id: &str,
) -> Option<serde_json::Value> {
    let sources = match plan.reverse_edge_map.get(node_id) {
        Some(s) => s,
        None => return Some(serde_json::Value::Null),
    };

    let mut inputs: Vec<serde_json::Value> = Vec::with_capacity(sources.len());
    for from_id in sources {
        if !ctx.has_node_ports(run_id, from_id) {
            // No per-port record: the node's whole output feeds every edge.
            if let Some(output) = ctx.get_node_output(run_id, from_id) {
                inputs.push(output);
            }
            continue;
        }

        let mut ports: Vec<&str> = plan
            .edge_map
            .get(from_id)
            .into_iter()
            .flatten()
            .filter(|(_, edges)| edges.iter().any(|e| e.to_node == node_id))
            .map(|(port, _)| port.as_str())
            .collect();
        ports.sort_unstable();
        for port in ports {
            if let Some(output) = ctx.get_port_output(run_id, from_id, port) {
                inputs.push(output);
            }
        }
    }

    match inputs.len() {
        0 => None,
        1 => inputs.pop(),
        _ => Some(serde_json::Value::Array(inputs)),
    }
}

/// Store node output into the context. Returns the stored value.
fn store_output(
    node_id: &str,
    output: &NodeOutput,
    plan: &ExecutionPlan,
    ctx: &ExecutionContext,
    run_id: &str,
) -> serde_json::Value {
    let val = if let Some(default_val) = output.ports.get("default") {
        default_val.clone()
    } else if output.ports.len() == 1 {
        output
            .ports
            .values()
            .next()
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    } else if !output.ports.is_empty() {
        let map: serde_json::Map<String, serde_json::Value> = output
            .ports
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        serde_json::Value::Object(map)
    } else {
        serde_json::Value::Null
    };

    ctx.set_node_output(run_id, node_id, val.clone());

    // Record per-port outputs so only edges leaving an emitted port carry data.
    // A graph that wires none of the emitted ports treats the node as
    // single-output, so its output falls through to the "default" port.
    let mut ports = output.ports.clone();
    if !ports.is_empty() && !ports.contains_key("default") {
        let wired = plan.edge_map.get(node_id);
        let routed = ports
            .keys()
            .any(|port| wired.is_some_and(|m| m.contains_key(port)));
        if !routed {
            ports.insert("default".to_string(), val.clone());
        }
    }
    ctx.set_node_ports(run_id, node_id, ports);

    val
}

/// Store error output — routes to "error" port edges if they exist, otherwise logs
fn store_error_output(
    node_id: &str,
    output: &NodeOutput,
    plan: &ExecutionPlan,
    ctx: &ExecutionContext,
    run_id: &str,
) {
    let error_val = serde_json::json!({
        "error": output.error,
        "node_id": node_id,
        "ports": output.ports,
    });

    // Check if there's an "error" port in the edge_map for this node
    if let Some(port_map) = plan.edge_map.get(node_id) {
        if port_map.contains_key("error") {
            ctx.set_node_output(run_id, node_id, error_val.clone());
            ctx.set_node_ports(
                run_id,
                node_id,
                HashMap::from([("error".to_string(), error_val)]),
            );
            debug!(node_id = %node_id, "routed error to error port");
            return;
        }
    }

    ctx.set_node_output(run_id, node_id, error_val);
    warn!(node_id = %node_id, error = ?output.error, "no error port found, error logged");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::spec::*;
    use crate::node::traits::*;
    use crate::runtime::context::ExecutionContext;
    use crate::runtime::scheduler;

    /// Mock node that returns configurable output
    struct MockNode {
        kind: NodeKind,
        output: serde_json::Value,
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
            Ok(())
        }
        async fn execute(
            &self,
            input: serde_json::Value,
            _ctx: &ExecutionContext,
        ) -> NodeResult<NodeOutput> {
            if self.output.is_null() {
                // Echo node: passes input through
                Ok(NodeOutput::data(input))
            } else {
                Ok(NodeOutput::data(self.output.clone()))
            }
        }
    }

    /// Mock node that always fails
    struct FailNode;

    #[async_trait::async_trait]
    impl NodeExecutor for FailNode {
        fn node_type(&self) -> &str {
            "fail"
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
            Err(NodeError::ExecutionFailed("mock failure".into()))
        }
    }

    /// Mock node that returns multi-port output
    struct MultiPortNode;

    #[async_trait::async_trait]
    impl NodeExecutor for MultiPortNode {
        fn node_type(&self) -> &str {
            "multi"
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
            let mut ports = HashMap::new();
            ports.insert("port_a".to_string(), serde_json::json!("data_a"));
            ports.insert("port_b".to_string(), serde_json::json!("data_b"));
            Ok(NodeOutput::multi(ports))
        }
    }

    /// Mock node that returns skip status
    struct SkipNode;

    #[async_trait::async_trait]
    impl NodeExecutor for SkipNode {
        fn node_type(&self) -> &str {
            "skip"
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
            Ok(NodeOutput::skip())
        }
    }

    /// Mock node that returns error status (not Err)
    struct ErrorStatusNode;

    #[async_trait::async_trait]
    impl NodeExecutor for ErrorStatusNode {
        fn node_type(&self) -> &str {
            "error_status"
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
            Ok(NodeOutput::error("soft error"))
        }
    }

    fn make_graph(nodes: Vec<Node>, edges: Vec<Edge>) -> AnpGraph {
        AnpGraph {
            version: GraphVersion::V0_1,
            metadata: GraphMetadata {
                name: "test".into(),
                description: String::new(),
            },
            nodes,
            edges,
        }
    }

    // -- gather_input tests --

    #[test]
    fn test_gather_input_no_upstream() {
        let graph = make_graph(
            vec![Node {
                id: "s".into(),
                node_type: "mock".into(),
                kind: NodeKind::Source,
                config: serde_json::Value::Null,
                inputs: vec![],
                outputs: vec![PortSpec::new("default")],
            }],
            vec![],
        );
        let plan = scheduler::schedule(&graph).unwrap();
        let ctx = ExecutionContext::new();
        let run_id = "test-run";
        ctx.start_run(run_id.into(), "s".into());

        let input = gather_input("s", &plan, &ctx, run_id);
        assert_eq!(input, Some(serde_json::Value::Null));
    }

    #[test]
    fn test_gather_input_single_upstream() {
        let graph = make_graph(
            vec![
                Node {
                    id: "s".into(),
                    node_type: "mock".into(),
                    kind: NodeKind::Source,
                    config: serde_json::Value::Null,
                    inputs: vec![],
                    outputs: vec![PortSpec::new("default")],
                },
                Node {
                    id: "p".into(),
                    node_type: "mock".into(),
                    kind: NodeKind::Processor,
                    config: serde_json::Value::Null,
                    inputs: vec![PortSpec::new("default")],
                    outputs: vec![PortSpec::new("default")],
                },
            ],
            vec![Edge {
                id: "e1".into(),
                from_node: "s".into(),
                from_port: "default".into(),
                to_node: "p".into(),
                to_port: "default".into(),
            }],
        );
        let plan = scheduler::schedule(&graph).unwrap();
        let ctx = ExecutionContext::new();
        let run_id = "test-run";
        ctx.start_run(run_id.into(), "s".into());
        ctx.set_node_output(run_id, "s", serde_json::json!({"value": 42}));

        let input = gather_input("p", &plan, &ctx, run_id);
        assert_eq!(input, Some(serde_json::json!({"value": 42})));
    }

    #[test]
    fn test_gather_input_multiple_upstream() {
        let graph = make_graph(
            vec![
                Node {
                    id: "s1".into(),
                    node_type: "mock".into(),
                    kind: NodeKind::Source,
                    config: serde_json::Value::Null,
                    inputs: vec![],
                    outputs: vec![PortSpec::new("default")],
                },
                Node {
                    id: "s2".into(),
                    node_type: "mock".into(),
                    kind: NodeKind::Source,
                    config: serde_json::Value::Null,
                    inputs: vec![],
                    outputs: vec![PortSpec::new("default")],
                },
                Node {
                    id: "p".into(),
                    node_type: "mock".into(),
                    kind: NodeKind::Processor,
                    config: serde_json::Value::Null,
                    inputs: vec![PortSpec::new("default")],
                    outputs: vec![PortSpec::new("default")],
                },
            ],
            vec![
                Edge {
                    id: "e1".into(),
                    from_node: "s1".into(),
                    from_port: "default".into(),
                    to_node: "p".into(),
                    to_port: "default".into(),
                },
                Edge {
                    id: "e2".into(),
                    from_node: "s2".into(),
                    from_port: "default".into(),
                    to_node: "p".into(),
                    to_port: "default".into(),
                },
            ],
        );
        let plan = scheduler::schedule(&graph).unwrap();
        let ctx = ExecutionContext::new();
        let run_id = "test-run";
        ctx.start_run(run_id.into(), "s1".into());
        ctx.set_node_output(run_id, "s1", serde_json::json!("a"));
        ctx.set_node_output(run_id, "s2", serde_json::json!("b"));

        let input = gather_input("p", &plan, &ctx, run_id).unwrap();
        // Multiple upstream → Array
        assert!(input.is_array());
        let arr = input.as_array().unwrap();
        assert_eq!(arr.len(), 2);
    }

    // -- store_output tests --

    #[test]
    fn test_store_output_default_port() {
        let graph = make_graph(
            vec![Node {
                id: "s".into(),
                node_type: "mock".into(),
                kind: NodeKind::Source,
                config: serde_json::Value::Null,
                inputs: vec![],
                outputs: vec![PortSpec::new("default")],
            }],
            vec![],
        );
        let plan = scheduler::schedule(&graph).unwrap();
        let ctx = ExecutionContext::new();
        let run_id = "test-run";
        ctx.start_run(run_id.into(), "s".into());

        let output = NodeOutput::data(serde_json::json!({"result": "ok"}));
        let stored = store_output("s", &output, &plan, &ctx, run_id);
        assert_eq!(stored, serde_json::json!({"result": "ok"}));

        // Verify it's in the context
        let from_ctx = ctx.get_node_output(run_id, "s").unwrap();
        assert_eq!(from_ctx, serde_json::json!({"result": "ok"}));
    }

    #[test]
    fn test_store_output_multi_port() {
        let graph = make_graph(
            vec![Node {
                id: "p".into(),
                node_type: "mock".into(),
                kind: NodeKind::Processor,
                config: serde_json::Value::Null,
                inputs: vec![PortSpec::new("default")],
                outputs: vec![PortSpec::new("a"), PortSpec::new("b")],
            }],
            vec![],
        );
        let plan = scheduler::schedule(&graph).unwrap();
        let ctx = ExecutionContext::new();
        let run_id = "test-run";
        ctx.start_run(run_id.into(), "p".into());

        let mut ports = HashMap::new();
        ports.insert("a".to_string(), serde_json::json!("val_a"));
        ports.insert("b".to_string(), serde_json::json!("val_b"));
        let output = NodeOutput::multi(ports);
        let stored = store_output("p", &output, &plan, &ctx, run_id);
        // Multi-port without "default" → Object with both ports
        assert!(stored.is_object());
        let obj = stored.as_object().unwrap();
        assert_eq!(obj.get("a").unwrap(), &serde_json::json!("val_a"));
        assert_eq!(obj.get("b").unwrap(), &serde_json::json!("val_b"));
    }

    #[test]
    fn test_store_output_empty_ports() {
        let graph = make_graph(
            vec![Node {
                id: "p".into(),
                node_type: "mock".into(),
                kind: NodeKind::Processor,
                config: serde_json::Value::Null,
                inputs: vec![PortSpec::new("default")],
                outputs: vec![],
            }],
            vec![],
        );
        let plan = scheduler::schedule(&graph).unwrap();
        let ctx = ExecutionContext::new();
        let run_id = "test-run";
        ctx.start_run(run_id.into(), "p".into());

        let output = NodeOutput {
            ports: HashMap::new(),
            status: NodeStatus::Ok,
            logs: vec![],
            error: None,
        };
        let stored = store_output("p", &output, &plan, &ctx, run_id);
        assert_eq!(stored, serde_json::Value::Null);
    }

    // -- find_downstream_from_port tests --

    #[test]
    fn test_find_downstream_linear() {
        let graph = make_graph(
            vec![
                Node {
                    id: "a".into(),
                    node_type: "mock".into(),
                    kind: NodeKind::Source,
                    config: serde_json::Value::Null,
                    inputs: vec![],
                    outputs: vec![PortSpec::new("default")],
                },
                Node {
                    id: "b".into(),
                    node_type: "mock".into(),
                    kind: NodeKind::Processor,
                    config: serde_json::Value::Null,
                    inputs: vec![PortSpec::new("default")],
                    outputs: vec![PortSpec::new("default")],
                },
                Node {
                    id: "c".into(),
                    node_type: "mock".into(),
                    kind: NodeKind::Sink,
                    config: serde_json::Value::Null,
                    inputs: vec![PortSpec::new("default")],
                    outputs: vec![],
                },
            ],
            vec![
                Edge {
                    id: "e1".into(),
                    from_node: "a".into(),
                    from_port: "default".into(),
                    to_node: "b".into(),
                    to_port: "default".into(),
                },
                Edge {
                    id: "e2".into(),
                    from_node: "b".into(),
                    from_port: "default".into(),
                    to_node: "c".into(),
                    to_port: "default".into(),
                },
            ],
        );
        let plan = scheduler::schedule(&graph).unwrap();
        let downstream = find_downstream_from_port("a", "default", &plan);
        assert!(downstream.contains("b"));
        assert!(downstream.contains("c"));
        assert_eq!(downstream.len(), 2);
    }

    #[test]
    fn test_find_downstream_branching() {
        let graph = make_graph(
            vec![
                Node {
                    id: "a".into(),
                    node_type: "mock".into(),
                    kind: NodeKind::Source,
                    config: serde_json::Value::Null,
                    inputs: vec![],
                    outputs: vec![PortSpec::new("left"), PortSpec::new("right")],
                },
                Node {
                    id: "b".into(),
                    node_type: "mock".into(),
                    kind: NodeKind::Processor,
                    config: serde_json::Value::Null,
                    inputs: vec![PortSpec::new("default")],
                    outputs: vec![],
                },
                Node {
                    id: "c".into(),
                    node_type: "mock".into(),
                    kind: NodeKind::Processor,
                    config: serde_json::Value::Null,
                    inputs: vec![PortSpec::new("default")],
                    outputs: vec![],
                },
            ],
            vec![
                Edge {
                    id: "e1".into(),
                    from_node: "a".into(),
                    from_port: "left".into(),
                    to_node: "b".into(),
                    to_port: "default".into(),
                },
                Edge {
                    id: "e2".into(),
                    from_node: "a".into(),
                    from_port: "right".into(),
                    to_node: "c".into(),
                    to_port: "default".into(),
                },
            ],
        );
        let plan = scheduler::schedule(&graph).unwrap();

        // Only "left" port → only b
        let left = find_downstream_from_port("a", "left", &plan);
        assert!(left.contains("b"));
        assert!(!left.contains("c"));

        // Only "right" port → only c
        let right = find_downstream_from_port("a", "right", &plan);
        assert!(right.contains("c"));
        assert!(!right.contains("b"));
    }

    #[test]
    fn test_find_downstream_nonexistent_port() {
        let graph = make_graph(
            vec![Node {
                id: "a".into(),
                node_type: "mock".into(),
                kind: NodeKind::Source,
                config: serde_json::Value::Null,
                inputs: vec![],
                outputs: vec![PortSpec::new("default")],
            }],
            vec![],
        );
        let plan = scheduler::schedule(&graph).unwrap();
        let downstream = find_downstream_from_port("a", "nonexistent", &plan);
        assert!(downstream.is_empty());
    }

    // -- execute_single_node tests --

    #[tokio::test]
    async fn test_execute_single_node_ok() {
        let graph = make_graph(
            vec![Node {
                id: "s".into(),
                node_type: "mock".into(),
                kind: NodeKind::Source,
                config: serde_json::Value::Null,
                inputs: vec![],
                outputs: vec![PortSpec::new("default")],
            }],
            vec![],
        );
        let plan = scheduler::schedule(&graph).unwrap();
        let mut nodes: HashMap<String, Box<dyn NodeExecutor>> = HashMap::new();
        nodes.insert(
            "s".into(),
            Box::new(MockNode::source(serde_json::json!("test_output"))),
        );
        let ctx = ExecutionContext::new();
        let run_id = "run-1";
        ctx.start_run(run_id.into(), "s".into());

        let result = execute_single_node("s", &plan, &nodes, &ctx, run_id).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), serde_json::json!("test_output"));

        // Verify trace recorded
        let traces = ctx.get_traces(run_id);
        assert_eq!(traces.len(), 1);
        assert!(traces[0].success);
        assert_eq!(traces[0].node_id, "s");
    }

    #[tokio::test]
    async fn test_execute_single_node_failure() {
        let graph = make_graph(
            vec![Node {
                id: "f".into(),
                node_type: "fail".into(),
                kind: NodeKind::Processor,
                config: serde_json::Value::Null,
                inputs: vec![PortSpec::new("default")],
                outputs: vec![PortSpec::new("default")],
            }],
            vec![],
        );
        let plan = scheduler::schedule(&graph).unwrap();
        let mut nodes: HashMap<String, Box<dyn NodeExecutor>> = HashMap::new();
        nodes.insert("f".into(), Box::new(FailNode));
        let ctx = ExecutionContext::new();
        let run_id = "run-1";
        ctx.start_run(run_id.into(), "f".into());

        let result = execute_single_node("f", &plan, &nodes, &ctx, run_id).await;
        assert!(result.is_err());

        // Verify trace recorded with failure
        let traces = ctx.get_traces(run_id);
        assert_eq!(traces.len(), 1);
        assert!(!traces[0].success);
        assert!(traces[0].error.is_some());
    }

    #[tokio::test]
    async fn test_execute_single_node_skip() {
        let graph = make_graph(
            vec![Node {
                id: "s".into(),
                node_type: "skip".into(),
                kind: NodeKind::Processor,
                config: serde_json::Value::Null,
                inputs: vec![PortSpec::new("default")],
                outputs: vec![PortSpec::new("default")],
            }],
            vec![],
        );
        let plan = scheduler::schedule(&graph).unwrap();
        let mut nodes: HashMap<String, Box<dyn NodeExecutor>> = HashMap::new();
        nodes.insert("s".into(), Box::new(SkipNode));
        let ctx = ExecutionContext::new();
        let run_id = "run-1";
        ctx.start_run(run_id.into(), "s".into());

        let result = execute_single_node("s", &plan, &nodes, &ctx, run_id).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), serde_json::Value::Null);
    }

    #[tokio::test]
    async fn test_execute_single_node_error_status() {
        let graph = make_graph(
            vec![Node {
                id: "e".into(),
                node_type: "error_status".into(),
                kind: NodeKind::Processor,
                config: serde_json::Value::Null,
                inputs: vec![PortSpec::new("default")],
                outputs: vec![PortSpec::new("default")],
            }],
            vec![],
        );
        let plan = scheduler::schedule(&graph).unwrap();
        let mut nodes: HashMap<String, Box<dyn NodeExecutor>> = HashMap::new();
        nodes.insert("e".into(), Box::new(ErrorStatusNode));
        let ctx = ExecutionContext::new();
        let run_id = "run-1";
        ctx.start_run(run_id.into(), "e".into());

        // Error status returns Ok with error payload (soft error)
        let result = execute_single_node("e", &plan, &nodes, &ctx, run_id).await;
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["error"], "soft error");
    }

    #[tokio::test]
    async fn test_execute_single_node_multi_port() {
        let graph = make_graph(
            vec![Node {
                id: "m".into(),
                node_type: "multi".into(),
                kind: NodeKind::Processor,
                config: serde_json::Value::Null,
                inputs: vec![PortSpec::new("default")],
                outputs: vec![PortSpec::new("port_a"), PortSpec::new("port_b")],
            }],
            vec![],
        );
        let plan = scheduler::schedule(&graph).unwrap();
        let mut nodes: HashMap<String, Box<dyn NodeExecutor>> = HashMap::new();
        nodes.insert("m".into(), Box::new(MultiPortNode));
        let ctx = ExecutionContext::new();
        let run_id = "run-1";
        ctx.start_run(run_id.into(), "m".into());

        let result = execute_single_node("m", &plan, &nodes, &ctx, run_id).await;
        assert!(result.is_ok());
        let val = result.unwrap();
        // Multi-port output stored as object
        assert!(val.is_object());
    }

    #[tokio::test]
    async fn test_execute_single_node_not_found() {
        let graph = make_graph(
            vec![Node {
                id: "s".into(),
                node_type: "mock".into(),
                kind: NodeKind::Source,
                config: serde_json::Value::Null,
                inputs: vec![],
                outputs: vec![PortSpec::new("default")],
            }],
            vec![],
        );
        let plan = scheduler::schedule(&graph).unwrap();
        let nodes: HashMap<String, Box<dyn NodeExecutor>> = HashMap::new(); // empty!
        let ctx = ExecutionContext::new();
        let run_id = "run-1";
        ctx.start_run(run_id.into(), "s".into());

        let result = execute_single_node("s", &plan, &nodes, &ctx, run_id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_single_node_service_skipped() {
        let graph = make_graph(
            vec![Node {
                id: "srv".into(),
                node_type: "mock".into(),
                kind: NodeKind::Service,
                config: serde_json::Value::Null,
                inputs: vec![],
                outputs: vec![PortSpec::new("default")],
            }],
            vec![],
        );
        let plan = scheduler::schedule(&graph).unwrap();
        let mut nodes: HashMap<String, Box<dyn NodeExecutor>> = HashMap::new();
        nodes.insert(
            "srv".into(),
            Box::new(MockNode {
                kind: NodeKind::Service,
                output: serde_json::json!("should not execute"),
            }),
        );
        let ctx = ExecutionContext::new();
        let run_id = "run-1";
        ctx.start_run(run_id.into(), "srv".into());

        // Service nodes return Null without executing
        let result = execute_single_node("srv", &plan, &nodes, &ctx, run_id).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), serde_json::Value::Null);
    }

    // -- execute_batch tests --

    #[tokio::test]
    async fn test_execute_batch_linear_pipeline() {
        let graph = make_graph(
            vec![
                Node {
                    id: "s".into(),
                    node_type: "mock".into(),
                    kind: NodeKind::Source,
                    config: serde_json::Value::Null,
                    inputs: vec![],
                    outputs: vec![PortSpec::new("default")],
                },
                Node {
                    id: "p".into(),
                    node_type: "mock".into(),
                    kind: NodeKind::Processor,
                    config: serde_json::Value::Null,
                    inputs: vec![PortSpec::new("default")],
                    outputs: vec![PortSpec::new("default")],
                },
                Node {
                    id: "k".into(),
                    node_type: "mock".into(),
                    kind: NodeKind::Sink,
                    config: serde_json::Value::Null,
                    inputs: vec![PortSpec::new("default")],
                    outputs: vec![],
                },
            ],
            vec![
                Edge {
                    id: "e1".into(),
                    from_node: "s".into(),
                    from_port: "default".into(),
                    to_node: "p".into(),
                    to_port: "default".into(),
                },
                Edge {
                    id: "e2".into(),
                    from_node: "p".into(),
                    from_port: "default".into(),
                    to_node: "k".into(),
                    to_port: "default".into(),
                },
            ],
        );
        let plan = scheduler::schedule(&graph).unwrap();
        let mut nodes: HashMap<String, Box<dyn NodeExecutor>> = HashMap::new();
        nodes.insert(
            "s".into(),
            Box::new(MockNode::source(serde_json::json!("start"))),
        );
        nodes.insert(
            "p".into(),
            Box::new(MockNode::processor(serde_json::Value::Null)),
        ); // echo
        nodes.insert("k".into(), Box::new(MockNode::sink()));
        let ctx = ExecutionContext::new();

        let result = execute_batch(&graph, &plan, &nodes, &ctx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_batch_node_failure_stops_pipeline() {
        let graph = make_graph(
            vec![
                Node {
                    id: "s".into(),
                    node_type: "mock".into(),
                    kind: NodeKind::Source,
                    config: serde_json::Value::Null,
                    inputs: vec![],
                    outputs: vec![PortSpec::new("default")],
                },
                Node {
                    id: "fail".into(),
                    node_type: "fail".into(),
                    kind: NodeKind::Processor,
                    config: serde_json::Value::Null,
                    inputs: vec![PortSpec::new("default")],
                    outputs: vec![PortSpec::new("default")],
                },
                Node {
                    id: "k".into(),
                    node_type: "mock".into(),
                    kind: NodeKind::Sink,
                    config: serde_json::Value::Null,
                    inputs: vec![PortSpec::new("default")],
                    outputs: vec![],
                },
            ],
            vec![
                Edge {
                    id: "e1".into(),
                    from_node: "s".into(),
                    from_port: "default".into(),
                    to_node: "fail".into(),
                    to_port: "default".into(),
                },
                Edge {
                    id: "e2".into(),
                    from_node: "fail".into(),
                    from_port: "default".into(),
                    to_node: "k".into(),
                    to_port: "default".into(),
                },
            ],
        );
        let plan = scheduler::schedule(&graph).unwrap();
        let mut nodes: HashMap<String, Box<dyn NodeExecutor>> = HashMap::new();
        nodes.insert(
            "s".into(),
            Box::new(MockNode::source(serde_json::json!("data"))),
        );
        nodes.insert("fail".into(), Box::new(FailNode));
        nodes.insert("k".into(), Box::new(MockNode::sink()));
        let ctx = ExecutionContext::new();

        let result = execute_batch(&graph, &plan, &nodes, &ctx).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ExecutionError::NodeFailed { node_id, .. } => assert_eq!(node_id, "fail"),
            e => panic!("expected NodeFailed, got {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_execute_batch_no_entry_nodes() {
        // A graph with only a processor (no source/service) → error
        let graph = make_graph(
            vec![Node {
                id: "p".into(),
                node_type: "mock".into(),
                kind: NodeKind::Processor,
                config: serde_json::Value::Null,
                inputs: vec![PortSpec::new("default")],
                outputs: vec![PortSpec::new("default")],
            }],
            vec![],
        );
        let plan = scheduler::schedule(&graph).unwrap();
        let mut nodes: HashMap<String, Box<dyn NodeExecutor>> = HashMap::new();
        nodes.insert(
            "p".into(),
            Box::new(MockNode::processor(serde_json::Value::Null)),
        );
        let ctx = ExecutionContext::new();

        let result = execute_batch(&graph, &plan, &nodes, &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_batch_data_flows_through() {
        // Source → Echo processor → Sink: verify data passes through context
        let graph = make_graph(
            vec![
                Node {
                    id: "s".into(),
                    node_type: "mock".into(),
                    kind: NodeKind::Source,
                    config: serde_json::Value::Null,
                    inputs: vec![],
                    outputs: vec![PortSpec::new("default")],
                },
                Node {
                    id: "echo".into(),
                    node_type: "mock".into(),
                    kind: NodeKind::Processor,
                    config: serde_json::Value::Null,
                    inputs: vec![PortSpec::new("default")],
                    outputs: vec![PortSpec::new("default")],
                },
            ],
            vec![Edge {
                id: "e1".into(),
                from_node: "s".into(),
                from_port: "default".into(),
                to_node: "echo".into(),
                to_port: "default".into(),
            }],
        );
        let plan = scheduler::schedule(&graph).unwrap();
        let mut nodes: HashMap<String, Box<dyn NodeExecutor>> = HashMap::new();
        nodes.insert(
            "s".into(),
            Box::new(MockNode::source(serde_json::json!({"msg": "hello"}))),
        );
        // Echo node (output is null → echoes input)
        nodes.insert(
            "echo".into(),
            Box::new(MockNode::processor(serde_json::Value::Null)),
        );
        let ctx = ExecutionContext::new();

        let result = execute_batch(&graph, &plan, &nodes, &ctx).await;
        assert!(result.is_ok());
    }

    // -- PipelineTriggerImpl tests --

    #[tokio::test]
    async fn test_pipeline_trigger_timeout() {
        // Create a trigger with very short timeout
        use std::sync::Arc;

        /// Mock node that sleeps forever
        struct SlowNode;

        #[async_trait::async_trait]
        impl NodeExecutor for SlowNode {
            fn node_type(&self) -> &str {
                "slow"
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
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                Ok(NodeOutput::data(serde_json::json!("never")))
            }
        }

        let graph = make_graph(
            vec![
                Node {
                    id: "srv".into(),
                    node_type: "mock".into(),
                    kind: NodeKind::Service,
                    config: serde_json::Value::Null,
                    inputs: vec![],
                    outputs: vec![PortSpec::new("default")],
                },
                Node {
                    id: "slow".into(),
                    node_type: "slow".into(),
                    kind: NodeKind::Processor,
                    config: serde_json::Value::Null,
                    inputs: vec![PortSpec::new("default")],
                    outputs: vec![PortSpec::new("default")],
                },
            ],
            vec![Edge {
                id: "e1".into(),
                from_node: "srv".into(),
                from_port: "default".into(),
                to_node: "slow".into(),
                to_port: "default".into(),
            }],
        );
        let plan = Arc::new(scheduler::schedule(&graph).unwrap());
        let mut nodes: HashMap<String, Box<dyn NodeExecutor>> = HashMap::new();
        nodes.insert(
            "srv".into(),
            Box::new(MockNode {
                kind: NodeKind::Service,
                output: serde_json::Value::Null,
            }),
        );
        nodes.insert("slow".into(), Box::new(SlowNode));
        let nodes = Arc::new(nodes);
        let ctx = Arc::new(ExecutionContext::new());

        let trigger = PipelineTriggerImpl {
            plan,
            nodes,
            ctx,
            pipeline_timeout_secs: 1, // 1 second timeout
        };

        let result = trigger
            .trigger("srv", "default", serde_json::json!("go"))
            .await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("timed out"));
    }

    impl MockNode {
        fn source(output: serde_json::Value) -> Self {
            Self {
                kind: NodeKind::Source,
                output,
            }
        }
        fn processor(output: serde_json::Value) -> Self {
            Self {
                kind: NodeKind::Processor,
                output,
            }
        }
        fn sink() -> Self {
            Self {
                kind: NodeKind::Sink,
                output: serde_json::Value::Null,
            }
        }
    }

    // -- port routing tests --

    /// Mock node that passes its input through on one named port
    struct RouteNode(&'static str);

    #[async_trait::async_trait]
    impl NodeExecutor for RouteNode {
        fn node_type(&self) -> &str {
            "route"
        }
        fn node_kind(&self) -> NodeKind {
            NodeKind::Processor
        }
        async fn init(&mut self, _config: &serde_json::Value) -> NodeResult<()> {
            Ok(())
        }
        async fn execute(
            &self,
            input: serde_json::Value,
            _ctx: &ExecutionContext,
        ) -> NodeResult<NodeOutput> {
            Ok(NodeOutput::multi(HashMap::from([(
                self.0.to_string(),
                input,
            )])))
        }
    }

    fn test_node(id: &str, kind: NodeKind) -> Node {
        Node {
            id: id.into(),
            node_type: "mock".into(),
            kind,
            config: serde_json::Value::Null,
            inputs: vec![],
            outputs: vec![],
        }
    }

    fn test_edge(from: &str, port: &str, to: &str) -> Edge {
        Edge {
            id: format!("{}.{}->{}", from, port, to),
            from_node: from.into(),
            from_port: port.into(),
            to_node: to.into(),
            to_port: "default".into(),
        }
    }

    fn echo(kind: NodeKind) -> Box<dyn NodeExecutor> {
        Box::new(MockNode {
            kind,
            output: serde_json::Value::Null,
        })
    }

    /// Source "s" emits "data" into router "r"; the remaining nodes echo.
    /// Returns the ids of the nodes that ran and each node's input-derived output.
    async fn run_routed(
        router: RouteNode,
        others: &[(&str, NodeKind)],
        edges: Vec<Edge>,
    ) -> (Vec<String>, HashMap<String, serde_json::Value>) {
        let mut graph_nodes = vec![
            test_node("s", NodeKind::Source),
            test_node("r", NodeKind::Processor),
        ];
        let mut nodes: HashMap<String, Box<dyn NodeExecutor>> = HashMap::new();
        nodes.insert(
            "s".into(),
            Box::new(MockNode {
                kind: NodeKind::Source,
                output: serde_json::json!("data"),
            }),
        );
        nodes.insert("r".into(), Box::new(router));
        for (id, kind) in others {
            graph_nodes.push(test_node(id, *kind));
            nodes.insert(id.to_string(), echo(*kind));
        }
        let mut all_edges = vec![test_edge("s", "default", "r")];
        all_edges.extend(edges);

        let plan = scheduler::schedule(&make_graph(graph_nodes, all_edges)).unwrap();
        let ctx = ExecutionContext::new();
        ctx.start_run("run".into(), "s".into());
        for stage in &plan.stages {
            for node_id in &stage.node_ids {
                run_node(node_id, &plan, &nodes, &ctx, "run").await.unwrap();
            }
        }

        let mut ran: Vec<String> = ctx
            .get_traces("run")
            .into_iter()
            .map(|t| t.node_id)
            .collect();
        ran.sort();
        let outputs = ctx.finish_run("run").unwrap().data;
        (ran, outputs)
    }

    #[tokio::test]
    async fn test_only_taken_branch_executes() {
        let (ran, outputs) = run_routed(
            RouteNode("yes"),
            &[("a", NodeKind::Sink), ("b", NodeKind::Sink)],
            vec![test_edge("r", "yes", "a"), test_edge("r", "no", "b")],
        )
        .await;
        assert_eq!(ran, ["a", "r", "s"]);
        assert_eq!(outputs["a"], "data");
    }

    #[tokio::test]
    async fn test_untaken_branch_is_skipped_transitively() {
        let (ran, _) = run_routed(
            RouteNode("yes"),
            &[
                ("a", NodeKind::Sink),
                ("b", NodeKind::Processor),
                ("b2", NodeKind::Sink),
            ],
            vec![
                test_edge("r", "yes", "a"),
                test_edge("r", "no", "b"),
                test_edge("b", "default", "b2"),
            ],
        )
        .await;
        assert_eq!(ran, ["a", "r", "s"]);
    }

    #[tokio::test]
    async fn test_branches_rejoin_with_single_input() {
        let (ran, outputs) = run_routed(
            RouteNode("no"),
            &[
                ("a", NodeKind::Processor),
                ("b", NodeKind::Processor),
                ("join", NodeKind::Sink),
            ],
            vec![
                test_edge("r", "yes", "a"),
                test_edge("r", "no", "b"),
                test_edge("a", "default", "join"),
                test_edge("b", "default", "join"),
            ],
        )
        .await;
        assert_eq!(ran, ["b", "join", "r", "s"]);
        assert_eq!(outputs["join"], "data");
    }

    #[tokio::test]
    async fn test_default_edge_inactive_when_emitted_port_is_wired() {
        let (ran, _) = run_routed(
            RouteNode("yes"),
            &[("a", NodeKind::Sink), ("fallback", NodeKind::Sink)],
            vec![
                test_edge("r", "yes", "a"),
                test_edge("r", "default", "fallback"),
            ],
        )
        .await;
        assert_eq!(ran, ["a", "r", "s"]);
    }

    #[tokio::test]
    async fn test_unwired_port_falls_through_to_default() {
        let (ran, outputs) = run_routed(
            RouteNode("yes"),
            &[("next", NodeKind::Sink)],
            vec![test_edge("r", "default", "next")],
        )
        .await;
        assert_eq!(ran, ["next", "r", "s"]);
        assert_eq!(outputs["next"], "data");
    }

    #[tokio::test]
    async fn test_skip_status_skips_downstream() {
        let graph = make_graph(
            vec![
                test_node("s", NodeKind::Source),
                test_node("k", NodeKind::Sink),
            ],
            vec![test_edge("s", "default", "k")],
        );
        let plan = scheduler::schedule(&graph).unwrap();
        let mut nodes: HashMap<String, Box<dyn NodeExecutor>> = HashMap::new();
        nodes.insert("s".into(), Box::new(SkipNode));
        nodes.insert("k".into(), echo(NodeKind::Sink));
        let ctx = ExecutionContext::new();
        ctx.start_run("run".into(), "s".into());

        assert_eq!(
            run_node("s", &plan, &nodes, &ctx, "run").await.unwrap(),
            None
        );
        assert_eq!(
            run_node("k", &plan, &nodes, &ctx, "run").await.unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn test_trigger_only_feeds_fired_port() {
        // Service "srv" has two routes; "shared" hangs off the fired route and
        // also has a direct edge from the route that did not fire.
        let graph = make_graph(
            vec![
                test_node("srv", NodeKind::Service),
                test_node("a", NodeKind::Processor),
                test_node("shared", NodeKind::Processor),
            ],
            vec![
                test_edge("srv", "one", "a"),
                test_edge("a", "default", "shared"),
                test_edge("srv", "two", "shared"),
            ],
        );
        let plan = Arc::new(scheduler::schedule(&graph).unwrap());
        let mut nodes: HashMap<String, Box<dyn NodeExecutor>> = HashMap::new();
        nodes.insert("srv".into(), echo(NodeKind::Service));
        nodes.insert("a".into(), echo(NodeKind::Processor));
        nodes.insert("shared".into(), echo(NodeKind::Processor));
        let trigger = PipelineTriggerImpl {
            plan,
            nodes: Arc::new(nodes),
            ctx: Arc::new(ExecutionContext::new()),
            pipeline_timeout_secs: 5,
        };

        let result = trigger
            .execute_pipeline("srv", "one", serde_json::json!("req"))
            .await
            .unwrap();
        assert_eq!(result, "req");
    }
}
