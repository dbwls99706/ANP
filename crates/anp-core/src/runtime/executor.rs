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
        self.ctx.set_node_output(&run_id, source_node, data);

        // Find downstream nodes reachable from source_node via the specified port
        let downstream_nodes = find_downstream_from_port(source_node, port, &self.plan);

        // Find the source node's stage index
        let source_stage = self.plan.node_map.get(source_node).copied().unwrap_or(0);

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

            for node_id in &stage_nodes {
                match execute_single_node(node_id, &self.plan, &self.nodes, &self.ctx, &run_id)
                    .await
                {
                    Ok(val) => {
                        last_output = val;
                    }
                    Err(e) => {
                        error!(node_id = %node_id, error = %e, "node execution failed in trigger pipeline");
                        self.ctx.finish_run(&run_id);
                        return Err(e);
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
    let node = nodes
        .get(node_id)
        .ok_or_else(|| NodeError::Other(format!("node '{}' not found", node_id)))?;

    // Service nodes are never executed directly
    if node.node_kind() == NodeKind::Service {
        return Ok(serde_json::Value::Null);
    }

    let input = gather_input(node_id, plan, ctx, run_id);
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
                    Ok(stored)
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
                    Ok(serde_json::Value::Null)
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
                    Ok(serde_json::json!({
                        "error": err_msg,
                        "node_id": node_id
                    }))
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

        for node_id in &stage.node_ids {
            let node = nodes
                .get(node_id)
                .ok_or_else(|| ExecutionError::NodeNotFound(node_id.clone()))?;

            // Service nodes are skipped in batch mode
            if node.node_kind() == NodeKind::Service {
                warn!(node_id = %node_id, "skipping service node in batch mode");
                continue;
            }

            let input = gather_input(node_id, plan, ctx, &run_id);
            let node_type_str = node.node_type().to_string();

            debug!(
                node_id = %node_id,
                node_type = %node_type_str,
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
                                &run_id,
                                NodeTrace {
                                    node_id: node_id.clone(),
                                    node_type: node_type_str,
                                    duration_us,
                                    success: true,
                                    error: None,
                                    output_ports,
                                },
                            );
                            store_output(node_id, &output, plan, ctx, &run_id);
                        }
                        NodeStatus::Skip => {
                            ctx.record_trace(
                                &run_id,
                                NodeTrace {
                                    node_id: node_id.clone(),
                                    node_type: node_type_str,
                                    duration_us,
                                    success: true,
                                    error: None,
                                    output_ports: vec!["skip".to_string()],
                                },
                            );
                            info!(node_id = %node_id, "node returned Skip, stopping pipeline branch");
                        }
                        NodeStatus::Error => {
                            let err_msg = output.error.clone();
                            ctx.record_trace(
                                &run_id,
                                NodeTrace {
                                    node_id: node_id.clone(),
                                    node_type: node_type_str,
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
                            store_error_output(node_id, &output, plan, ctx, &run_id);
                        }
                    }
                }
                Err(e) => {
                    let duration_us = start.elapsed().as_micros() as u64;
                    ctx.record_trace(
                        &run_id,
                        NodeTrace {
                            node_id: node_id.clone(),
                            node_type: node_type_str,
                            duration_us,
                            success: false,
                            error: Some(e.to_string()),
                            output_ports: vec![],
                        },
                    );
                    error!(node_id = %node_id, error = %e, "node execution failed");
                    return Err(ExecutionError::NodeFailed {
                        node_id: node_id.clone(),
                        message: e.to_string(),
                    });
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

                let input = gather_input(node_id, &plan, &ctx, &run_id);
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
            let mut sigterm =
                signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");
            tokio::select! {
                _ = ctrl_c => info!("received SIGINT (Ctrl+C)"),
                _ = sigterm.recv() => info!("received SIGTERM"),
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

/// Gather input data for a node from its upstream edges
fn gather_input(
    node_id: &str,
    plan: &ExecutionPlan,
    ctx: &ExecutionContext,
    run_id: &str,
) -> serde_json::Value {
    let mut inputs: Vec<serde_json::Value> = Vec::new();
    let mut seen_sources = std::collections::HashSet::new();

    for (from_id, ports) in &plan.edge_map {
        for edges in ports.values() {
            for edge in edges {
                if edge.to_node == node_id && seen_sources.insert(from_id.clone()) {
                    if let Some(output) = ctx.get_node_output(run_id, from_id) {
                        inputs.push(output);
                    }
                }
            }
        }
    }

    match inputs.len() {
        0 => serde_json::Value::Null,
        1 => inputs.into_iter().next().unwrap_or(serde_json::Value::Null),
        _ => serde_json::Value::Array(inputs),
    }
}

/// Store node output into the context. Returns the stored value.
fn store_output(
    node_id: &str,
    output: &NodeOutput,
    _plan: &ExecutionPlan,
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
            ctx.set_node_output(run_id, node_id, error_val);
            debug!(node_id = %node_id, "routed error to error port");
            return;
        }
    }

    ctx.set_node_output(run_id, node_id, error_val);
    warn!(node_id = %node_id, error = ?output.error, "no error port found, error logged");
}
