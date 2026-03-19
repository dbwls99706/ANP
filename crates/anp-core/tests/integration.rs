//! Cross-crate integration tests for the ANP runtime.
//! Tests end-to-end: graph loading → validation → scheduling → execution.

use anp_core::graph::spec::*;
use anp_core::graph::validate;
use anp_core::node::plugin::NodePlugin;
use anp_core::node::registry::{NodeInfo, NodeRegistry, NodeSchema};
use anp_core::node::traits::*;
use anp_core::runtime::context::ExecutionContext;
use anp_core::runtime::engine::AnpEngine;
use anp_core::runtime::scheduler;

// ── Mock nodes for integration testing ──

struct EchoSource {
    data: serde_json::Value,
}

#[async_trait::async_trait]
impl NodeExecutor for EchoSource {
    fn node_type(&self) -> &str {
        "echo_source"
    }
    fn node_kind(&self) -> NodeKind {
        NodeKind::Source
    }
    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(data) = config.get("data") {
            self.data = data.clone();
        }
        Ok(())
    }
    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        Ok(NodeOutput::data(self.data.clone()))
    }
}

struct UpperCaseProcessor;

#[async_trait::async_trait]
impl NodeExecutor for UpperCaseProcessor {
    fn node_type(&self) -> &str {
        "upper_case"
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
        let text = input.as_str().unwrap_or_default().to_uppercase();
        Ok(NodeOutput::data(serde_json::json!(text)))
    }
}

struct CollectorSink;

#[async_trait::async_trait]
impl NodeExecutor for CollectorSink {
    fn node_type(&self) -> &str {
        "collector"
    }
    fn node_kind(&self) -> NodeKind {
        NodeKind::Sink
    }
    async fn init(&mut self, _config: &serde_json::Value) -> NodeResult<()> {
        Ok(())
    }
    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        // Store result in global state for test verification
        let state = ctx.state();
        let mut s = state.write().await;
        s["collected"] = input;
        Ok(NodeOutput::data(serde_json::Value::Null))
    }
}

struct DoubleProcessor;

#[async_trait::async_trait]
impl NodeExecutor for DoubleProcessor {
    fn node_type(&self) -> &str {
        "double"
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
        if let Some(n) = input.as_i64() {
            Ok(NodeOutput::data(serde_json::json!(n * 2)))
        } else {
            Ok(NodeOutput::data(input))
        }
    }
}

// ── Test plugin ──

struct TestPlugin;

impl NodePlugin for TestPlugin {
    fn name(&self) -> &str {
        "test-plugin"
    }
    fn version(&self) -> &str {
        "0.1.0"
    }
    fn register(&self, registry: &mut NodeRegistry) {
        registry.register(NodeInfo {
            factory: || {
                Box::new(EchoSource {
                    data: serde_json::json!("default"),
                })
            },
            schema: NodeSchema {
                node_type: "echo_source".to_string(),
                kind: NodeKind::Source,
                description: "Echoes configured data".to_string(),
                inputs: vec![],
                outputs: vec![PortSpec::new("default")],
                config_schema: serde_json::json!({}),
            },
        });
        registry.register(NodeInfo {
            factory: || Box::new(UpperCaseProcessor),
            schema: NodeSchema {
                node_type: "upper_case".to_string(),
                kind: NodeKind::Processor,
                description: "Converts text to upper case".to_string(),
                inputs: vec![PortSpec::new("default")],
                outputs: vec![PortSpec::new("default")],
                config_schema: serde_json::json!({}),
            },
        });
        registry.register(NodeInfo {
            factory: || Box::new(CollectorSink),
            schema: NodeSchema {
                node_type: "collector".to_string(),
                kind: NodeKind::Sink,
                description: "Collects data into global state".to_string(),
                inputs: vec![PortSpec::new("default")],
                outputs: vec![],
                config_schema: serde_json::json!({}),
            },
        });
        registry.register(NodeInfo {
            factory: || Box::new(DoubleProcessor),
            schema: NodeSchema {
                node_type: "double".to_string(),
                kind: NodeKind::Processor,
                description: "Doubles numeric input".to_string(),
                inputs: vec![PortSpec::new("default")],
                outputs: vec![PortSpec::new("default")],
                config_schema: serde_json::json!({}),
            },
        });
    }
}

