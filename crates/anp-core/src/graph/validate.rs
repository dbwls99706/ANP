use std::collections::{HashMap, HashSet};

use crate::graph::spec::{AnpGraph, NodeKind};

/// Validation errors for ANP graphs
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("duplicate node id: {0}")]
    DuplicateNodeId(String),

    #[error("edge {edge_id} references non-existent node: {node_id}")]
    InvalidNodeRef { edge_id: String, node_id: String },

    #[error("edge {edge_id} references non-existent port '{port}' on node '{node_id}'")]
    InvalidPortRef {
        edge_id: String,
        node_id: String,
        port: String,
    },

    #[error("graph contains a cycle involving node: {0}")]
    CycleDetected(String),

    #[error("sink node '{0}' cannot have outgoing edges")]
    SinkHasOutput(String),

    #[error("service node '{0}' cannot have incoming edges")]
    ServiceHasInput(String),
}

/// Validate an ANP graph against all 6 rules
pub fn validate(graph: &AnpGraph) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();

    check_duplicate_node_ids(graph, &mut errors);
    check_edge_references(graph, &mut errors);
    check_port_references(graph, &mut errors);
    check_node_kind_rules(graph, &mut errors);
    check_cycles(graph, &mut errors);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Rule 1: No duplicate node IDs
fn check_duplicate_node_ids(graph: &AnpGraph, errors: &mut Vec<ValidationError>) {
    let mut seen = HashSet::new();
    for node in &graph.nodes {
        if !seen.insert(&node.id) {
            errors.push(ValidationError::DuplicateNodeId(node.id.clone()));
        }
    }
}

/// Rule 2: Edge references must point to existing nodes
fn check_edge_references(graph: &AnpGraph, errors: &mut Vec<ValidationError>) {
    let node_ids: HashSet<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();

    for edge in &graph.edges {
        if !node_ids.contains(edge.from_node.as_str()) {
            errors.push(ValidationError::InvalidNodeRef {
                edge_id: edge.id.clone(),
                node_id: edge.from_node.clone(),
            });
        }
        if !node_ids.contains(edge.to_node.as_str()) {
            errors.push(ValidationError::InvalidNodeRef {
                edge_id: edge.id.clone(),
                node_id: edge.to_node.clone(),
            });
        }
    }
}

/// Rule 3: Edge ports must exist on the referenced nodes
fn check_port_references(graph: &AnpGraph, errors: &mut Vec<ValidationError>) {
    let node_map: HashMap<&str, &crate::graph::spec::Node> =
        graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    for edge in &graph.edges {
        // Check from_port exists on source node's outputs
        if let Some(node) = node_map.get(edge.from_node.as_str()) {
            if edge.from_port != "default"
                && !node.outputs.iter().any(|p| p.name == edge.from_port)
            {
                errors.push(ValidationError::InvalidPortRef {
                    edge_id: edge.id.clone(),
                    node_id: edge.from_node.clone(),
                    port: edge.from_port.clone(),
                });
            }
        }

        // Check to_port exists on target node's inputs
        if let Some(node) = node_map.get(edge.to_node.as_str()) {
            if edge.to_port != "default"
                && !node.inputs.iter().any(|p| p.name == edge.to_port)
            {
                errors.push(ValidationError::InvalidPortRef {
                    edge_id: edge.id.clone(),
                    node_id: edge.to_node.clone(),
                    port: edge.to_port.clone(),
                });
            }
        }
    }
}

/// Rule 4 & 5: NodeKind edge rules
/// - Sink nodes cannot have outgoing edges
/// - Service nodes cannot have incoming edges
fn check_node_kind_rules(graph: &AnpGraph, errors: &mut Vec<ValidationError>) {
    let node_map: HashMap<&str, &crate::graph::spec::Node> =
        graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    for edge in &graph.edges {
        // Sink cannot be a source of an edge
        if let Some(node) = node_map.get(edge.from_node.as_str()) {
            if node.kind == NodeKind::Sink {
                errors.push(ValidationError::SinkHasOutput(node.id.clone()));
            }
        }

        // Service cannot be a target of an edge
        if let Some(node) = node_map.get(edge.to_node.as_str()) {
            if node.kind == NodeKind::Service {
                errors.push(ValidationError::ServiceHasInput(node.id.clone()));
            }
        }
    }
}

