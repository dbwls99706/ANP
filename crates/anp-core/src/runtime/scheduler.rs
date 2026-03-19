use std::collections::{HashMap, HashSet, VecDeque};

use crate::graph::spec::{AnpGraph, Edge, NodeKind};

/// A group of nodes that can execute in parallel
#[derive(Debug, Clone)]
pub struct ExecutionStage {
    pub node_ids: Vec<String>,
}

/// Complete execution plan for a graph
#[derive(Debug)]
pub struct ExecutionPlan {
    /// Ordered stages of parallel execution
    pub stages: Vec<ExecutionStage>,
    /// NodeId → PortName → Edges (O(1) port-based lookup, outbound edges)
    pub edge_map: HashMap<String, HashMap<String, Vec<Edge>>>,
    /// NodeId → stage index (O(1) node lookup)
    pub node_map: HashMap<String, usize>,
    /// Source + Service nodes (pipeline entry points)
    pub entry_nodes: Vec<String>,
    /// Reverse index: ToNodeId → list of FromNodeIds (O(1) input gathering)
    pub reverse_edge_map: HashMap<String, Vec<String>>,
}

/// Errors from scheduling
#[derive(Debug, thiserror::Error)]
pub enum ScheduleError {
    #[error("cycle detected in graph")]
    CycleDetected,

    #[error("node '{0}' referenced in edge but not found")]
    NodeNotFound(String),
}