fn test_engine() -> AnpEngine {
    let mut registry = NodeRegistry::new();
    registry.register_plugin(&TestPlugin);
    AnpEngine::new(registry)
}

// ── Integration tests ──

#[tokio::test]
async fn test_end_to_end_source_to_sink() {
    let engine = test_engine();
    let json = r#"{
        "version": "0.1",
        "metadata": { "name": "e2e-test", "description": "end to end" },
        "nodes": [
            { "id": "src", "node_type": "echo_source", "kind": "source",
              "config": { "data": "hello world" },
              "inputs": [], "outputs": [{"name": "default"}] },
            { "id": "sink", "node_type": "collector", "kind": "sink",
              "config": {},
              "inputs": [{"name": "default"}], "outputs": [] }
        ],
        "edges": [
            { "id": "e1", "from_node": "src", "to_node": "sink" }
        ]
    }"#;
    let graph = engine.load_graph_str(json).unwrap();
    engine.run(&graph).await.unwrap();
}

#[tokio::test]
async fn test_end_to_end_three_stage_pipeline() {
    let engine = test_engine();
    let json = r#"{
        "version": "0.1",
        "metadata": { "name": "pipeline-test", "description": "" },
        "nodes": [
            { "id": "src", "node_type": "echo_source", "kind": "source",
              "config": { "data": "hello" },
              "inputs": [], "outputs": [{"name": "default"}] },
            { "id": "upper", "node_type": "upper_case", "kind": "processor",
              "config": {},
              "inputs": [{"name": "default"}], "outputs": [{"name": "default"}] },
            { "id": "sink", "node_type": "collector", "kind": "sink",
              "config": {},
              "inputs": [{"name": "default"}], "outputs": [] }
        ],
        "edges": [
            { "id": "e1", "from_node": "src", "to_node": "upper" },
            { "id": "e2", "from_node": "upper", "to_node": "sink" }
        ]
    }"#;
    let graph = engine.load_graph_str(json).unwrap();
    engine.run(&graph).await.unwrap();
}

#[tokio::test]
async fn test_end_to_end_parallel_sources() {
    let engine = test_engine();
    let json = r#"{
        "version": "0.1",
        "metadata": { "name": "parallel-test", "description": "" },
        "nodes": [
            { "id": "src1", "node_type": "echo_source", "kind": "source",
              "config": { "data": "a" },
              "inputs": [], "outputs": [{"name": "default"}] },
            { "id": "src2", "node_type": "echo_source", "kind": "source",
              "config": { "data": "b" },
              "inputs": [], "outputs": [{"name": "default"}] },
            { "id": "sink", "node_type": "collector", "kind": "sink",
              "config": {},
              "inputs": [{"name": "default"}], "outputs": [] }
        ],
        "edges": [
            { "id": "e1", "from_node": "src1", "to_node": "sink" },
            { "id": "e2", "from_node": "src2", "to_node": "sink" }
        ]
    }"#;
    let graph = engine.load_graph_str(json).unwrap();
    engine.run(&graph).await.unwrap();
}

#[tokio::test]
async fn test_end_to_end_diamond_topology() {
    // src → upper → sink
    //   └→ double → sink
    let engine = test_engine();
    let json = r#"{
        "version": "0.1",
        "metadata": { "name": "diamond", "description": "" },
        "nodes": [
            { "id": "src", "node_type": "echo_source", "kind": "source",
              "config": { "data": 5 },
              "inputs": [], "outputs": [{"name": "default"}] },
            { "id": "upper", "node_type": "upper_case", "kind": "processor",
              "config": {},
              "inputs": [{"name": "default"}], "outputs": [{"name": "default"}] },
            { "id": "double", "node_type": "double", "kind": "processor",
              "config": {},
              "inputs": [{"name": "default"}], "outputs": [{"name": "default"}] },
            { "id": "sink", "node_type": "collector", "kind": "sink",
              "config": {},
              "inputs": [{"name": "default"}], "outputs": [] }
        ],
        "edges": [
            { "id": "e1", "from_node": "src", "to_node": "upper" },
            { "id": "e2", "from_node": "src", "to_node": "double" },
            { "id": "e3", "from_node": "upper", "to_node": "sink" },
            { "id": "e4", "from_node": "double", "to_node": "sink" }
        ]
    }"#;
    let graph = engine.load_graph_str(json).unwrap();
    engine.run(&graph).await.unwrap();
}

