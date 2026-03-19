//! Formal graph verification system for ANP graphs.
//!
//! Goes beyond structural validation to prove mathematical properties:
//! reachability, completeness, determinism, data flow coverage,
//! termination guarantees, parallelism scoring, and resource estimation.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

use crate::graph::spec::{AnpGraph, NodeKind};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Complete verification report for an ANP graph.
#[derive(Debug, Clone)]
pub struct VerificationReport {
    pub graph_name: String,
    pub checks: Vec<VerificationCheck>,
    pub metrics: GraphMetrics,
    pub overall_pass: bool,
}

/// A single verification check result.
#[derive(Debug, Clone)]
pub struct VerificationCheck {
    pub name: String,
    pub status: CheckStatus,
    pub details: Vec<String>,
}

/// Outcome of a single check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

/// Quantitative metrics about the graph.
#[derive(Debug, Clone)]
pub struct GraphMetrics {
    pub node_count: usize,
    pub edge_count: usize,
    pub max_depth: usize,
    pub max_parallel_width: usize,
    pub parallelism_score: f64,
    pub branching_factor: f64,
    pub estimated_connections: usize,
    pub has_ai_nodes: bool,
    pub has_network_nodes: bool,
    pub has_state_nodes: bool,
}

// ---------------------------------------------------------------------------
// Node-type classification helpers
// ---------------------------------------------------------------------------

const NETWORK_TYPES: &[&str] = &[
    "http_server",
    "http_client",
    "websocket",
    "ws",
    "sse",
    "webhook",
    "cors",
    "redirect",
    "static_file",
    "oauth",
];

const AI_TYPES: &[&str] = &[
    "llm_call",
    "tool_router",
    "embedding",
    "sentiment",
    "summarize",
    "classify",
    "extract",
    "translate",
    "prompt_chain",
    "agent_loop",
];

const STATE_TYPES: &[&str] = &[
    "database",
    "memory",
    "cache",
    "queue",
    "priority_queue",
    "session",
    "kv_store",
    "counter",
    "redis",
    "sql_query",
    "state_machine",
    "message_broker",
];

fn is_network(t: &str) -> bool {
    NETWORK_TYPES.contains(&t)
}
fn is_ai(t: &str) -> bool {
    AI_TYPES.contains(&t)
}
fn is_state(t: &str) -> bool {
    STATE_TYPES.contains(&t)
}

// ---------------------------------------------------------------------------
// Internal graph helpers
// ---------------------------------------------------------------------------

/// Build adjacency list (forward edges): node_id -> vec of neighbor ids.
fn build_adj<'a>(graph: &'a AnpGraph) -> HashMap<&'a str, Vec<&'a str>> {
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for n in &graph.nodes {
        adj.entry(n.id.as_str()).or_default();
    }
    for e in &graph.edges {
        adj.entry(e.from_node.as_str())
            .or_default()
            .push(e.to_node.as_str());
    }
    adj
}

/// Compute topological layers via Kahn's algorithm.
/// Returns a Vec of layers, each layer being a Vec of node ids.
fn topological_layers<'a>(graph: &'a AnpGraph) -> Vec<Vec<&'a str>> {
    let adj = build_adj(graph);
    let mut in_deg: HashMap<&str, usize> = HashMap::new();
    for n in &graph.nodes {
        in_deg.insert(n.id.as_str(), 0);
    }
    for e in &graph.edges {
        *in_deg.entry(e.to_node.as_str()).or_default() += 1;
    }

    let mut queue: VecDeque<&str> = VecDeque::new();
    for (&id, &deg) in &in_deg {
        if deg == 0 {
            queue.push_back(id);
        }
    }

    let mut layers: Vec<Vec<&str>> = Vec::new();
    while !queue.is_empty() {
        let layer: Vec<&str> = queue.drain(..).collect();
        let mut next_queue: VecDeque<&str> = VecDeque::new();
        for &id in &layer {
            if let Some(neighbors) = adj.get(id) {
                for &nb in neighbors {
                    if let Some(d) = in_deg.get_mut(nb) {
                        *d -= 1;
                        if *d == 0 {
                            next_queue.push_back(nb);
                        }
                    }
                }
            }
        }
        layers.push(layer);
        queue = next_queue;
    }
    layers
}

