use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

use crate::graph::spec::{AnpGraph, Edge, GraphMetadata, GraphVersion, Node, NodeKind, PortSpec};

/// Result of running optimization passes on a graph
#[derive(Debug)]
pub struct OptimizationResult {
    pub original_nodes: usize,
    pub original_edges: usize,
    pub optimized_nodes: usize,
    pub optimized_edges: usize,
    pub passes_applied: Vec<OptimizationPass>,
    pub graph: AnpGraph,
}

/// A single optimization pass result
#[derive(Debug)]
pub struct OptimizationPass {
    pub name: String,
    pub description: String,
    pub nodes_removed: usize,
    pub edges_removed: usize,
    pub nodes_fused: usize,
}

impl fmt::Display for OptimizationResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ANP Graph Optimizer")?;
        writeln!(f, "═══════════════════")?;
        writeln!(
            f,
            "Original:  {} nodes, {} edges",
            self.original_nodes, self.original_edges
        )?;

        let node_reduction = if self.original_nodes > 0 {
            ((self.original_nodes - self.optimized_nodes) as f64 / self.original_nodes as f64)
                * 100.0
        } else {
            0.0
        };

        writeln!(
            f,
            "Optimized: {} nodes, {} edges  ({:.1}% reduction)",
            self.optimized_nodes, self.optimized_edges, node_reduction
        )?;

        if !self.passes_applied.is_empty() {
            writeln!(f)?;
            writeln!(f, "Passes Applied:")?;
            for (i, pass) in self.passes_applied.iter().enumerate() {
                writeln!(f, "  {}. {:.<30} {}", i + 1, pass.name, pass.description)?;
            }
        }

        Ok(())
    }
}

/// Apply all optimization passes to a graph
pub fn optimize(graph: &AnpGraph) -> OptimizationResult {
    let original_nodes = graph.nodes.len();
    let original_edges = graph.edges.len();
    let mut optimized = graph.clone();
    let mut passes = Vec::new();

    // Pass 1: Dead Node Elimination
    let pass1 = dead_node_elimination(&mut optimized);
    if pass1.nodes_removed > 0 || pass1.edges_removed > 0 {
        passes.push(pass1);
    }

    // Pass 2: Transform Fusion
    let pass2 = transform_fusion(&mut optimized);
    if pass2.nodes_fused > 0 {
        passes.push(pass2);
    }

    // Pass 3: Unused Edge Pruning
    let pass3 = unused_edge_pruning(&mut optimized);
    if pass3.edges_removed > 0 {
        passes.push(pass3);
    }

    OptimizationResult {
        original_nodes,
        original_edges,
        optimized_nodes: optimized.nodes.len(),
        optimized_edges: optimized.edges.len(),
        passes_applied: passes,
        graph: optimized,
    }
}

/// Pass 1: Remove nodes not reachable from any Source or Service node
fn dead_node_elimination(graph: &mut AnpGraph) -> OptimizationPass {
    let entry_ids: Vec<String> = graph
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Source | NodeKind::Service))
        .map(|n| n.id.clone())
        .collect();

    // BFS from entry nodes (use owned strings to avoid borrowing graph)
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for node in &graph.nodes {
        adj.insert(node.id.clone(), Vec::new());
    }
    for edge in &graph.edges {
        adj.entry(edge.from_node.clone())
            .or_default()
            .push(edge.to_node.clone());
    }

    let mut reachable: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<String> = VecDeque::new();
    for id in entry_ids {
        reachable.insert(id.clone());
        queue.push_back(id);
    }
    while let Some(node) = queue.pop_front() {
        if let Some(neighbors) = adj.get(&node) {
            for next in neighbors {
                if reachable.insert(next.clone()) {
                    queue.push_back(next.clone());
                }
            }
        }
    }

    let original_count = graph.nodes.len();
    let original_edges = graph.edges.len();

    graph.nodes.retain(|n| reachable.contains(&n.id));
    graph.edges.retain(|e| {
        reachable.contains(&e.from_node) && reachable.contains(&e.to_node)
    });

    let nodes_removed = original_count - graph.nodes.len();
    let edges_removed = original_edges - graph.edges.len();

    OptimizationPass {
        name: "Dead Node Elimination".to_string(),
        description: if nodes_removed > 0 {
            format!("removed {} unreachable nodes", nodes_removed)
        } else {
            "no dead nodes found".to_string()
        },
        nodes_removed,
        edges_removed,
        nodes_fused: 0,
    }
}