#[test]
fn test_validate_rejects_cycle() {
    let graph = AnpGraph {
        version: GraphVersion::V0_1,
        metadata: GraphMetadata {
            name: "cycle".into(),
            description: String::new(),
        },
        nodes: vec![
            Node {
                id: "a".into(),
                node_type: "upper_case".into(),
                kind: NodeKind::Processor,
                config: serde_json::Value::Null,
                inputs: vec![PortSpec::new("default")],
                outputs: vec![PortSpec::new("default")],
            },
            Node {
                id: "b".into(),
                node_type: "upper_case".into(),
                kind: NodeKind::Processor,
                config: serde_json::Value::Null,
                inputs: vec![PortSpec::new("default")],
                outputs: vec![PortSpec::new("default")],
            },
        ],
        edges: vec![
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
                to_node: "a".into(),
                to_port: "default".into(),
            },
        ],
    };
    let errors = validate::validate(&graph).unwrap_err();
    let has_cycle = errors
        .iter()
        .any(|e| matches!(e, validate::ValidationError::CycleDetected(_)));
    assert!(has_cycle);
}

#[test]
fn test_validate_rejects_duplicate_ids() {
    let graph = AnpGraph {
        version: GraphVersion::V0_1,
        metadata: GraphMetadata {
            name: "dup".into(),
            description: String::new(),
        },
        nodes: vec![
            Node {
                id: "same".into(),
                node_type: "x".into(),
                kind: NodeKind::Source,
                config: serde_json::Value::Null,
                inputs: vec![],
                outputs: vec![PortSpec::new("default")],
            },
            Node {
                id: "same".into(),
                node_type: "x".into(),
                kind: NodeKind::Source,
                config: serde_json::Value::Null,
                inputs: vec![],
                outputs: vec![PortSpec::new("default")],
            },
        ],
        edges: vec![],
    };
    let errors = validate::validate(&graph).unwrap_err();
    let has_dup = errors
        .iter()
        .any(|e| matches!(e, validate::ValidationError::DuplicateNodeId(_)));
    assert!(has_dup);
}

#[test]
fn test_validate_rejects_bad_edge_refs() {
    let graph = AnpGraph {
        version: GraphVersion::V0_1,
        metadata: GraphMetadata {
            name: "bad-edge".into(),
            description: String::new(),
        },
        nodes: vec![Node {
            id: "a".into(),
            node_type: "x".into(),
            kind: NodeKind::Source,
            config: serde_json::Value::Null,
            inputs: vec![],
            outputs: vec![PortSpec::new("default")],
        }],
        edges: vec![Edge {
            id: "e1".into(),
            from_node: "a".into(),
            from_port: "default".into(),
            to_node: "nonexistent".into(),
            to_port: "default".into(),
        }],
    };
    let errors = validate::validate(&graph).unwrap_err();
    assert!(!errors.is_empty());
}

#[test]
fn test_scheduler_rejects_cycle() {
    let graph = AnpGraph {
        version: GraphVersion::V0_1,
        metadata: GraphMetadata {
            name: "cycle".into(),
            description: String::new(),
        },
        nodes: vec![
            Node {
                id: "a".into(),
                node_type: "x".into(),
                kind: NodeKind::Processor,
                config: serde_json::Value::Null,
                inputs: vec![PortSpec::new("default")],
                outputs: vec![PortSpec::new("default")],
            },
            Node {
                id: "b".into(),
                node_type: "x".into(),
                kind: NodeKind::Processor,
                config: serde_json::Value::Null,
                inputs: vec![PortSpec::new("default")],
                outputs: vec![PortSpec::new("default")],
            },
        ],
        edges: vec![
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
                to_node: "a".into(),
                to_port: "default".into(),
            },
        ],
    };
    let result = scheduler::schedule(&graph);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        scheduler::ScheduleError::CycleDetected
    ));
}