/// Rule 6: DAG cycle detection using DFS
fn check_cycles(graph: &AnpGraph, errors: &mut Vec<ValidationError>) {
    let node_ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();

    // Build adjacency list
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for id in &node_ids {
        adj.insert(id, Vec::new());
    }
    for edge in &graph.edges {
        if let Some(neighbors) = adj.get_mut(edge.from_node.as_str()) {
            neighbors.push(edge.to_node.as_str());
        }
    }

    // DFS cycle detection
    let mut visited = HashSet::new();
    let mut in_stack = HashSet::new();

    for &start in &node_ids {
        if visited.contains(start) {
            continue;
        }
        let mut stack = vec![(start, false)];
        while let Some((node, backtrack)) = stack.pop() {
            if backtrack {
                in_stack.remove(node);
                continue;
            }
            if in_stack.contains(node) {
                errors.push(ValidationError::CycleDetected(node.to_string()));
                return;
            }
            if visited.contains(node) {
                continue;
            }
            visited.insert(node);
            in_stack.insert(node);
            stack.push((node, true)); // backtrack marker

            if let Some(neighbors) = adj.get(node) {
                for &next in neighbors {
                    if in_stack.contains(next) {
                        errors.push(ValidationError::CycleDetected(next.to_string()));
                        return;
                    }
                    if !visited.contains(next) {
                        stack.push((next, false));
                    }
                }
            }
        }
    }
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

    fn source_node(id: &str) -> Node {
        Node {
            id: id.to_string(),
            node_type: "file_read".to_string(),
            kind: NodeKind::Source,
            config: serde_json::Value::Null,
            inputs: vec![],
            outputs: vec![PortSpec::new("default")],
        }
    }

    fn processor_node(id: &str) -> Node {
        Node {
            id: id.to_string(),
            node_type: "transform".to_string(),
            kind: NodeKind::Processor,
            config: serde_json::Value::Null,
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
        }
    }

    fn sink_node(id: &str) -> Node {
        Node {
            id: id.to_string(),
            node_type: "console".to_string(),
            kind: NodeKind::Sink,
            config: serde_json::Value::Null,
            inputs: vec![PortSpec::new("default")],
            outputs: vec![],
        }
    }

    fn edge(id: &str, from: &str, to: &str) -> Edge {
        Edge {
            id: id.to_string(),
            from_node: from.to_string(),
            from_port: "default".to_string(),
            to_node: to.to_string(),
            to_port: "default".to_string(),
        }
    }

    #[test]
    fn test_valid_graph() {
        let graph = make_graph(
            vec![source_node("s"), processor_node("p"), sink_node("k")],
            vec![edge("e1", "s", "p"), edge("e2", "p", "k")],
        );
        assert!(validate(&graph).is_ok());
    }

    #[test]
    fn test_duplicate_node_id() {
        let graph = make_graph(
            vec![source_node("a"), source_node("a")],
            vec![],
        );
        let errs = validate(&graph).unwrap_err();
        assert!(errs.iter().any(|e| matches!(e, ValidationError::DuplicateNodeId(id) if id == "a")));
    }

    #[test]
    fn test_invalid_node_ref() {
        let graph = make_graph(
            vec![source_node("a")],
            vec![edge("e1", "a", "nonexistent")],
        );
        let errs = validate(&graph).unwrap_err();
        assert!(errs.iter().any(|e| matches!(e, ValidationError::InvalidNodeRef { .. })));
    }

    #[test]
    fn test_sink_has_output() {
        let graph = make_graph(
            vec![sink_node("s"), processor_node("p")],
            vec![edge("e1", "s", "p")],
        );
        let errs = validate(&graph).unwrap_err();
        assert!(errs.iter().any(|e| matches!(e, ValidationError::SinkHasOutput(_))));
    }

    #[test]
    fn test_cycle_detected() {
        let graph = make_graph(
            vec![processor_node("a"), processor_node("b")],
            vec![edge("e1", "a", "b"), edge("e2", "b", "a")],
        );
        let errs = validate(&graph).unwrap_err();
        assert!(errs.iter().any(|e| matches!(e, ValidationError::CycleDetected(_))));
    }
}
