use std::collections::HashMap;

use tracing::{debug, error, info, warn};

use crate::graph::spec::{AnpGraph, NodeKind};
use crate::node::traits::{NodeExecutor, NodeOutput, NodeStatus};
use crate::runtime::context::ExecutionContext;
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

/// Execute a batch pipeline (Source → Processor → Sink)
pub async fn execute_batch(
    _graph: &AnpGraph,
    plan: &ExecutionPlan,
    nodes: &HashMap<String, Box<dyn NodeExecutor>>,
    ctx: &ExecutionContext,
) -> Result<(), ExecutionError> {
    let run_id = uuid::Uuid::new_v4().to_string();

    // Find the first source node as the run trigger
    let source_node = plan
        .entry_nodes
        .first()
        .cloned()
        .unwrap_or_else(|| "unknown".to_string());

    ctx.start_run(run_id.clone(), source_node);

    info!(run_id = %run_id, "starting batch execution");

    for (stage_idx, stage) in plan.stages.iter().enumerate() {
        debug!(stage = stage_idx, nodes = ?stage.node_ids, "executing stage");

        // Execute nodes in this stage
        // For simplicity in v0.1, execute sequentially within a stage
        // TODO: Use tokio::join! for true parallel execution
        for node_id in &stage.node_ids {
            let node = nodes
                .get(node_id)
                .ok_or_else(|| ExecutionError::NodeNotFound(node_id.clone()))?;

            // Service nodes are skipped in batch mode
            if node.node_kind() == NodeKind::Service {
                warn!(node_id = %node_id, "skipping service node in batch mode");
                continue;
            }

            // Gather input from upstream nodes via edge_map
            let input = gather_input(node_id, plan, ctx, &run_id);

            debug!(
                node_id = %node_id,
                node_type = %node.node_type(),
                "executing node"
            );

            match node.execute(input, ctx).await {
                Ok(output) => {
                    // Log node output logs
                    for log in &output.logs {
                        debug!(node_id = %node_id, "{}", log);
                    }

                    match output.status {
                        NodeStatus::Ok => {
                            // Store output for downstream consumption
                            store_output(node_id, &output, plan, ctx, &run_id);
                        }
                        NodeStatus::Skip => {
                            info!(node_id = %node_id, "node returned Skip, stopping pipeline branch");
                        }
                        NodeStatus::Error => {
                            error!(
                                node_id = %node_id,
                                error = ?output.error,
                                "node returned Error status"
                            );
                            // Store error output for error port routing
                            store_output(node_id, &output, plan, ctx, &run_id);
                        }
                    }
                }
                Err(e) => {
                    error!(node_id = %node_id, error = %e, "node execution failed");
                    return Err(ExecutionError::NodeFailed {
                        node_id: node_id.clone(),
                        message: e.to_string(),
                    });
                }
            }
        }
    }

    // Cleanup run
    if let Some(run_ctx) = ctx.finish_run(&run_id) {
        let elapsed = run_ctx.metadata.started_at.elapsed();
        info!(run_id = %run_id, elapsed_ms = elapsed.as_millis(), "batch execution completed");
    }

    Ok(())
}

/// Gather input data for a node from its upstream edges
fn gather_input(
    node_id: &str,
    plan: &ExecutionPlan,
    ctx: &ExecutionContext,
    run_id: &str,
) -> serde_json::Value {
    // Find all edges that target this node
    let mut inputs: Vec<serde_json::Value> = Vec::new();

    for (from_id, ports) in &plan.edge_map {
        for edges in ports.values() {
            for edge in edges {
                if edge.to_node == node_id {
                    if let Some(output) = ctx.get_node_output(run_id, from_id) {
                        inputs.push(output);
                    }
                }
            }
        }
    }

    match inputs.len() {
        0 => serde_json::Value::Null,
        1 => inputs.into_iter().next().unwrap(),
        _ => serde_json::Value::Array(inputs),
    }
}

/// Store node output into the context, routing through output ports
fn store_output(
    node_id: &str,
    output: &NodeOutput,
    _plan: &ExecutionPlan,
    ctx: &ExecutionContext,
    run_id: &str,
) {
    // Store the default port value, or merge all ports
    if let Some(default_val) = output.ports.get("default") {
        ctx.set_node_output(run_id, node_id, default_val.clone());
    } else if output.ports.len() == 1 {
        // Single non-default port
        let val = output.ports.values().next().unwrap();
        ctx.set_node_output(run_id, node_id, val.clone());
    } else if !output.ports.is_empty() {
        // Multiple ports — store as object
        let map: serde_json::Map<String, serde_json::Value> = output
            .ports
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        ctx.set_node_output(run_id, node_id, serde_json::Value::Object(map));
    }
}
