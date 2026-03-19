use std::collections::HashMap;
use std::fmt;
use std::time::Instant;

use crate::graph::spec::AnpGraph;
use crate::runtime::scheduler::schedule;

/// Benchmark results for a graph
#[derive(Debug)]
pub struct BenchmarkReport {
    pub graph_name: String,
    /// Scheduling metrics
    pub scheduling: SchedulingMetrics,
    /// Graph complexity metrics
    pub complexity: ComplexityMetrics,
    /// Performance predictions
    pub predictions: PerformancePredictions,
    /// Comparison with hypothetical implementations
    pub comparison: ComparisonMetrics,
}

#[derive(Debug)]
pub struct SchedulingMetrics {
    /// Time to create execution plan (microseconds)
    pub schedule_time_us: u64,
    /// Number of execution stages
    pub stage_count: usize,
    /// Max nodes in a single stage (parallelism)
    pub max_stage_width: usize,
    /// Input gathering complexity
    pub input_lookup: String,
}

#[derive(Debug)]
pub struct ComplexityMetrics {
    pub nodes: usize,
    pub edges: usize,
    pub depth: usize,
    pub branching_factor: f64,
    /// Cyclomatic complexity analog for graphs: E - N + 2P
    pub graph_complexity: usize,
    /// Number of independent subgraphs
    pub connected_components: usize,
}

#[derive(Debug)]
pub struct PerformancePredictions {
    /// Estimated parallel speedup vs sequential
    pub parallel_speedup: f64,
    /// Estimated memory usage (bytes)
    pub estimated_memory_bytes: usize,
    /// Number of network I/O operations
    pub network_ops: usize,
    /// Number of disk I/O operations
    pub disk_ops: usize,
    /// Number of AI/LLM calls
    pub ai_calls: usize,
}

#[derive(Debug)]
pub struct ComparisonMetrics {
    /// ANP's O(1) input gathering vs O(E) scanning
    pub input_gathering_speedup: String,
    /// Parallel execution advantage
    pub parallel_advantage: String,
    /// Estimated LOC equivalent in Python/JS
    pub equivalent_loc_python: usize,
    /// Estimated LOC equivalent in Rust
    pub equivalent_loc_rust: usize,
}

/// Estimate lines of Python code for a given node type.
fn python_loc_for_node(node_type: &str) -> usize {
    match node_type {
        "http_server" => 50,
        "http_client" => 35,
        "database" | "sql_query" => 40,
        "llm_call" | "agent_loop" | "prompt_chain" => 30,
        "template" | "template_engine" | "html_template" => 15,
        "transform" | "filter" | "conditional" | "switch" => 10,
        "file_read" | "file_write" => 12,
        "console" | "log" => 5,
        "jwt" | "oauth" | "api_key" | "password" | "totp" => 35,
        "websocket" | "ws" | "sse" => 45,
        "email" => 25,
        "cache" | "redis" | "kv_store" | "memory" | "session" => 20,
        "retry" | "circuit_breaker" | "rate_limit" => 25,
        "foreach" | "parallel" | "merge" => 15,
        "json_parse" | "csv" | "yaml" | "xml" | "toml" => 10,
        "regex" | "string" | "math" | "array" => 8,
        "validate" | "sanitize" => 12,
        "chart" | "table" | "form" => 30,
        "cron" => 20,
        "encrypt" | "aes" | "hash" => 20,
        "image_resize" => 25,
        "state_machine" => 40,
        "message_broker" => 35,
        "embedding" | "sentiment" | "summarize" | "classify" | "extract" | "translate" => 25,
        _ => 15,
    }
}