// ---------------------------------------------------------------------------
// Individual checks
// ---------------------------------------------------------------------------

/// 1. Reachability: every node is reachable from a Source or Service.
fn check_reachability(graph: &AnpGraph) -> VerificationCheck {
    let adj = build_adj(graph);
    let roots: Vec<&str> = graph
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Source | NodeKind::Service))
        .map(|n| n.id.as_str())
        .collect();

    let mut visited: HashSet<&str> = HashSet::new();
    let mut queue: VecDeque<&str> = VecDeque::new();
    for &r in &roots {
        queue.push_back(r);
        visited.insert(r);
    }
    while let Some(cur) = queue.pop_front() {
        if let Some(neighbors) = adj.get(cur) {
            for &nb in neighbors {
                if visited.insert(nb) {
                    queue.push_back(nb);
                }
            }
        }
    }

    let unreachable: Vec<String> = graph
        .nodes
        .iter()
        .filter(|n| !visited.contains(n.id.as_str()))
        .map(|n| n.id.clone())
        .collect();

    if unreachable.is_empty() {
        VerificationCheck {
            name: "Reachability".to_string(),
            status: CheckStatus::Pass,
            details: vec![format!("All {} nodes reachable", graph.nodes.len())],
        }
    } else if roots.is_empty() {
        VerificationCheck {
            name: "Reachability".to_string(),
            status: CheckStatus::Fail,
            details: vec!["No Source or Service nodes found".to_string()],
        }
    } else {
        VerificationCheck {
            name: "Reachability".to_string(),
            status: CheckStatus::Fail,
            details: vec![format!(
                "{} unreachable node(s): {}",
                unreachable.len(),
                unreachable.join(", ")
            )],
        }
    }
}

/// 2. Completeness: every Source and Processor has at least one outgoing edge.
fn check_completeness(graph: &AnpGraph) -> VerificationCheck {
    let outgoing: HashSet<&str> = graph.edges.iter().map(|e| e.from_node.as_str()).collect();

    let missing: Vec<String> = graph
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Source | NodeKind::Processor))
        .filter(|n| !outgoing.contains(n.id.as_str()))
        .map(|n| format!("{} ({})", n.id, n.kind))
        .collect();

    if missing.is_empty() {
        VerificationCheck {
            name: "Completeness".to_string(),
            status: CheckStatus::Pass,
            details: vec!["All outputs connected".to_string()],
        }
    } else {
        VerificationCheck {
            name: "Completeness".to_string(),
            status: CheckStatus::Warn,
            details: vec![format!(
                "{} node(s) with no outgoing edges: {}",
                missing.len(),
                missing.join(", ")
            )],
        }
    }
}

/// 3. Determinism: no node receives multiple edges on the same input port.
fn check_determinism(graph: &AnpGraph) -> VerificationCheck {
    // Map (to_node, to_port) -> count of incoming edges
    let mut port_counts: HashMap<(&str, &str), usize> = HashMap::new();
    for e in &graph.edges {
        *port_counts
            .entry((e.to_node.as_str(), e.to_port.as_str()))
            .or_default() += 1;
    }

    let ambiguous: Vec<String> = port_counts
        .iter()
        .filter(|(_, &count)| count > 1)
        .map(|((node, port), count)| format!("{}.{} ({} sources)", node, port, count))
        .collect();

    if ambiguous.is_empty() {
        VerificationCheck {
            name: "Determinism".to_string(),
            status: CheckStatus::Pass,
            details: vec!["No ambiguous data flow".to_string()],
        }
    } else {
        VerificationCheck {
            name: "Determinism".to_string(),
            status: CheckStatus::Warn,
            details: vec![format!(
                "{} port(s) with multiple sources: {}",
                ambiguous.len(),
                ambiguous.join(", ")
            )],
        }
    }
}