/// Create an execution plan from a validated graph
pub fn schedule(graph: &AnpGraph) -> Result<ExecutionPlan, ScheduleError> {
    let node_ids: HashSet<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();

    // Build in-degree map and adjacency list
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();

    for node in &graph.nodes {
        in_degree.insert(node.id.as_str(), 0);
        adj.insert(node.id.as_str(), Vec::new());
    }

    for edge in &graph.edges {
        if !node_ids.contains(edge.from_node.as_str()) {
            return Err(ScheduleError::NodeNotFound(edge.from_node.clone()));
        }
        if !node_ids.contains(edge.to_node.as_str()) {
            return Err(ScheduleError::NodeNotFound(edge.to_node.clone()));
        }
        if let Some(neighbors) = adj.get_mut(edge.from_node.as_str()) {
            neighbors.push(edge.to_node.as_str());
        }
        if let Some(deg) = in_degree.get_mut(edge.to_node.as_str()) {
            *deg += 1;
        }
    }

    // Topological sort by levels (Kahn's algorithm)
    let mut queue: VecDeque<&str> = VecDeque::new();
    for (&node_id, &deg) in &in_degree {
        if deg == 0 {
            queue.push_back(node_id);
        }
    }

    let mut stages: Vec<ExecutionStage> = Vec::new();
    let mut node_map: HashMap<String, usize> = HashMap::new();
    let mut processed = 0;

    while !queue.is_empty() {
        let stage_idx = stages.len();
        let mut stage_nodes = Vec::new();

        // All nodes in queue at this level form one stage
        let level_size = queue.len();
        for _ in 0..level_size {
            let node_id = match queue.pop_front() {
                Some(id) => id,
                None => break,
            };
            stage_nodes.push(node_id.to_string());
            node_map.insert(node_id.to_string(), stage_idx);
            processed += 1;

            for &next in adj.get(node_id).unwrap_or(&Vec::new()) {
                if let Some(deg) = in_degree.get_mut(next) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(next);
                    }
                }
            }
        }

        stages.push(ExecutionStage {
            node_ids: stage_nodes,
        });
    }

    if processed != graph.nodes.len() {
        return Err(ScheduleError::CycleDetected);
    }

    // Build nested edge_map: NodeId → PortName → Vec<Edge>
    let mut edge_map: HashMap<String, HashMap<String, Vec<Edge>>> = HashMap::new();
    for node in &graph.nodes {
        edge_map.insert(node.id.clone(), HashMap::new());
    }
    for edge in &graph.edges {
        edge_map
            .entry(edge.from_node.clone())
            .or_default()
            .entry(edge.from_port.clone())
            .or_default()
            .push(edge.clone());
    }

    // Find entry nodes (Source + Service)
    let entry_nodes: Vec<String> = graph
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Source | NodeKind::Service))
        .map(|n| n.id.clone())
        .collect();

    // Build reverse edge map: ToNodeId → [FromNodeId] (deduped)
    let mut reverse_edge_map: HashMap<String, Vec<String>> = HashMap::new();
    for edge in &graph.edges {
        let sources = reverse_edge_map.entry(edge.to_node.clone()).or_default();
        if !sources.contains(&edge.from_node) {
            sources.push(edge.from_node.clone());
        }
    }

    Ok(ExecutionPlan {
        stages,
        edge_map,
        node_map,
        entry_nodes,
        reverse_edge_map,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::spec::*;

    fn make_graph(nodes: Vec<Node>, edges: Vec<Edge>) -> AnpGraph {
        AnpGraph {
            version: GraphVersion::V0_1,
            metadata: GraphMetadata {
                name: "test".to_string(),
                description: String::new(),
            },
            nodes,
            edges,
        }
    }

    #[test]
    fn test_linear_pipeline() {
        let graph = make_graph(
            vec![
                Node {
                    id: "s".into(),
                    node_type: "file_read".into(),
                    kind: NodeKind::Source,
                    config: serde_json::Value::Null,
                    inputs: vec![],
                    outputs: vec![PortSpec::new("default")],
                },
                Node {
                    id: "p".into(),
                    node_type: "transform".into(),
                    kind: NodeKind::Processor,
                    config: serde_json::Value::Null,
                    inputs: vec![PortSpec::new("default")],
                    outputs: vec![PortSpec::new("default")],
                },
                Node {
                    id: "k".into(),
                    node_type: "console".into(),
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

        let plan = schedule(&graph).unwrap();
        assert_eq!(plan.stages.len(), 3);
        assert_eq!(plan.stages[0].node_ids, vec!["s"]);
        assert_eq!(plan.stages[1].node_ids, vec!["p"]);
        assert_eq!(plan.stages[2].node_ids, vec!["k"]);
        assert_eq!(plan.entry_nodes, vec!["s"]);
    }

    #[test]
    fn test_parallel_stages() {
        // s1 and s2 are independent sources → same stage
        let graph = make_graph(
            vec![
                Node {
                    id: "s1".into(),
                    node_type: "file_read".into(),
                    kind: NodeKind::Source,
                    config: serde_json::Value::Null,
                    inputs: vec![],
                    outputs: vec![PortSpec::new("default")],
                },
                Node {
                    id: "s2".into(),
                    node_type: "file_read".into(),
                    kind: NodeKind::Source,
                    config: serde_json::Value::Null,
                    inputs: vec![],
                    outputs: vec![PortSpec::new("default")],
                },
                Node {
                    id: "k".into(),
                    node_type: "console".into(),
                    kind: NodeKind::Sink,
                    config: serde_json::Value::Null,
                    inputs: vec![PortSpec::new("default")],
                    outputs: vec![],
                },
            ],
            vec![
                Edge {
                    id: "e1".into(),
                    from_node: "s1".into(),
                    from_port: "default".into(),
                    to_node: "k".into(),
                    to_port: "default".into(),
                },
                Edge {
                    id: "e2".into(),
                    from_node: "s2".into(),
                    from_port: "default".into(),
                    to_node: "k".into(),
                    to_port: "default".into(),
                },
            ],
        );

        let plan = schedule(&graph).unwrap();
        assert_eq!(plan.stages.len(), 2);
        // First stage has both sources (parallel)
        assert_eq!(plan.stages[0].node_ids.len(), 2);
        assert_eq!(plan.entry_nodes.len(), 2);
    }
}