#[test]
fn test_scheduler_edge_map_correctness() {
    let graph = AnpGraph {
        version: GraphVersion::V0_1,
        metadata: GraphMetadata {
            name: "edge-map".into(),
            description: String::new(),
        },
        nodes: vec![
            Node {
                id: "a".into(),
                node_type: "x".into(),
                kind: NodeKind::Source,
                config: serde_json::Value::Null,
                inputs: vec![],
                outputs: vec![PortSpec::new("out1"), PortSpec::new("out2")],
            },
            Node {
                id: "b".into(),
                node_type: "x".into(),
                kind: NodeKind::Processor,
                config: serde_json::Value::Null,
                inputs: vec![PortSpec::new("default")],
                outputs: vec![],
            },
            Node {
                id: "c".into(),
                node_type: "x".into(),
                kind: NodeKind::Processor,
                config: serde_json::Value::Null,
                inputs: vec![PortSpec::new("default")],
                outputs: vec![],
            },
        ],
        edges: vec![
            Edge {
                id: "e1".into(),
                from_node: "a".into(),
                from_port: "out1".into(),
                to_node: "b".into(),
                to_port: "default".into(),
            },
            Edge {
                id: "e2".into(),
                from_node: "a".into(),
                from_port: "out2".into(),
                to_node: "c".into(),
                to_port: "default".into(),
            },
        ],
    };
    let plan = scheduler::schedule(&graph).unwrap();

    // Verify edge_map structure
    let a_ports = plan.edge_map.get("a").unwrap();
    assert!(a_ports.contains_key("out1"));
    assert!(a_ports.contains_key("out2"));
    assert_eq!(a_ports["out1"][0].to_node, "b");
    assert_eq!(a_ports["out2"][0].to_node, "c");
}