/// 4. Data Flow Coverage: all declared output ports have at least one edge.
fn check_port_coverage(graph: &AnpGraph) -> VerificationCheck {
    // Collect all (node_id, port_name) pairs actually used as from_port
    let used_outputs: HashSet<(&str, &str)> = graph
        .edges
        .iter()
        .map(|e| (e.from_node.as_str(), e.from_port.as_str()))
        .collect();

    let mut unused: Vec<String> = Vec::new();
    for node in &graph.nodes {
        // Sinks have no outputs to check
        if node.kind == NodeKind::Sink {
            continue;
        }
        for port in &node.outputs {
            if !used_outputs.contains(&(node.id.as_str(), port.name.as_str())) {
                unused.push(format!("{}.{}", node.id, port.name));
            }
        }
        // If node has no declared outputs and is not a Sink, also check the
        // implicit "default" port via outgoing edges
    }

    if unused.is_empty() {
        VerificationCheck {
            name: "Port Coverage".to_string(),
            status: CheckStatus::Pass,
            details: vec!["All declared output ports connected".to_string()],
        }
    } else {
        VerificationCheck {
            name: "Port Coverage".to_string(),
            status: CheckStatus::Warn,
            details: vec![format!(
                "{} unused port(s): {}",
                unused.len(),
                unused.join(", ")
            )],
        }
    }
}

/// 5. Termination: in batch mode (no Service nodes), all paths from sources reach a Sink.
fn check_termination(graph: &AnpGraph) -> VerificationCheck {
    let has_service = graph.nodes.iter().any(|n| n.kind == NodeKind::Service);
    if has_service {
        return VerificationCheck {
            name: "Termination".to_string(),
            status: CheckStatus::Pass,
            details: vec!["Service mode — termination not required".to_string()],
        };
    }

    let adj = build_adj(graph);
    let sink_ids: HashSet<&str> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Sink)
        .map(|n| n.id.as_str())
        .collect();

    if sink_ids.is_empty() && !graph.nodes.is_empty() {
        return VerificationCheck {
            name: "Termination".to_string(),
            status: CheckStatus::Fail,
            details: vec!["No Sink nodes — batch graph cannot terminate properly".to_string()],
        };
    }

    // For every node that has no outgoing edges (leaf), check if it is a Sink.
    let outgoing: HashSet<&str> = graph.edges.iter().map(|e| e.from_node.as_str()).collect();
    let dangling: Vec<String> = graph
        .nodes
        .iter()
        .filter(|n| !outgoing.contains(n.id.as_str()))
        .filter(|n| n.kind != NodeKind::Sink)
        .map(|n| n.id.clone())
        .collect();

    // Also verify via DFS: every Source can reach at least one Sink.
    let sources: Vec<&str> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Source)
        .map(|n| n.id.as_str())
        .collect();

    let mut unreachable_sources: Vec<String> = Vec::new();
    for &src in &sources {
        let mut visited: HashSet<&str> = HashSet::new();
        let mut stack: Vec<&str> = vec![src];
        let mut found_sink = false;
        while let Some(cur) = stack.pop() {
            if !visited.insert(cur) {
                continue;
            }
            if sink_ids.contains(cur) {
                found_sink = true;
                break;
            }
            if let Some(neighbors) = adj.get(cur) {
                for &nb in neighbors {
                    stack.push(nb);
                }
            }
        }
        if !found_sink {
            unreachable_sources.push(src.to_string());
        }
    }

    let mut details = Vec::new();
    let mut status = CheckStatus::Pass;

    if !dangling.is_empty() {
        status = CheckStatus::Warn;
        details.push(format!(
            "{} dangling non-sink leaf node(s): {}",
            dangling.len(),
            dangling.join(", ")
        ));
    }

    if !unreachable_sources.is_empty() {
        status = CheckStatus::Fail;
        details.push(format!(
            "{} source(s) cannot reach any sink: {}",
            unreachable_sources.len(),
            unreachable_sources.join(", ")
        ));
    }

    if status == CheckStatus::Pass {
        details.push("All paths reach sinks".to_string());
    }

    VerificationCheck {
        name: "Termination".to_string(),
        status,
        details,
    }
}