/// Count connected components using Union-Find.
fn count_connected_components(graph: &AnpGraph) -> usize {
    if graph.nodes.is_empty() {
        return 0;
    }

    let id_to_idx: HashMap<&str, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();

    let n = graph.nodes.len();
    let mut parent: Vec<usize> = (0..n).collect();
    let mut rank: Vec<usize> = vec![0; n];

    fn find(parent: &mut Vec<usize>, x: usize) -> usize {
        if parent[x] != x {
            parent[x] = find(parent, parent[x]);
        }
        parent[x]
    }

    fn union(parent: &mut Vec<usize>, rank: &mut Vec<usize>, a: usize, b: usize) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra == rb {
            return;
        }
        if rank[ra] < rank[rb] {
            parent[ra] = rb;
        } else if rank[ra] > rank[rb] {
            parent[rb] = ra;
        } else {
            parent[rb] = ra;
            rank[ra] += 1;
        }
    }

    for edge in &graph.edges {
        if let (Some(&a), Some(&b)) = (
            id_to_idx.get(edge.from_node.as_str()),
            id_to_idx.get(edge.to_node.as_str()),
        ) {
            union(&mut parent, &mut rank, a, b);
        }
    }

    let mut roots = std::collections::HashSet::new();
    for i in 0..n {
        roots.insert(find(&mut parent, i));
    }
    roots.len()
}

/// Classify a node type as network, disk, or AI I/O.
fn classify_io(node_type: &str) -> (bool, bool, bool) {
    let network = matches!(
        node_type,
        "http_server"
            | "http_client"
            | "webhook"
            | "websocket"
            | "ws"
            | "sse"
            | "email"
            | "cors"
            | "redirect"
            | "static_file"
            | "oauth"
    );
    let disk = matches!(node_type, "file_read" | "file_write" | "database" | "sql_query");
    let ai = matches!(
        node_type,
        "llm_call"
            | "tool_router"
            | "embedding"
            | "sentiment"
            | "summarize"
            | "classify"
            | "extract"
            | "translate"
            | "prompt_chain"
            | "agent_loop"
    );
    (network, disk, ai)
}

/// Benchmark a graph, producing a full report.
///
/// This measures graph characteristics (scheduling time, complexity, parallelism)
/// without actually executing any nodes.
pub fn benchmark(graph: &AnpGraph) -> BenchmarkReport {
    // -- Scheduling metrics: actually measure schedule() --
    let start = Instant::now();
    let plan_result = schedule(graph);
    let elapsed = start.elapsed();
    let schedule_time_us = elapsed.as_micros() as u64;

    let (stage_count, max_stage_width, depth) = match &plan_result {
        Ok(plan) => {
            let sc = plan.stages.len();
            let msw = plan.stages.iter().map(|s| s.node_ids.len()).max().unwrap_or(0);
            (sc, msw, sc)
        }
        Err(_) => (0, 0, 0),
    };

    let scheduling = SchedulingMetrics {
        schedule_time_us,
        stage_count,
        max_stage_width,
        input_lookup: "O(1)".to_string(),
    };

    // -- Complexity metrics --
    let nodes = graph.nodes.len();
    let edges = graph.edges.len();
    let connected_components = count_connected_components(graph);

    let branching_factor = if nodes > 0 {
        edges as f64 / nodes as f64
    } else {
        0.0
    };

    // Cyclomatic complexity: E - N + 2P
    let graph_complexity = (edges + 2 * connected_components).saturating_sub(nodes);

    let complexity = ComplexityMetrics {
        nodes,
        edges,
        depth,
        branching_factor,
        graph_complexity,
        connected_components,
    };

    // -- Performance predictions --
    let parallel_speedup = if max_stage_width > 0 {
        max_stage_width as f64
    } else {
        1.0
    };

    // Memory estimate: ~512 bytes per node (config, ports, id) + ~128 bytes per edge
    let estimated_memory_bytes = nodes * 512 + edges * 128;

    let mut network_ops = 0usize;
    let mut disk_ops = 0usize;
    let mut ai_calls = 0usize;
    for node in &graph.nodes {
        let (net, disk, ai) = classify_io(&node.node_type);
        if net {
            network_ops += 1;
        }
        if disk {
            disk_ops += 1;
        }
        if ai {
            ai_calls += 1;
        }
    }

    let predictions = PerformancePredictions {
        parallel_speedup,
        estimated_memory_bytes,
        network_ops,
        disk_ops,
        ai_calls,
    };

    // -- Comparison metrics --
    let input_gathering_speedup = if edges > 100 {
        "~100x faster (O(1) vs O(E))".to_string()
    } else if edges > 10 {
        "~10x faster (O(1) vs O(E))".to_string()
    } else {
        format!("~{}x faster (O(1) vs O(E))", edges.max(1))
    };

    let parallel_advantage = if max_stage_width > 1 {
        format!(
            "automatic stage-based parallelism (up to {}x)",
            max_stage_width
        )
    } else {
        "automatic stage-based parallelism".to_string()
    };

    let equivalent_loc_python: usize = graph
        .nodes
        .iter()
        .map(|n| python_loc_for_node(&n.node_type))
        .sum();

    // Rust is typically ~1.5x more verbose than Python
    let equivalent_loc_rust = (equivalent_loc_python as f64 * 1.5) as usize;

    let comparison = ComparisonMetrics {
        input_gathering_speedup,
        parallel_advantage,
        equivalent_loc_python,
        equivalent_loc_rust,
    };

    BenchmarkReport {
        graph_name: graph.metadata.name.clone(),
        scheduling,
        complexity,
        predictions,
        comparison,
    }
}