#[test]
fn test_scheduler_node_map_correctness() {
    let graph = AnpGraph {
        version: GraphVersion::V0_1,
        metadata: GraphMetadata {
            name: "node-map".into(),
            description: String::new(),
        },
        nodes: vec![
            Node {
                id: "s".into(),
                node_type: "x".into(),
                kind: NodeKind::Source,
                config: serde_json::Value::Null,
                inputs: vec![],
                outputs: vec![PortSpec::new("default")],
            },
            Node {
                id: "p".into(),
                node_type: "x".into(),
                kind: NodeKind::Processor,
                config: serde_json::Value::Null,
                inputs: vec![PortSpec::new("default")],
                outputs: vec![PortSpec::new("default")],
            },
            Node {
                id: "k".into(),
                node_type: "x".into(),
                kind: NodeKind::Sink,
                config: serde_json::Value::Null,
                inputs: vec![PortSpec::new("default")],
                outputs: vec![],
            },
        ],
        edges: vec![
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
    };
    let plan = scheduler::schedule(&graph).unwrap();

    // Each node should be in increasing stage
    assert!(plan.node_map["s"] < plan.node_map["p"]);
    assert!(plan.node_map["p"] < plan.node_map["k"]);
}

#[test]
fn test_plugin_registration() {
    let mut registry = NodeRegistry::new();
    registry.register_plugin(&TestPlugin);

    assert!(registry.has("echo_source"));
    assert!(registry.has("upper_case"));
    assert!(registry.has("collector"));
    assert!(registry.has("double"));
    assert_eq!(registry.node_types().len(), 4);

    // Verify factory works
    let node = registry.create_node("echo_source").unwrap();
    assert_eq!(node.node_type(), "echo_source");
    assert_eq!(node.node_kind(), NodeKind::Source);
}

#[test]
fn test_multiple_plugins() {
    struct SecondPlugin;
    impl NodePlugin for SecondPlugin {
        fn name(&self) -> &str {
            "second"
        }
        fn version(&self) -> &str {
            "1.0.0"
        }
        fn register(&self, registry: &mut NodeRegistry) {
            registry.register(NodeInfo {
                factory: || {
                    Box::new(EchoSource {
                        data: serde_json::json!("from second"),
                    })
                },
                schema: NodeSchema {
                    node_type: "second_source".to_string(),
                    kind: NodeKind::Source,
                    description: "second plugin source".to_string(),
                    inputs: vec![],
                    outputs: vec![PortSpec::new("default")],
                    config_schema: serde_json::json!({}),
                },
            });
        }
    }

    let mut registry = NodeRegistry::new();
    let plugins: Vec<&dyn NodePlugin> = vec![&TestPlugin, &SecondPlugin];
    registry.register_plugins(&plugins);

    assert!(registry.has("echo_source"));
    assert!(registry.has("second_source"));
    assert_eq!(registry.node_types().len(), 5); // 4 from TestPlugin + 1 from SecondPlugin
}

#[test]
fn test_empty_graph_schedules() {
    let graph = AnpGraph {
        version: GraphVersion::V0_1,
        metadata: GraphMetadata {
            name: "empty".into(),
            description: String::new(),
        },
        nodes: vec![],
        edges: vec![],
    };
    let plan = scheduler::schedule(&graph).unwrap();
    assert!(plan.stages.is_empty());
    assert!(plan.entry_nodes.is_empty());
}

#[test]
fn test_single_node_graph() {
    let graph = AnpGraph {
        version: GraphVersion::V0_1,
        metadata: GraphMetadata {
            name: "single".into(),
            description: String::new(),
        },
        nodes: vec![Node {
            id: "only".into(),
            node_type: "echo_source".into(),
            kind: NodeKind::Source,
            config: serde_json::Value::Null,
            inputs: vec![],
            outputs: vec![PortSpec::new("default")],
        }],
        edges: vec![],
    };
    let plan = scheduler::schedule(&graph).unwrap();
    assert_eq!(plan.stages.len(), 1);
    assert_eq!(plan.stages[0].node_ids, vec!["only"]);
    assert_eq!(plan.entry_nodes, vec!["only"]);
}

#[test]
fn test_context_trace_recording() {
    use anp_core::runtime::context::{ExecutionContext, NodeTrace};

    let ctx = ExecutionContext::new();
    let run_id = "trace-test";
    ctx.start_run(run_id.into(), "src".into());

    ctx.record_trace(
        run_id,
        NodeTrace {
            node_id: "src".into(),
            node_type: "echo_source".into(),
            duration_us: 100,
            success: true,
            error: None,
            output_ports: vec!["default".into()],
        },
    );
    ctx.record_trace(
        run_id,
        NodeTrace {
            node_id: "proc".into(),
            node_type: "upper_case".into(),
            duration_us: 200,
            success: false,
            error: Some("test error".into()),
            output_ports: vec![],
        },
    );

    let traces = ctx.get_traces(run_id);
    assert_eq!(traces.len(), 2);
    assert!(traces[0].success);
    assert!(!traces[1].success);
    assert_eq!(traces[1].error, Some("test error".into()));
}

#[tokio::test]
async fn test_context_global_state() {
    let ctx = ExecutionContext::new();
    let state = ctx.state();

    {
        let mut s = state.write().await;
        s["key"] = serde_json::json!("value");
    }

    {
        let s = state.read().await;
        assert_eq!(s["key"], "value");
    }
}

#[test]
fn test_context_multiple_concurrent_runs() {
    let ctx = ExecutionContext::new();

    ctx.start_run("run-1".into(), "a".into());
    ctx.start_run("run-2".into(), "b".into());
    assert_eq!(ctx.active_runs(), 2);

    ctx.set_node_output("run-1", "a", serde_json::json!("data-1"));
    ctx.set_node_output("run-2", "b", serde_json::json!("data-2"));

    // Runs are isolated
    assert_eq!(
        ctx.get_node_output("run-1", "a").unwrap(),
        serde_json::json!("data-1")
    );
    assert_eq!(
        ctx.get_node_output("run-2", "b").unwrap(),
        serde_json::json!("data-2")
    );
    assert!(ctx.get_node_output("run-1", "b").is_none());
    assert!(ctx.get_node_output("run-2", "a").is_none());

    ctx.finish_run("run-1");
    assert_eq!(ctx.active_runs(), 1);
    ctx.finish_run("run-2");
    assert_eq!(ctx.active_runs(), 0);
}