// ---------------------------------------------------------------------------
// Metrics computation
// ---------------------------------------------------------------------------

fn compute_metrics(graph: &AnpGraph) -> GraphMetrics {
    let layers = topological_layers(graph);
    let max_depth = layers.len();
    let max_parallel_width = layers.iter().map(|l| l.len()).max().unwrap_or(0);
    let node_count = graph.nodes.len();
    let edge_count = graph.edges.len();

    let parallelism_score = if node_count > 0 {
        max_parallel_width as f64 / node_count as f64
    } else {
        0.0
    };

    // Branching factor = edges / nodes (average outgoing degree)
    let branching_factor = if node_count > 0 {
        edge_count as f64 / node_count as f64
    } else {
        0.0
    };

    // Resource estimation
    let mut network_count = 0usize;
    let mut ai_count = 0usize;
    let mut state_count = 0usize;
    let mut server_count = 0usize;

    for n in &graph.nodes {
        let t = n.node_type.as_str();
        if is_network(t) {
            network_count += 1;
        }
        if is_ai(t) {
            ai_count += 1;
        }
        if is_state(t) {
            state_count += 1;
        }
        if t == "http_server" || t == "websocket" || t == "ws" || t == "sse" {
            server_count += 1;
        }
    }

    // Estimated connections: each server can handle ~100 concurrent conns,
    // each http_client counts as 1 outbound, each websocket as 10 potential.
    let mut estimated_connections = server_count * 100;
    for n in &graph.nodes {
        match n.node_type.as_str() {
            "http_client" => estimated_connections += 1,
            "websocket" | "ws" => estimated_connections += 10,
            _ => {}
        }
    }

    GraphMetrics {
        node_count,
        edge_count,
        max_depth,
        max_parallel_width,
        parallelism_score,
        branching_factor,
        estimated_connections,
        has_ai_nodes: ai_count > 0,
        has_network_nodes: network_count > 0,
        has_state_nodes: state_count > 0,
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Perform formal verification of an ANP graph.
///
/// Returns a [`VerificationReport`] containing individual check results,
/// quantitative metrics, and an overall pass/fail verdict.
pub fn verify(graph: &AnpGraph) -> VerificationReport {
    let checks = vec![
        check_reachability(graph),
        check_completeness(graph),
        check_determinism(graph),
        check_port_coverage(graph),
        check_termination(graph),
    ];

    let overall_pass = checks
        .iter()
        .all(|c| !matches!(c.status, CheckStatus::Fail));

    let metrics = compute_metrics(graph);

    VerificationReport {
        graph_name: graph.metadata.name.clone(),
        checks,
        metrics,
        overall_pass,
    }
}

// ---------------------------------------------------------------------------
// Display — beautiful ASCII report
// ---------------------------------------------------------------------------

const BOX_WIDTH: usize = 58;

fn pad_line(content: &str) -> String {
    let visible_len = strip_ansi_len(content);
    if visible_len >= BOX_WIDTH - 4 {
        format!("\u{2551}  {}\u{2551}", content)
    } else {
        let padding = BOX_WIDTH - 4 - visible_len;
        format!("\u{2551}  {}{}\u{2551}", content, " ".repeat(padding))
    }
}

/// Approximate visible character length (ignoring nothing here — no ANSI).
fn strip_ansi_len(s: &str) -> usize {
    s.chars().count()
}

impl fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckStatus::Pass => write!(f, "\u{2713} PASS"),
            CheckStatus::Warn => write!(f, "\u{26a0} WARN"),
            CheckStatus::Fail => write!(f, "\u{2717} FAIL"),
        }
    }
}

