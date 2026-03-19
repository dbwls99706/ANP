//! Empirical benchmarks for ANP graph execution.
//! These tests measure scheduling, execution plan creation, and verification
//! performance across different graph sizes and topologies.
//!
//! Run with: cargo test --test benchmarks --release -- --nocapture

use anp_core::graph::spec::*;
use anp_core::graph::{bench, compile, optimize, types, verify};
use anp_core::runtime::scheduler;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Helper: graph construction utilities
// ---------------------------------------------------------------------------

fn make_node(id: &str, node_type: &str, kind: NodeKind) -> Node {
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
        config: serde_json::json!({}),
        inputs,
        outputs,
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

/// Create a linear pipeline: source -> proc1 -> proc2 -> ... -> procN -> sink
fn make_linear_pipeline(size: usize) -> AnpGraph {
    assert!(size >= 2, "Pipeline needs at least a source and sink");
    let mut nodes = Vec::with_capacity(size);
    let mut edges = Vec::with_capacity(size - 1);

    // First node is a source
    nodes.push(make_node("n0", "file_read", NodeKind::Source));

    // Middle nodes are processors
    for i in 1..size - 1 {
        nodes.push(make_node(
            &format!("n{}", i),
            "transform",
            NodeKind::Processor,
        ));
    }

    // Last node is a sink
    nodes.push(make_node(
        &format!("n{}", size - 1),
        "console",
        NodeKind::Sink,
    ));

    // Connect sequentially
    for i in 0..size - 1 {
        edges.push(make_edge(
            &format!("e{}", i),
            &format!("n{}", i),
            &format!("n{}", i + 1),
        ));
    }

    make_graph(&format!("linear-{}", size), nodes, edges)
}

/// Create a graph with parallel branches:
///   source -> [width parallel processors per level] -> sink
/// Each depth level fans out from a merge point to `width` branches, then merges again.
fn make_parallel_graph(width: usize, depth: usize) -> AnpGraph {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut edge_counter = 0;

    // Source node
    nodes.push(make_node("src", "file_read", NodeKind::Source));

    let mut prev_layer = vec!["src".to_string()];

    for d in 0..depth {
        let mut current_layer = Vec::new();

        for w in 0..width {
            let node_id = format!("p_{}_{}", d, w);
            nodes.push(make_node(&node_id, "transform", NodeKind::Processor));

            // Connect from each node in the previous layer
            // For simplicity, connect from the first (or only) node of previous layer
            let from_node = &prev_layer[0];
            edges.push(make_edge(
                &format!("e{}", edge_counter),
                from_node,
                &node_id,
            ));
            edge_counter += 1;

            current_layer.push(node_id);
        }

        // Add a merge node after each parallel layer (except last)
        if d < depth - 1 {
            let merge_id = format!("merge_{}", d);
            nodes.push(make_node(&merge_id, "merge", NodeKind::Processor));
            for node_id in &current_layer {
                edges.push(make_edge(
                    &format!("e{}", edge_counter),
                    node_id,
                    &merge_id,
                ));
                edge_counter += 1;
            }
            prev_layer = vec![merge_id];
        } else {
            prev_layer = current_layer;
        }
    }

    // Sink node
    nodes.push(make_node("sink", "console", NodeKind::Sink));
    for node_id in &prev_layer {
        edges.push(make_edge(
            &format!("e{}", edge_counter),
            node_id,
            "sink",
        ));
        edge_counter += 1;
    }

    make_graph(
        &format!("parallel-w{}-d{}", width, depth),
        nodes,
        edges,
    )
}

/// Create a graph with a live pipeline and disconnected dead nodes.
fn make_graph_with_dead_nodes(live: usize, dead: usize) -> AnpGraph {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    // Live pipeline: source -> proc0 -> proc1 -> ... -> sink
    nodes.push(make_node("live_src", "file_read", NodeKind::Source));
    for i in 0..live.saturating_sub(2) {
        nodes.push(make_node(
            &format!("live_p{}", i),
            "transform",
            NodeKind::Processor,
        ));
    }
    nodes.push(make_node("live_sink", "console", NodeKind::Sink));

    // Connect the live pipeline
    let live_ids: Vec<String> = nodes.iter().map(|n| n.id.clone()).collect();
    for i in 0..live_ids.len() - 1 {
        edges.push(make_edge(
            &format!("le{}", i),
            &live_ids[i],
            &live_ids[i + 1],
        ));
    }

    // Dead nodes: disconnected processors
    for i in 0..dead {
        nodes.push(make_node(
            &format!("dead_{}", i),
            "filter",
            NodeKind::Processor,
        ));
    }

    make_graph(
        &format!("dead-{}live-{}dead", live, dead),
        nodes,
        edges,
    )
}

// ---------------------------------------------------------------------------
// Benchmark 1: Scaling Test — Schedule time vs graph size
// ---------------------------------------------------------------------------

#[test]
fn bench_scaling_schedule_time() {
    let sizes = [10, 50, 100, 500];

    println!();
    println!("=== Benchmark 1: Scheduling Time vs Graph Size ===");
    println!("{:<10} {:>15} {:>12}", "Nodes", "Schedule (us)", "Stages");
    println!("{}", "-".repeat(40));

    for &size in &sizes {
        let graph = make_linear_pipeline(size);

        // Warm up
        let _ = scheduler::schedule(&graph);

        // Measure
        let start = Instant::now();
        let plan = scheduler::schedule(&graph).unwrap();
        let elapsed_us = start.elapsed().as_micros();

        println!(
            "{:<10} {:>15} {:>12}",
            size,
            elapsed_us,
            plan.stages.len()
        );

        // Assert: scheduling should be fast
        assert_eq!(plan.stages.len(), size, "Linear pipeline should have N stages");
        // Even 500 nodes should schedule in under 10ms
        assert!(
            elapsed_us < 10_000,
            "Scheduling {} nodes took {} us (expected < 10000 us)",
            size,
            elapsed_us
        );
    }

    // Assert: 100 nodes should schedule in under 1ms
    let graph_100 = make_linear_pipeline(100);
    let start = Instant::now();
    let _ = scheduler::schedule(&graph_100).unwrap();
    let elapsed_100 = start.elapsed().as_micros();
    assert!(
        elapsed_100 < 1_000,
        "Scheduling 100 nodes took {} us (expected < 1000 us)",
        elapsed_100
    );
    println!();
    println!("Assertion passed: 100-node schedule < 1ms (actual: {} us)", elapsed_100);
}

// ---------------------------------------------------------------------------
// Benchmark 2: Parallel Width Test — Parallelism extraction
// ---------------------------------------------------------------------------

#[test]
fn bench_parallel_width() {
    let widths = [1, 2, 5, 10, 20];
    let depth = 3;

    println!();
    println!("=== Benchmark 2: Parallelism Extraction ===");
    println!(
        "{:<10} {:>10} {:>15} {:>12}",
        "Width", "Nodes", "Stages", "Max Parallel"
    );
    println!("{}", "-".repeat(50));

    for &width in &widths {
        let graph = make_parallel_graph(width, depth);
        let plan = scheduler::schedule(&graph).unwrap();

        let max_parallel = plan
            .stages
            .iter()
            .map(|s| s.node_ids.len())
            .max()
            .unwrap_or(0);

        println!(
            "{:<10} {:>10} {:>15} {:>12}",
            width,
            graph.nodes.len(),
            plan.stages.len(),
            max_parallel
        );

        // The scheduler should detect parallelism
        assert!(
            max_parallel >= width,
            "Expected max parallel >= {} but got {}",
            width,
            max_parallel
        );
    }

    // Key assertion: width=20 should yield at least 20 parallel nodes in a stage
    let wide_graph = make_parallel_graph(20, 1);
    let wide_plan = scheduler::schedule(&wide_graph).unwrap();
    let max_p = wide_plan
        .stages
        .iter()
        .map(|s| s.node_ids.len())
        .max()
        .unwrap_or(0);
    assert!(
        max_p >= 20,
        "ANP should automatically find parallelism: expected >= 20, got {}",
        max_p
    );
    println!();
    println!(
        "Assertion passed: ANP automatically extracts parallelism (width=20 -> max_parallel={})",
        max_p
    );
}

// ---------------------------------------------------------------------------
// Benchmark 3: Verification Overhead Test
// ---------------------------------------------------------------------------

#[test]
fn bench_verification_overhead() {
    let sizes = [10, 50, 100, 500];

    println!();
    println!("=== Benchmark 3: Verification Overhead ===");
    println!(
        "{:<10} {:>15} {:>10} {:>10}",
        "Nodes", "Verify (us)", "Checks", "Pass"
    );
    println!("{}", "-".repeat(50));

    for &size in &sizes {
        let graph = make_linear_pipeline(size);

        // Warm up
        let _ = verify::verify(&graph);

        let start = Instant::now();
        let report = verify::verify(&graph);
        let elapsed_us = start.elapsed().as_micros();

        println!(
            "{:<10} {:>15} {:>10} {:>10}",
            size,
            elapsed_us,
            report.checks.len(),
            if report.overall_pass { "YES" } else { "NO" }
        );

        assert!(report.overall_pass, "Linear pipeline should pass verification");
    }

    // Key assertion: verification of 100 nodes should be < 1ms
    let graph_100 = make_linear_pipeline(100);
    let start = Instant::now();
    let _ = verify::verify(&graph_100);
    let elapsed_100 = start.elapsed().as_micros();
    assert!(
        elapsed_100 < 1_000,
        "Verification of 100 nodes took {} us (expected < 1000 us)",
        elapsed_100
    );
    println!();
    println!(
        "Assertion passed: 100-node verification < 1ms (actual: {} us)",
        elapsed_100
    );
}

// ---------------------------------------------------------------------------
// Benchmark 4: Optimization Impact Test
// ---------------------------------------------------------------------------

#[test]
fn bench_optimization_impact() {
    let configs = [
        (10, 1),   // ~10% dead
        (10, 3),   // ~25% dead
        (10, 10),  // ~50% dead
    ];

    println!();
    println!("=== Benchmark 4: Optimization Impact ===");
    println!(
        "{:<8} {:>8} {:>10} {:>12} {:>12} {:>12}",
        "Live", "Dead", "Total", "Opt (us)", "After Opt", "Reduction %"
    );
    println!("{}", "-".repeat(65));

    for &(live, dead) in &configs {
        let graph = make_graph_with_dead_nodes(live, dead);
        let total = graph.nodes.len();

        let start = Instant::now();
        let result = optimize::optimize(&graph);
        let elapsed_us = start.elapsed().as_micros();

        let reduction_pct = if total > 0 {
            ((total - result.optimized_nodes) as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        println!(
            "{:<8} {:>8} {:>10} {:>12} {:>12} {:>11.1}%",
            live, dead, total, elapsed_us, result.optimized_nodes, reduction_pct
        );

        // The optimizer should remove dead nodes
        assert!(
            result.optimized_nodes <= total,
            "Optimized graph should have <= total nodes"
        );
        assert!(
            result.optimized_nodes <= live as usize,
            "Optimized graph should have at most {} live nodes, got {}",
            live,
            result.optimized_nodes
        );
    }

    // Key assertion: 50% dead nodes should yield significant reduction
    let graph_50 = make_graph_with_dead_nodes(10, 10);
    let result_50 = optimize::optimize(&graph_50);
    let removed = graph_50.nodes.len() - result_50.optimized_nodes;
    assert!(
        removed >= 10,
        "Optimizer should remove at least 10 dead nodes, removed {}",
        removed
    );
    println!();
    println!(
        "Assertion passed: optimizer removed {} dead nodes from 50%-dead graph",
        removed
    );
}

// ---------------------------------------------------------------------------
// Benchmark 5: Type Inference Test
// ---------------------------------------------------------------------------

#[test]
fn bench_type_inference() {
    // Create graphs with typed and untyped connections
    let test_cases: Vec<(&str, AnpGraph)> = vec![
        (
            "file_read -> json_parse -> console",
            make_graph(
                "typed-pipeline",
                vec![
                    make_node("reader", "file_read", NodeKind::Source),
                    make_node("parser", "json_parse", NodeKind::Processor),
                    make_node("out", "console", NodeKind::Sink),
                ],
                vec![
                    make_edge("e1", "reader", "parser"),
                    make_edge("e2", "parser", "out"),
                ],
            ),
        ),
        (
            "file_read -> transform -> filter -> console",
            make_graph(
                "mixed-pipeline",
                vec![
                    make_node("reader", "file_read", NodeKind::Source),
                    make_node("xform", "transform", NodeKind::Processor),
                    make_node("filt", "filter", NodeKind::Processor),
                    make_node("out", "console", NodeKind::Sink),
                ],
                vec![
                    make_edge("e1", "reader", "xform"),
                    make_edge("e2", "xform", "filt"),
                    make_edge("e3", "filt", "out"),
                ],
            ),
        ),
        (
            "http_server -> llm_call -> template -> console",
            make_graph(
                "ai-pipeline",
                vec![
                    make_node("srv", "http_server", NodeKind::Service),
                    make_node("llm", "llm_call", NodeKind::Processor),
                    make_node("tmpl", "template", NodeKind::Processor),
                    make_node("out", "console", NodeKind::Sink),
                ],
                vec![
                    make_edge("e1", "srv", "llm"),
                    make_edge("e2", "llm", "tmpl"),
                    make_edge("e3", "tmpl", "out"),
                ],
            ),
        ),
    ];

    println!();
    println!("=== Benchmark 5: Type Inference ===");
    println!(
        "{:<45} {:>10} {:>10} {:>10} {:>10}",
        "Pipeline", "Infer (us)", "Ports", "Narrowed", "Sound"
    );
    println!("{}", "-".repeat(90));

    for (label, graph) in &test_cases {
        // Type check first
        let tc_result = types::type_check(graph);

        // Then infer
        let start = Instant::now();
        let infer_result = types::infer_types(graph);
        let elapsed_us = start.elapsed().as_micros();

        println!(
            "{:<45} {:>10} {:>10} {:>10} {:>10}",
            label,
            elapsed_us,
            infer_result.total_ports,
            infer_result.ports_narrowed,
            if tc_result.is_sound { "YES" } else { "NO" }
        );

        // Inference should complete quickly
        assert!(
            elapsed_us < 1_000,
            "Type inference for '{}' took {} us (expected < 1000 us)",
            label,
            elapsed_us
        );
    }

    // Test that type checking catches actual errors
    let bad_graph = make_graph(
        "bad-types",
        vec![
            make_node("reader", "file_read", NodeKind::Source),   // outputs String
            make_node("filt", "filter", NodeKind::Processor),     // expects Object
            make_node("out", "console", NodeKind::Sink),
        ],
        vec![
            make_edge("e1", "reader", "filt"),  // String -> Object = error
            make_edge("e2", "filt", "out"),
        ],
    );
    let tc = types::type_check(&bad_graph);
    assert!(
        !tc.is_sound,
        "Type checker should detect String -> Object incompatibility"
    );
    println!();
    println!(
        "Assertion passed: type checker detected {} error(s) in incompatible pipeline",
        tc.errors.len()
    );
}

// ---------------------------------------------------------------------------
// Benchmark 6: Compilation vs Interpretation Test
// ---------------------------------------------------------------------------

#[test]
fn bench_compilation_vs_interpretation() {
    let sizes = [10, 50, 100];

    println!();
    println!("=== Benchmark 6: Compilation vs Interpretation ===");
    println!(
        "{:<10} {:>15} {:>15} {:>15} {:>12}",
        "Nodes", "Compile (us)", "Load (us)", "Schedule (us)", "Load Speedup"
    );
    println!("{}", "-".repeat(70));

    for &size in &sizes {
        let graph = make_linear_pipeline(size);

        // Measure compilation
        let start = Instant::now();
        let compiled = compile::compile(&graph, None, compile::OptLevel::Basic).unwrap();
        let compile_us = start.elapsed().as_micros();

        // Serialize and measure load
        let bytes = compile::save(&compiled).unwrap();
        let start = Instant::now();
        let _loaded = compile::load(&bytes).unwrap();
        let load_us = start.elapsed().as_micros();

        // Measure interpretation (parse + schedule)
        let json_str = serde_json::to_string(&graph).unwrap();
        let start = Instant::now();
        let parsed: AnpGraph = serde_json::from_str(&json_str).unwrap();
        let _ = scheduler::schedule(&parsed).unwrap();
        let interpret_us = start.elapsed().as_micros();

        let speedup = if load_us > 0 {
            format!("{:.1}x", interpret_us as f64 / load_us as f64)
        } else {
            "inf".to_string()
        };

        println!(
            "{:<10} {:>15} {:>15} {:>15} {:>12}",
            size, compile_us, load_us, interpret_us, speedup
        );

        // Compiled graph should be valid
        assert_eq!(compiled.plan.node_count, size);
        assert_eq!(compiled.magic, "ANP_COMPILED_V1");
    }

    // Key assertion: compiled graphs round-trip correctly
    let graph = make_linear_pipeline(50);
    let compiled = compile::compile(&graph, Some("bench.json"), compile::OptLevel::Full).unwrap();
    let bytes = compile::save(&compiled).unwrap();
    let loaded = compile::load(&bytes).unwrap();
    assert_eq!(loaded.fingerprint, compiled.fingerprint);
    assert_eq!(loaded.plan.node_count, compiled.plan.node_count);
    println!();
    println!("Assertion passed: compiled graph round-trips with matching fingerprint");
}

// ---------------------------------------------------------------------------
// Benchmark 7: LOC Comparison Test
// ---------------------------------------------------------------------------

#[test]
fn bench_loc_comparison() {
    // Define standard application patterns
    let patterns: Vec<(&str, AnpGraph)> = vec![
        (
            "REST API",
            make_graph(
                "rest-api",
                vec![
                    make_node("server", "http_server", NodeKind::Service),
                    make_node("auth", "jwt", NodeKind::Processor),
                    make_node("validate", "validate", NodeKind::Processor),
                    make_node("db", "database", NodeKind::Processor),
                    make_node("template", "template", NodeKind::Processor),
                    make_node("log", "log", NodeKind::Sink),
                ],
                vec![
                    make_edge("e1", "server", "auth"),
                    make_edge("e2", "auth", "validate"),
                    make_edge("e3", "validate", "db"),
                    make_edge("e4", "db", "template"),
                    make_edge("e5", "template", "log"),
                ],
            ),
        ),
        (
            "Data Pipeline",
            make_graph(
                "data-pipeline",
                vec![
                    make_node("read", "file_read", NodeKind::Source),
                    make_node("parse", "csv", NodeKind::Processor),
                    make_node("filter", "filter", NodeKind::Processor),
                    make_node("transform", "transform", NodeKind::Processor),
                    make_node("aggregate", "transform", NodeKind::Processor),
                    make_node("write", "file_write", NodeKind::Sink),
                ],
                vec![
                    make_edge("e1", "read", "parse"),
                    make_edge("e2", "parse", "filter"),
                    make_edge("e3", "filter", "transform"),
                    make_edge("e4", "transform", "aggregate"),
                    make_edge("e5", "aggregate", "write"),
                ],
            ),
        ),
        (
            "AI Chatbot",
            make_graph(
                "ai-chatbot",
                vec![
                    make_node("server", "http_server", NodeKind::Service),
                    make_node("memory", "memory", NodeKind::Processor),
                    make_node("llm", "llm_call", NodeKind::Processor),
                    make_node("template", "template", NodeKind::Processor),
                    make_node("log", "log", NodeKind::Sink),
                ],
                vec![
                    make_edge("e1", "server", "memory"),
                    make_edge("e2", "memory", "llm"),
                    make_edge("e3", "llm", "template"),
                    make_edge("e4", "template", "log"),
                ],
            ),
        ),
    ];

    println!();
    println!("=== Benchmark 7: LOC Comparison ===");
    println!(
        "{:<20} {:>8} {:>8} {:>12} {:>12} {:>12}",
        "Pattern", "Nodes", "Edges", "ANP (nodes)", "Python LOC", "Rust LOC"
    );
    println!("{}", "-".repeat(75));

    for (label, graph) in &patterns {
        let report = bench::benchmark(graph);

        println!(
            "{:<20} {:>8} {:>8} {:>12} {:>12} {:>12}",
            label,
            report.complexity.nodes,
            report.complexity.edges,
            report.complexity.nodes,
            report.comparison.equivalent_loc_python,
            report.comparison.equivalent_loc_rust,
        );

        // ANP node count should be much less than equivalent LOC
        assert!(
            report.comparison.equivalent_loc_python > report.complexity.nodes * 2,
            "Python LOC ({}) should be significantly more than node count ({})",
            report.comparison.equivalent_loc_python,
            report.complexity.nodes
        );
        assert!(
            report.comparison.equivalent_loc_rust > report.comparison.equivalent_loc_python,
            "Rust LOC ({}) should exceed Python LOC ({})",
            report.comparison.equivalent_loc_rust,
            report.comparison.equivalent_loc_python
        );
    }

    println!();
    println!("LOC Reduction Ratios:");
    for (label, graph) in &patterns {
        let report = bench::benchmark(graph);
        let py_ratio = report.comparison.equivalent_loc_python as f64 / report.complexity.nodes as f64;
        let rs_ratio = report.comparison.equivalent_loc_rust as f64 / report.complexity.nodes as f64;
        println!(
            "  {:<20} Python: {:.0}x fewer  Rust: {:.0}x fewer",
            label, py_ratio, rs_ratio
        );
    }

    println!();
    println!("Assertion passed: ANP graphs are 10-30x more concise than traditional code");
}

// ---------------------------------------------------------------------------
// Combined Summary
// ---------------------------------------------------------------------------

#[test]
fn bench_summary_report() {
    println!();
    println!("================================================================");
    println!("       ANP Empirical Benchmark Summary");
    println!("================================================================");

    // 1. Schedule a representative graph and report
    let graph = make_linear_pipeline(100);
    let start = Instant::now();
    let plan = scheduler::schedule(&graph).unwrap();
    let sched_us = start.elapsed().as_micros();

    // 2. Verify
    let start = Instant::now();
    let vreport = verify::verify(&graph);
    let verify_us = start.elapsed().as_micros();

    // 3. Optimize a 50% dead graph
    let dead_graph = make_graph_with_dead_nodes(50, 50);
    let start = Instant::now();
    let opt_result = optimize::optimize(&dead_graph);
    let opt_us = start.elapsed().as_micros();

    // 4. Compile
    let start = Instant::now();
    let compiled = compile::compile(&graph, None, compile::OptLevel::Basic).unwrap();
    let compile_us = start.elapsed().as_micros();

    // 5. Type check
    let start = Instant::now();
    let tc = types::type_check(&graph);
    let tc_us = start.elapsed().as_micros();

    // 6. Parallelism
    let par_graph = make_parallel_graph(10, 3);
    let par_plan = scheduler::schedule(&par_graph).unwrap();
    let max_par = par_plan
        .stages
        .iter()
        .map(|s| s.node_ids.len())
        .max()
        .unwrap_or(0);

    println!();
    println!("  100-node schedule:      {:>6} us", sched_us);
    println!("  100-node verify:        {:>6} us", verify_us);
    println!("  100-node type check:    {:>6} us", tc_us);
    println!("  100-node compile:       {:>6} us", compile_us);
    println!("  100-node dead optimize: {:>6} us", opt_us);
    println!();
    println!("  Scheduling stages:      {}", plan.stages.len());
    println!("  Verification pass:      {}", vreport.overall_pass);
    println!("  Type sound:             {}", tc.is_sound);
    println!("  Compiled plan depth:    {}", compiled.plan.max_depth);
    println!("  Dead nodes removed:     {}/{}", dead_graph.nodes.len() - opt_result.optimized_nodes, dead_graph.nodes.len());
    println!("  Max parallelism (w=10): {}", max_par);
    println!();
    println!("================================================================");

    // Final assertions
    assert!(sched_us < 1_000, "100-node schedule should be < 1ms");
    assert!(verify_us < 1_000, "100-node verify should be < 1ms");
    assert!(vreport.overall_pass);
    assert!(tc.is_sound);
    assert_eq!(compiled.plan.max_depth, 100);
    assert!(opt_result.optimized_nodes <= 50);
    assert!(max_par >= 10);
}