/// Pass 2: Fuse consecutive transform nodes into one
fn transform_fusion(graph: &mut AnpGraph) -> OptimizationPass {
    let mut fused = 0;

    loop {
        let pair = find_fusible_transforms(graph);
        match pair {
            Some((first_id, second_id)) => {
                fuse_transforms(graph, &first_id, &second_id);
                fused += 1;
            }
            None => break,
        }
    }

    OptimizationPass {
        name: "Transform Fusion".to_string(),
        description: if fused > 0 {
            format!("fused {} transform pairs", fused)
        } else {
            "no fusible transforms".to_string()
        },
        nodes_removed: fused,
        edges_removed: fused,
        nodes_fused: fused,
    }
}

/// Find two consecutive transform nodes connected by a single default edge
fn find_fusible_transforms(graph: &AnpGraph) -> Option<(String, String)> {
    let transform_ids: HashSet<&str> = graph
        .nodes
        .iter()
        .filter(|n| n.node_type == "transform")
        .map(|n| n.id.as_str())
        .collect();

    for edge in &graph.edges {
        if edge.from_port == "default"
            && edge.to_port == "default"
            && transform_ids.contains(edge.from_node.as_str())
            && transform_ids.contains(edge.to_node.as_str())
        {
            // Check second transform has exactly one incoming edge
            let incoming_count = graph
                .edges
                .iter()
                .filter(|e| e.to_node == edge.to_node)
                .count();
            // Check first transform has exactly one outgoing edge
            let outgoing_count = graph
                .edges
                .iter()
                .filter(|e| e.from_node == edge.from_node)
                .count();

            if incoming_count == 1 && outgoing_count == 1 {
                return Some((edge.from_node.clone(), edge.to_node.clone()));
            }
        }
    }
    None
}

/// Merge second transform's config into first, remove second, rewire edges
fn fuse_transforms(graph: &mut AnpGraph, first_id: &str, second_id: &str) {
    // Merge configs: combine mapping objects
    let second_config = graph
        .nodes
        .iter()
        .find(|n| n.id == second_id)
        .map(|n| n.config.clone())
        .unwrap_or(serde_json::Value::Null);

    if let Some(first_node) = graph.nodes.iter_mut().find(|n| n.id == first_id) {
        // Merge mapping fields
        if let (Some(first_map), Some(second_map)) = (
            first_node
                .config
                .get("mapping")
                .and_then(|v| v.as_object())
                .cloned(),
            second_config
                .get("mapping")
                .and_then(|v| v.as_object())
                .cloned(),
        ) {
            let mut merged = first_map;
            for (k, v) in second_map {
                merged.insert(k, v);
            }
            first_node.config["mapping"] = serde_json::Value::Object(merged);
        }
    }

    // Rewire: edges FROM second_id now come FROM first_id
    let mut new_edges: Vec<Edge> = Vec::new();
    for edge in &graph.edges {
        if edge.from_node == second_id {
            new_edges.push(Edge {
                id: edge.id.clone(),
                from_node: first_id.to_string(),
                from_port: edge.from_port.clone(),
                to_node: edge.to_node.clone(),
                to_port: edge.to_port.clone(),
            });
        }
    }

    // Remove edge between first and second
    graph
        .edges
        .retain(|e| !(e.from_node == first_id && e.to_node == second_id));
    // Remove edges from second
    graph.edges.retain(|e| e.from_node != second_id);
    // Add rewired edges
    graph.edges.extend(new_edges);

    // Remove second node
    graph.nodes.retain(|n| n.id != second_id);
}