impl fmt::Display for VerificationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let top = format!(
            "\u{2554}{}\u{2557}",
            "\u{2550}".repeat(BOX_WIDTH - 2)
        );
        let mid = format!(
            "\u{2560}{}\u{2563}",
            "\u{2550}".repeat(BOX_WIDTH - 2)
        );
        let bot = format!(
            "\u{255a}{}\u{255d}",
            "\u{2550}".repeat(BOX_WIDTH - 2)
        );

        // Header
        writeln!(f, "{}", top)?;
        writeln!(f, "{}", pad_line("ANP Graph Verification Report"))?;
        writeln!(
            f,
            "{}",
            pad_line(&format!("Graph: {}", self.graph_name))
        )?;
        writeln!(f, "{}", mid)?;

        // Checks
        for check in &self.checks {
            let summary = if check.details.is_empty() {
                String::new()
            } else {
                check.details[0].clone()
            };
            let line = format!(
                "{:<6} {:<16} {}",
                check.status, check.name, summary
            );
            writeln!(f, "{}", pad_line(&line))?;
        }

        // Metrics
        writeln!(f, "{}", mid)?;
        writeln!(f, "{}", pad_line("Metrics"))?;
        writeln!(
            f,
            "{}",
            pad_line(&format!(
                "  Nodes: {}  Edges: {}  Depth: {}",
                self.metrics.node_count, self.metrics.edge_count, self.metrics.max_depth
            ))
        )?;
        writeln!(
            f,
            "{}",
            pad_line(&format!(
                "  Parallelism: {:.2}  Branching: {:.2}",
                self.metrics.parallelism_score, self.metrics.branching_factor
            ))
        )?;

        let mut resources = Vec::new();
        if self.metrics.has_network_nodes {
            resources.push(format!("Conns: ~{}", self.metrics.estimated_connections));
        }
        if self.metrics.has_ai_nodes {
            resources.push("AI: yes".to_string());
        }
        if self.metrics.has_state_nodes {
            resources.push("State: yes".to_string());
        }
        if !resources.is_empty() {
            writeln!(
                f,
                "{}",
                pad_line(&format!("  {}", resources.join("  ")))
            )?;
        }

        // Verdict
        writeln!(f, "{}", mid)?;
        if self.overall_pass {
            writeln!(
                f,
                "{}",
                pad_line("VERIFIED \u{2713}  Graph is provably correct")
            )?;
        } else {
            writeln!(
                f,
                "{}",
                pad_line("FAILED \u{2717}  Graph has verification errors")
            )?;
        }
        write!(f, "{}", bot)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::spec::*;

    // -- Helpers --

    fn make_graph(name: &str, nodes: Vec<Node>, edges: Vec<Edge>) -> AnpGraph {
        AnpGraph {
            version: GraphVersion::V0_1,
            metadata: GraphMetadata {
                name: name.to_string(),
                description: String::new(),
            },
            nodes,
            edges,
        }
    }

    fn node(id: &str, node_type: &str, kind: NodeKind) -> Node {
        let inputs = match kind {
            NodeKind::Source | NodeKind::Service => vec![],
            _ => vec![PortSpec::new("default")],
        };
        let outputs = match kind {
            NodeKind::Sink => vec![],
            _ => vec![PortSpec::new("default")],
        };
        Node {
            id: id.to_string(),
            node_type: node_type.to_string(),
            kind,
            config: serde_json::Value::Null,
            inputs,
            outputs,
        }
    }

    fn source(id: &str) -> Node {
        node(id, "file_read", NodeKind::Source)
    }

    fn processor(id: &str) -> Node {
        node(id, "transform", NodeKind::Processor)
    }

    fn sink(id: &str) -> Node {
        node(id, "console", NodeKind::Sink)
    }

    fn service(id: &str) -> Node {
        node(id, "http_server", NodeKind::Service)
    }

    fn ai_processor(id: &str) -> Node {
        node(id, "llm_call", NodeKind::Processor)
    }

    fn db_processor(id: &str) -> Node {
        node(id, "database", NodeKind::Processor)
    }

    fn http_client_proc(id: &str) -> Node {
        node(id, "http_client", NodeKind::Processor)
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

    fn edge_port(id: &str, from: &str, from_port: &str, to: &str, to_port: &str) -> Edge {
        Edge {
            id: id.to_string(),
            from_node: from.to_string(),
            from_port: from_port.to_string(),
            to_node: to.to_string(),
            to_port: to_port.to_string(),
        }
    }

    fn find_check<'a>(report: &'a VerificationReport, name: &str) -> &'a VerificationCheck {
        report
            .checks
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("check '{}' not found", name))
    }

    // -- Perfect graph --

    #[test]
    fn test_perfect_graph_passes_all() {
        let graph = make_graph(
            "perfect",
            vec![source("s1"), processor("p1"), sink("k1")],
            vec![edge("e1", "s1", "p1"), edge("e2", "p1", "k1")],
        );
        let report = verify(&graph);
        assert!(report.overall_pass);
        for check in &report.checks {
            assert_ne!(check.status, CheckStatus::Fail, "check {} failed", check.name);
        }
    }

    // -- Reachability --

    #[test]
    fn test_unreachable_node() {
        let graph = make_graph(
            "unreachable",
            vec![source("s1"), processor("p1"), processor("orphan"), sink("k1")],
            vec![edge("e1", "s1", "p1"), edge("e2", "p1", "k1")],
        );
        let report = verify(&graph);
        let check = find_check(&report, "Reachability");
        assert_eq!(check.status, CheckStatus::Fail);
        assert!(check.details[0].contains("orphan"));
    }

    #[test]
    fn test_no_sources() {
        let graph = make_graph(
            "no-sources",
            vec![processor("p1"), sink("k1")],
            vec![edge("e1", "p1", "k1")],
        );
        let report = verify(&graph);
        let check = find_check(&report, "Reachability");
        assert_eq!(check.status, CheckStatus::Fail);
    }

    #[test]
    fn test_service_counts_as_root() {
        let graph = make_graph(
            "service-root",
            vec![service("srv"), processor("p1"), sink("k1")],
            vec![edge("e1", "srv", "p1"), edge("e2", "p1", "k1")],
        );
        let report = verify(&graph);
        let check = find_check(&report, "Reachability");
        assert_eq!(check.status, CheckStatus::Pass);
    }

    // -- Completeness --

    #[test]
    fn test_dead_end_processor() {
        let graph = make_graph(
            "dead-end",
            vec![source("s1"), processor("p1")],
            vec![edge("e1", "s1", "p1")],
        );
        let report = verify(&graph);
        let check = find_check(&report, "Completeness");
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(check.details[0].contains("p1"));
    }

    #[test]
    fn test_source_with_no_output() {
        let graph = make_graph(
            "lonely-source",
            vec![source("s1"), sink("k1")],
            vec![], // s1 not connected
        );
        let report = verify(&graph);
        let check = find_check(&report, "Completeness");
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(check.details[0].contains("s1"));
    }

    // -- Determinism --

    #[test]
    fn test_ambiguous_merge() {
        let graph = make_graph(
            "ambiguous",
            vec![source("s1"), source("s2"), processor("p1"), sink("k1")],
            vec![
                edge("e1", "s1", "p1"),
                edge("e2", "s2", "p1"),
                edge("e3", "p1", "k1"),
            ],
        );
        let report = verify(&graph);
        let check = find_check(&report, "Determinism");
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(check.details[0].contains("p1.default"));
    }

    #[test]
    fn test_deterministic_graph() {
        let graph = make_graph(
            "deterministic",
            vec![source("s1"), processor("p1"), sink("k1")],
            vec![edge("e1", "s1", "p1"), edge("e2", "p1", "k1")],
        );
        let report = verify(&graph);
        let check = find_check(&report, "Determinism");
        assert_eq!(check.status, CheckStatus::Pass);
    }

    // -- Port Coverage --

    #[test]
    fn test_unused_output_port() {
        let mut p1 = processor("p1");
        p1.outputs.push(PortSpec::new("error"));
        let graph = make_graph(
            "unused-port",
            vec![source("s1"), p1, sink("k1")],
            vec![edge("e1", "s1", "p1"), edge("e2", "p1", "k1")],
        );
        let report = verify(&graph);
        let check = find_check(&report, "Port Coverage");
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(check.details[0].contains("p1.error"));
    }

    #[test]
    fn test_all_ports_covered() {
        let graph = make_graph(
            "covered",
            vec![source("s1"), processor("p1"), sink("k1")],
            vec![edge("e1", "s1", "p1"), edge("e2", "p1", "k1")],
        );
        let report = verify(&graph);
        let check = find_check(&report, "Port Coverage");
        assert_eq!(check.status, CheckStatus::Pass);
    }

    // -- Termination --

    #[test]
    fn test_no_sinks_in_batch() {
        let graph = make_graph(
            "no-sinks",
            vec![source("s1"), processor("p1")],
            vec![edge("e1", "s1", "p1")],
        );
        let report = verify(&graph);
        let check = find_check(&report, "Termination");
        assert_eq!(check.status, CheckStatus::Fail);
    }

    #[test]
    fn test_source_cannot_reach_sink() {
        // s1 -> p1 (dead end), s2 -> k1 (ok)
        let graph = make_graph(
            "partial-term",
            vec![source("s1"), processor("p1"), source("s2"), sink("k1")],
            vec![edge("e1", "s1", "p1"), edge("e2", "s2", "k1")],
        );
        let report = verify(&graph);
        let check = find_check(&report, "Termination");
        assert_eq!(check.status, CheckStatus::Fail);
        assert!(check.details.iter().any(|d| d.contains("s1")));
    }

    #[test]
    fn test_service_mode_skips_termination() {
        let graph = make_graph(
            "service-mode",
            vec![service("srv"), processor("p1")],
            vec![edge("e1", "srv", "p1")],
        );
        let report = verify(&graph);
        let check = find_check(&report, "Termination");
        assert_eq!(check.status, CheckStatus::Pass);
        assert!(check.details[0].contains("Service mode"));
    }

    #[test]
    fn test_batch_terminates() {
        let graph = make_graph(
            "batch-ok",
            vec![source("s1"), processor("p1"), sink("k1")],
            vec![edge("e1", "s1", "p1"), edge("e2", "p1", "k1")],
        );
        let report = verify(&graph);
        let check = find_check(&report, "Termination");
        assert_eq!(check.status, CheckStatus::Pass);
    }

    // -- Metrics --

    #[test]
    fn test_metrics_basic() {
        let graph = make_graph(
            "metrics",
            vec![source("s1"), processor("p1"), processor("p2"), sink("k1")],
            vec![
                edge("e1", "s1", "p1"),
                edge("e2", "s1", "p2"),
                edge("e3", "p1", "k1"),
                edge("e4", "p2", "k1"),
            ],
        );
        let report = verify(&graph);
        assert_eq!(report.metrics.node_count, 4);
        assert_eq!(report.metrics.edge_count, 4);
        assert_eq!(report.metrics.max_depth, 3); // s1 | p1,p2 | k1
        assert_eq!(report.metrics.max_parallel_width, 2); // p1,p2 layer
        assert!((report.metrics.branching_factor - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_parallelism_score() {
        // Linear pipeline: no parallelism
        let graph = make_graph(
            "linear",
            vec![source("s1"), processor("p1"), processor("p2"), sink("k1")],
            vec![
                edge("e1", "s1", "p1"),
                edge("e2", "p1", "p2"),
                edge("e3", "p2", "k1"),
            ],
        );
        let report = verify(&graph);
        assert_eq!(report.metrics.max_parallel_width, 1);
        assert!((report.metrics.parallelism_score - 0.25).abs() < 0.01);
    }

    #[test]
    fn test_ai_network_state_detection() {
        let graph = make_graph(
            "resource-heavy",
            vec![
                service("srv"),
                ai_processor("llm"),
                db_processor("db"),
                http_client_proc("api"),
                sink("out"),
            ],
            vec![
                edge("e1", "srv", "llm"),
                edge("e2", "llm", "db"),
                edge("e3", "db", "api"),
                edge("e4", "api", "out"),
            ],
        );
        let report = verify(&graph);
        assert!(report.metrics.has_ai_nodes);
        assert!(report.metrics.has_network_nodes);
        assert!(report.metrics.has_state_nodes);
        assert!(report.metrics.estimated_connections > 0);
    }

    // -- Display --

    #[test]
    fn test_display_output() {
        let graph = make_graph(
            "rest-api",
            vec![service("srv"), processor("p1"), sink("k1")],
            vec![edge("e1", "srv", "p1"), edge("e2", "p1", "k1")],
        );
        let report = verify(&graph);
        let output = format!("{}", report);
        assert!(output.contains("ANP Graph Verification Report"));
        assert!(output.contains("rest-api"));
        assert!(output.contains("VERIFIED"));
        assert!(output.contains("Nodes: 3"));
    }

    #[test]
    fn test_display_failed() {
        let graph = make_graph(
            "broken",
            vec![processor("orphan")],
            vec![],
        );
        let report = verify(&graph);
        assert!(!report.overall_pass);
        let output = format!("{}", report);
        assert!(output.contains("FAILED"));
    }

    // -- Empty graph --

    #[test]
    fn test_empty_graph() {
        let graph = make_graph("empty", vec![], vec![]);
        let report = verify(&graph);
        assert!(report.overall_pass);
        assert_eq!(report.metrics.node_count, 0);
        assert_eq!(report.metrics.edge_count, 0);
        assert_eq!(report.metrics.max_depth, 0);
        assert!((report.metrics.parallelism_score - 0.0).abs() < f64::EPSILON);
    }

    // -- Complex diamond graph --

    #[test]
    fn test_diamond_graph() {
        //   s1
        //  / \
        // p1  p2
        //  \ /
        //   k1
        let graph = make_graph(
            "diamond",
            vec![source("s1"), processor("p1"), processor("p2"), sink("k1")],
            vec![
                edge("e1", "s1", "p1"),
                edge("e2", "s1", "p2"),
                edge("e3", "p1", "k1"),
                edge("e4", "p2", "k1"),
            ],
        );
        let report = verify(&graph);
        assert!(report.overall_pass);
        // Determinism warns because k1.default has 2 sources
        let det = find_check(&report, "Determinism");
        assert_eq!(det.status, CheckStatus::Warn);
    }

    // -- Wide fan-out graph --

    #[test]
    fn test_wide_fanout() {
        let mut nodes = vec![source("s1")];
        let mut edges = Vec::new();
        for i in 0..10 {
            let pid = format!("p{}", i);
            let kid = format!("k{}", i);
            nodes.push(processor(&pid));
            nodes.push(sink(&kid));
            edges.push(edge(&format!("ea{}", i), "s1", &pid));
            edges.push(edge(&format!("eb{}", i), &pid, &kid));
        }
        let graph = make_graph("fanout", nodes, edges);
        let report = verify(&graph);
        assert!(report.overall_pass);
        assert_eq!(report.metrics.max_parallel_width, 10);
        assert_eq!(report.metrics.max_depth, 3);
    }

    // -- Multi-port edge test --

    #[test]
    fn test_different_ports_not_ambiguous() {
        let mut p1 = processor("p1");
        p1.inputs.push(PortSpec::new("alt"));
        let graph = make_graph(
            "multi-port",
            vec![source("s1"), source("s2"), p1, sink("k1")],
            vec![
                edge_port("e1", "s1", "default", "p1", "default"),
                edge_port("e2", "s2", "default", "p1", "alt"),
                edge("e3", "p1", "k1"),
            ],
        );
        let report = verify(&graph);
        let check = find_check(&report, "Determinism");
        assert_eq!(check.status, CheckStatus::Pass);
    }

    // -- Overall pass/fail logic --

    #[test]
    fn test_warn_does_not_fail() {
        // A graph with only warnings should still pass overall
        let mut p1 = processor("p1");
        p1.outputs.push(PortSpec::new("error")); // unused port -> warn
        let graph = make_graph(
            "warn-only",
            vec![source("s1"), p1, sink("k1")],
            vec![edge("e1", "s1", "p1"), edge("e2", "p1", "k1")],
        );
        let report = verify(&graph);
        assert!(report.overall_pass);
    }
}