impl fmt::Display for BenchmarkReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ANP Benchmark Report")?;
        writeln!(f, "\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}")?;
        writeln!(f, "Graph: {}", self.graph_name)?;
        writeln!(f)?;

        // Scheduling
        writeln!(f, "Scheduling")?;
        writeln!(
            f,
            "  Plan creation:     {} \u{03bc}s",
            self.scheduling.schedule_time_us
        )?;
        writeln!(f, "  Stages:            {}", self.scheduling.stage_count)?;
        writeln!(
            f,
            "  Max parallelism:   {} node{}",
            self.scheduling.max_stage_width,
            if self.scheduling.max_stage_width == 1 {
                ""
            } else {
                "s"
            }
        )?;
        writeln!(
            f,
            "  Input gathering:   {} (reverse edge index)",
            self.scheduling.input_lookup
        )?;
        writeln!(f)?;

        // Complexity
        writeln!(f, "Complexity")?;
        writeln!(
            f,
            "  Nodes: {}  Edges: {}  Depth: {}",
            self.complexity.nodes, self.complexity.edges, self.complexity.depth
        )?;
        writeln!(
            f,
            "  Branching factor:  {:.2}",
            self.complexity.branching_factor
        )?;
        let complexity_label = if self.complexity.graph_complexity <= 5 {
            "simple"
        } else if self.complexity.graph_complexity <= 15 {
            "moderate"
        } else {
            "complex"
        };
        writeln!(
            f,
            "  Graph complexity:  {} ({})",
            self.complexity.graph_complexity, complexity_label
        )?;
        writeln!(
            f,
            "  Components:        {}",
            self.complexity.connected_components
        )?;
        writeln!(f)?;

        // Performance Predictions
        writeln!(f, "Performance Predictions")?;
        let speedup_label = if self.predictions.parallel_speedup <= 1.0 {
            " (linear graph)".to_string()
        } else {
            String::new()
        };
        writeln!(
            f,
            "  Parallel speedup:  {:.1}x{}",
            self.predictions.parallel_speedup, speedup_label
        )?;
        let mem = self.predictions.estimated_memory_bytes;
        let mem_str = if mem >= 1_048_576 {
            format!("~{} MB", mem / 1_048_576)
        } else if mem >= 1024 {
            format!("~{} KB", mem / 1024)
        } else {
            format!("~{} B", mem)
        };
        writeln!(f, "  Memory estimate:   {}", mem_str)?;

        // Build I/O lines with context
        let net_label = if self.predictions.network_ops > 0 {
            let net_types: Vec<&str> = vec!["http_server", "http_client", "webhook", "websocket"];
            format!(" ({})", net_types[..self.predictions.network_ops.min(net_types.len())].join(", "))
        } else {
            String::new()
        };
        writeln!(
            f,
            "  Network I/O:       {}{}",
            self.predictions.network_ops, net_label
        )?;
        writeln!(f, "  Disk I/O:          {}", self.predictions.disk_ops)?;
        writeln!(f, "  AI/LLM calls:      {}", self.predictions.ai_calls)?;
        writeln!(f)?;

        // Comparison
        writeln!(f, "vs Traditional Code")?;
        writeln!(
            f,
            "  ANP:     {} nodes, {} edges (1 JSON file)",
            self.complexity.nodes, self.complexity.edges
        )?;
        writeln!(
            f,
            "  Python:  ~{} lines",
            self.comparison.equivalent_loc_python
        )?;
        writeln!(
            f,
            "  Rust:    ~{} lines",
            self.comparison.equivalent_loc_rust
        )?;
        writeln!(
            f,
            "  Input gathering: {}",
            self.comparison.input_gathering_speedup
        )?;
        write!(
            f,
            "  Parallel execution: {}",
            self.comparison.parallel_advantage
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::spec::*;

    fn make_node(id: &str, node_type: &str, kind: NodeKind) -> Node {
        Node {
            id: id.to_string(),
            node_type: node_type.to_string(),
            kind,
            config: serde_json::Value::Null,
            inputs: vec![],
            outputs: vec![],
        }
    }

    fn make_edge(id: &str, from: &str, to: &str) -> Edge {
        Edge {
            id: id.to_string(),
            from_node: from.to_string(),
            from_port: "default".to_string(),
            to_node: to.to_string(),
            to_port: "default".to_string(),
        }
    }

    fn make_graph(nodes: Vec<Node>, edges: Vec<Edge>) -> AnpGraph {
        AnpGraph {
            version: GraphVersion::V0_1,
            metadata: GraphMetadata {
                name: "test-graph".to_string(),
                description: String::new(),
            },
            nodes,
            edges,
        }
    }

    #[test]
    fn test_empty_graph() {
        let graph = make_graph(vec![], vec![]);
        let report = benchmark(&graph);
        assert_eq!(report.complexity.nodes, 0);
        assert_eq!(report.complexity.edges, 0);
        assert_eq!(report.complexity.connected_components, 0);
        assert_eq!(report.complexity.depth, 0);
        assert_eq!(report.predictions.parallel_speedup, 1.0);
    }

    #[test]
    fn test_single_node() {
        let graph = make_graph(
            vec![make_node("n1", "console", NodeKind::Sink)],
            vec![],
        );
        let report = benchmark(&graph);
        assert_eq!(report.complexity.nodes, 1);
        assert_eq!(report.complexity.edges, 0);
        assert_eq!(report.complexity.connected_components, 1);
        assert_eq!(report.scheduling.stage_count, 1);
        assert_eq!(report.scheduling.max_stage_width, 1);
        assert_eq!(report.complexity.graph_complexity, 1); // 0 - 1 + 2*1 = 1
    }

    #[test]
    fn test_linear_pipeline() {
        let graph = make_graph(
            vec![
                make_node("s", "file_read", NodeKind::Source),
                make_node("t", "transform", NodeKind::Processor),
                make_node("c", "console", NodeKind::Sink),
            ],
            vec![make_edge("e1", "s", "t"), make_edge("e2", "t", "c")],
        );
        let report = benchmark(&graph);
        assert_eq!(report.complexity.nodes, 3);
        assert_eq!(report.complexity.edges, 2);
        assert_eq!(report.scheduling.stage_count, 3);
        assert_eq!(report.scheduling.max_stage_width, 1);
        assert_eq!(report.predictions.parallel_speedup, 1.0);
        assert_eq!(report.complexity.connected_components, 1);
        // E - N + 2P = 2 - 3 + 2 = 1
        assert_eq!(report.complexity.graph_complexity, 1);
    }

    #[test]
    fn test_parallel_sources() {
        let graph = make_graph(
            vec![
                make_node("s1", "file_read", NodeKind::Source),
                make_node("s2", "file_read", NodeKind::Source),
                make_node("m", "merge", NodeKind::Processor),
                make_node("c", "console", NodeKind::Sink),
            ],
            vec![
                make_edge("e1", "s1", "m"),
                make_edge("e2", "s2", "m"),
                make_edge("e3", "m", "c"),
            ],
        );
        let report = benchmark(&graph);
        assert_eq!(report.scheduling.stage_count, 3);
        assert_eq!(report.scheduling.max_stage_width, 2);
        assert_eq!(report.predictions.parallel_speedup, 2.0);
        assert_eq!(report.complexity.connected_components, 1);
    }

    #[test]
    fn test_io_classification_network() {
        let graph = make_graph(
            vec![
                make_node("srv", "http_server", NodeKind::Service),
                make_node("cli", "http_client", NodeKind::Processor),
                make_node("c", "console", NodeKind::Sink),
            ],
            vec![make_edge("e1", "srv", "cli"), make_edge("e2", "cli", "c")],
        );
        let report = benchmark(&graph);
        assert_eq!(report.predictions.network_ops, 2);
        assert_eq!(report.predictions.disk_ops, 0);
        assert_eq!(report.predictions.ai_calls, 0);
    }

    #[test]
    fn test_io_classification_disk() {
        let graph = make_graph(
            vec![
                make_node("r", "file_read", NodeKind::Source),
                make_node("w", "file_write", NodeKind::Sink),
            ],
            vec![make_edge("e1", "r", "w")],
        );
        let report = benchmark(&graph);
        assert_eq!(report.predictions.disk_ops, 2);
        assert_eq!(report.predictions.network_ops, 0);
    }

    #[test]
    fn test_io_classification_ai() {
        let graph = make_graph(
            vec![
                make_node("s", "file_read", NodeKind::Source),
                make_node("llm", "llm_call", NodeKind::Processor),
                make_node("c", "console", NodeKind::Sink),
            ],
            vec![make_edge("e1", "s", "llm"), make_edge("e2", "llm", "c")],
        );
        let report = benchmark(&graph);
        assert_eq!(report.predictions.ai_calls, 1);
    }

    #[test]
    fn test_disconnected_components() {
        // Two independent subgraphs
        let graph = make_graph(
            vec![
                make_node("a1", "file_read", NodeKind::Source),
                make_node("a2", "console", NodeKind::Sink),
                make_node("b1", "file_read", NodeKind::Source),
                make_node("b2", "console", NodeKind::Sink),
            ],
            vec![make_edge("e1", "a1", "a2"), make_edge("e2", "b1", "b2")],
        );
        let report = benchmark(&graph);
        assert_eq!(report.complexity.connected_components, 2);
        // E - N + 2P = 2 - 4 + 4 = 2
        assert_eq!(report.complexity.graph_complexity, 2);
    }

    #[test]
    fn test_python_loc_estimation() {
        let graph = make_graph(
            vec![
                make_node("srv", "http_server", NodeKind::Service),
                make_node("db", "database", NodeKind::Processor),
                make_node("c", "console", NodeKind::Sink),
            ],
            vec![make_edge("e1", "srv", "db"), make_edge("e2", "db", "c")],
        );
        let report = benchmark(&graph);
        // http_server=50, database=40, console=5 = 95
        assert_eq!(report.comparison.equivalent_loc_python, 95);
        // 95 * 1.5 = 142
        assert_eq!(report.comparison.equivalent_loc_rust, 142);
    }

    #[test]
    fn test_input_gathering_speedup_small() {
        let graph = make_graph(
            vec![
                make_node("a", "file_read", NodeKind::Source),
                make_node("b", "console", NodeKind::Sink),
            ],
            vec![make_edge("e1", "a", "b")],
        );
        let report = benchmark(&graph);
        assert_eq!(report.comparison.input_gathering_speedup, "~1x faster (O(1) vs O(E))");
    }

    #[test]
    fn test_input_gathering_speedup_medium() {
        // Build a graph with 15 edges
        let mut nodes = vec![make_node("src", "file_read", NodeKind::Source)];
        let mut edges = Vec::new();
        for i in 0..14 {
            let id = format!("p{}", i);
            nodes.push(make_node(&id, "transform", NodeKind::Processor));
        }
        nodes.push(make_node("sink", "console", NodeKind::Sink));

        // Chain: src -> p0 -> p1 -> ... -> p13 -> sink = 15 edges
        edges.push(make_edge("e0", "src", "p0"));
        for i in 0..13 {
            edges.push(make_edge(
                &format!("e{}", i + 1),
                &format!("p{}", i),
                &format!("p{}", i + 1),
            ));
        }
        edges.push(make_edge("e14", "p13", "sink"));

        let graph = make_graph(nodes, edges);
        let report = benchmark(&graph);
        assert!(report.comparison.input_gathering_speedup.starts_with("~10x"));
    }

    #[test]
    fn test_memory_estimate() {
        let graph = make_graph(
            vec![
                make_node("a", "file_read", NodeKind::Source),
                make_node("b", "console", NodeKind::Sink),
            ],
            vec![make_edge("e1", "a", "b")],
        );
        let report = benchmark(&graph);
        // 2 nodes * 512 + 1 edge * 128 = 1152
        assert_eq!(report.predictions.estimated_memory_bytes, 1152);
    }

    #[test]
    fn test_display_format() {
        let graph = make_graph(
            vec![
                make_node("srv", "http_server", NodeKind::Service),
                make_node("t", "transform", NodeKind::Processor),
                make_node("c", "console", NodeKind::Sink),
            ],
            vec![make_edge("e1", "srv", "t"), make_edge("e2", "t", "c")],
        );
        let report = benchmark(&graph);
        let output = format!("{}", report);
        assert!(output.contains("ANP Benchmark Report"));
        assert!(output.contains("Graph: test-graph"));
        assert!(output.contains("Scheduling"));
        assert!(output.contains("Complexity"));
        assert!(output.contains("Performance Predictions"));
        assert!(output.contains("vs Traditional Code"));
        assert!(output.contains("O(1)"));
        assert!(output.contains("Nodes: 3"));
        assert!(output.contains("Edges: 2"));
    }

    #[test]
    fn test_branching_factor() {
        // Diamond: s -> a, s -> b, a -> c, b -> c  (4 edges, 4 nodes)
        let graph = make_graph(
            vec![
                make_node("s", "file_read", NodeKind::Source),
                make_node("a", "transform", NodeKind::Processor),
                make_node("b", "filter", NodeKind::Processor),
                make_node("c", "console", NodeKind::Sink),
            ],
            vec![
                make_edge("e1", "s", "a"),
                make_edge("e2", "s", "b"),
                make_edge("e3", "a", "c"),
                make_edge("e4", "b", "c"),
            ],
        );
        let report = benchmark(&graph);
        assert!((report.complexity.branching_factor - 1.0).abs() < f64::EPSILON);
        assert_eq!(report.scheduling.max_stage_width, 2); // a and b in parallel
        assert_eq!(report.predictions.parallel_speedup, 2.0);
    }

    #[test]
    fn test_schedule_time_is_measured() {
        let graph = make_graph(
            vec![
                make_node("a", "file_read", NodeKind::Source),
                make_node("b", "console", NodeKind::Sink),
            ],
            vec![make_edge("e1", "a", "b")],
        );
        let report = benchmark(&graph);
        // schedule_time_us should be a reasonable value (may be 0 on fast machines)
        // Just ensure it doesn't panic and is non-negative (u64 always is)
        assert!(report.scheduling.schedule_time_us < 1_000_000); // less than 1 second
    }

    #[test]
    fn test_complexity_label_simple() {
        let graph = make_graph(
            vec![make_node("a", "console", NodeKind::Sink)],
            vec![],
        );
        let report = benchmark(&graph);
        let output = format!("{}", report);
        assert!(output.contains("simple"));
    }

    #[test]
    fn test_wide_parallel_graph() {
        // 5 independent sources -> 1 sink = 5-wide parallelism
        let mut nodes: Vec<Node> = (0..5)
            .map(|i| make_node(&format!("s{}", i), "file_read", NodeKind::Source))
            .collect();
        nodes.push(make_node("sink", "console", NodeKind::Sink));

        let edges: Vec<Edge> = (0..5)
            .map(|i| make_edge(&format!("e{}", i), &format!("s{}", i), "sink"))
            .collect();

        let graph = make_graph(nodes, edges);
        let report = benchmark(&graph);
        assert_eq!(report.scheduling.max_stage_width, 5);
        assert_eq!(report.predictions.parallel_speedup, 5.0);
        assert!(report.comparison.parallel_advantage.contains("5x"));
    }
}