/// Pass 3: Remove edges that reference non-existent nodes
fn unused_edge_pruning(graph: &mut AnpGraph) -> OptimizationPass {
    let node_ids: HashSet<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    let original = graph.edges.len();

    graph.edges.retain(|e| {
        node_ids.contains(e.from_node.as_str()) && node_ids.contains(e.to_node.as_str())
    });

    let removed = original - graph.edges.len();

    OptimizationPass {
        name: "Unused Edge Pruning".to_string(),
        description: if removed > 0 {
            format!("removed {} orphaned edges", removed)
        } else {
            "no orphaned edges".to_string()
        },
        nodes_removed: 0,
        edges_removed: removed,
        nodes_fused: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn source(id: &str) -> Node {
        Node {
            id: id.to_string(),
            node_type: "file_read".to_string(),
            kind: NodeKind::Source,
            config: serde_json::json!({}),
            inputs: vec![],
            outputs: vec![PortSpec::new("default")],
        }
    }

    fn transform(id: &str, mapping: serde_json::Value) -> Node {
        Node {
            id: id.to_string(),
            node_type: "transform".to_string(),
            kind: NodeKind::Processor,
            config: serde_json::json!({ "type": "map", "mapping": mapping }),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
        }
    }

    fn sink(id: &str) -> Node {
        Node {
            id: id.to_string(),
            node_type: "console".to_string(),
            kind: NodeKind::Sink,
            config: serde_json::json!({}),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![],
        }
    }

    fn processor(id: &str, node_type: &str) -> Node {
        Node {
            id: id.to_string(),
            node_type: node_type.to_string(),
            kind: NodeKind::Processor,
            config: serde_json::json!({}),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
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
    fn test_no_optimization_needed() {
        let graph = make_graph(
            vec![source("s"), sink("k")],
            vec![edge("e1", "s", "k")],
        );
        let result = optimize(&graph);
        assert_eq!(result.optimized_nodes, 2);
        assert_eq!(result.optimized_edges, 1);
    }

    #[test]
    fn test_dead_node_elimination() {
        let graph = make_graph(
            vec![
                source("s"),
                processor("p", "filter"),
                sink("k"),
                processor("dead", "filter"), // not connected
            ],
            vec![edge("e1", "s", "p"), edge("e2", "p", "k")],
        );
        let result = optimize(&graph);
        assert_eq!(result.optimized_nodes, 3);
        assert_eq!(result.original_nodes, 4);
        assert!(result.passes_applied.iter().any(|p| p.name == "Dead Node Elimination"));
    }

    #[test]
    fn test_transform_fusion() {
        let graph = make_graph(
            vec![
                source("s"),
                transform("t1", serde_json::json!({"a": "input.x"})),
                transform("t2", serde_json::json!({"b": "input.y"})),
                sink("k"),
            ],
            vec![
                edge("e1", "s", "t1"),
                edge("e2", "t1", "t2"),
                edge("e3", "t2", "k"),
            ],
        );
        let result = optimize(&graph);
        assert_eq!(result.optimized_nodes, 3); // t1+t2 fused into one
        assert!(result.passes_applied.iter().any(|p| p.name == "Transform Fusion"));

        // Check fused transform has both mappings
        let fused = result.graph.nodes.iter().find(|n| n.node_type == "transform").unwrap();
        let mapping = fused.config.get("mapping").unwrap();
        assert!(mapping.get("a").is_some());
        assert!(mapping.get("b").is_some());
    }

    #[test]
    fn test_no_fusion_if_branching() {
        // t1 has two outgoing edges, so should not be fused
        let graph = make_graph(
            vec![
                source("s"),
                transform("t1", serde_json::json!({"a": "x"})),
                transform("t2", serde_json::json!({"b": "y"})),
                sink("k1"),
                sink("k2"),
            ],
            vec![
                edge("e1", "s", "t1"),
                edge("e2", "t1", "t2"),
                edge("e3", "t1", "k1"),
                edge("e4", "t2", "k2"),
            ],
        );
        let result = optimize(&graph);
        // t1 has 2 outgoing edges, no fusion
        assert_eq!(result.optimized_nodes, 5);
    }

    #[test]
    fn test_display() {
        let graph = make_graph(
            vec![source("s"), sink("k"), processor("dead", "filter")],
            vec![edge("e1", "s", "k")],
        );
        let result = optimize(&graph);
        let output = format!("{}", result);
        assert!(output.contains("ANP Graph Optimizer"));
        assert!(output.contains("Original:"));
        assert!(output.contains("Optimized:"));
    }

    #[test]
    fn test_multiple_dead_nodes() {
        let graph = make_graph(
            vec![
                source("s"),
                sink("k"),
                processor("d1", "filter"),
                processor("d2", "transform"),
                processor("d3", "template"),
            ],
            vec![edge("e1", "s", "k")],
        );
        let result = optimize(&graph);
        assert_eq!(result.optimized_nodes, 2);
        assert_eq!(result.original_nodes, 5);
    }

    #[test]
    fn test_chain_fusion() {
        // s -> t1 -> t2 -> t3 -> k  should fuse into s -> t_fused -> k
        let graph = make_graph(
            vec![
                source("s"),
                transform("t1", serde_json::json!({"a": "1"})),
                transform("t2", serde_json::json!({"b": "2"})),
                transform("t3", serde_json::json!({"c": "3"})),
                sink("k"),
            ],
            vec![
                edge("e1", "s", "t1"),
                edge("e2", "t1", "t2"),
                edge("e3", "t2", "t3"),
                edge("e4", "t3", "k"),
            ],
        );
        let result = optimize(&graph);
        // Should fuse pairs iteratively: t1+t2 -> fused, then fused+t3
        assert_eq!(result.optimized_nodes, 3); // s, fused_transform, k
    }
}
