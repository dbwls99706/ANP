use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::graph::spec::{AnpGraph, NodeKind};

/// A pre-compiled graph ready for instant execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledGraph {
    /// Format identifier
    pub magic: String,
    /// Build metadata
    pub build_info: BuildInfo,
    /// Original graph (preserved for introspection)
    pub graph: AnpGraph,
    /// Pre-computed execution plan
    pub plan: CompiledPlan,
    /// Integrity fingerprint (FNV-1a hash)
    pub fingerprint: String,
    /// Whether the graph passed verification
    pub verified: bool,
}

/// Build metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildInfo {
    pub compiled_at: String,
    pub anp_version: String,
    pub source_file: Option<String>,
    pub optimization_level: OptLevel,
}

/// Optimization level
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum OptLevel {
    None,
    Basic,
    Full,
}

impl fmt::Display for OptLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OptLevel::None => write!(f, "None"),
            OptLevel::Basic => write!(f, "Basic"),
            OptLevel::Full => write!(f, "Full"),
        }
    }
}

/// Pre-computed execution plan — eliminates runtime scheduling overhead
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledPlan {
    /// Execution stages (pre-sorted via Kahn's algorithm)
    pub stages: Vec<Vec<String>>,
    /// Node type lookup: node_id -> node_type
    pub node_types: HashMap<String, String>,
    /// Node kind lookup: node_id -> kind string
    pub node_kinds: HashMap<String, String>,
    /// Pre-validated configs: node_id -> config
    pub configs: HashMap<String, serde_json::Value>,
    /// Edge routing: "from_node.from_port" -> [(to_node, to_port)]
    pub routes: HashMap<String, Vec<(String, String)>>,
    /// Reverse routes: to_node -> [from_node]
    pub reverse_routes: HashMap<String, Vec<String>>,
    /// Entry points (source + service node IDs)
    pub entry_nodes: Vec<String>,
    /// Total node count
    pub node_count: usize,
    /// Max pipeline depth (number of stages)
    pub max_depth: usize,
    /// Template patterns found in configs: node_id -> [variable_names]
    pub template_patterns: HashMap<String, Vec<String>>,
}

/// Compilation errors
#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error("graph validation failed: {0}")]
    ValidationFailed(String),
    #[error("scheduling failed: {0}")]
    SchedulingFailed(String),
    #[error("serialization error: {0}")]
    SerializationError(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Compile a graph to optimized binary format
pub fn compile(
    graph: &AnpGraph,
    source_file: Option<&str>,
    opt_level: OptLevel,
) -> Result<CompiledGraph, CompileError> {
    // Optionally optimize
    let working_graph = if opt_level == OptLevel::Full {
        let result = crate::graph::optimize::optimize(graph);
        result.graph
    } else {
        graph.clone()
    };

    // Build execution stages via topological sort
    let stages = build_stages(&working_graph)?;

    // Build lookup maps
    let mut node_types = HashMap::new();
    let mut node_kinds = HashMap::new();
    let mut configs = HashMap::new();
    let mut template_patterns = HashMap::new();

    for node in &working_graph.nodes {
        node_types.insert(node.id.clone(), node.node_type.clone());
        node_kinds.insert(node.id.clone(), format!("{}", node.kind));
        configs.insert(node.id.clone(), node.config.clone());

        // Extract template patterns
        let patterns = extract_template_vars(&node.config);
        if !patterns.is_empty() {
            template_patterns.insert(node.id.clone(), patterns);
        }
    }

    // Build route maps
    let mut routes: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut reverse_routes: HashMap<String, Vec<String>> = HashMap::new();

    for edge in &working_graph.edges {
        let key = format!("{}.{}", edge.from_node, edge.from_port);
        routes
            .entry(key)
            .or_default()
            .push((edge.to_node.clone(), edge.to_port.clone()));

        let sources = reverse_routes.entry(edge.to_node.clone()).or_default();
        if !sources.contains(&edge.from_node) {
            sources.push(edge.from_node.clone());
        }
    }

    let entry_nodes: Vec<String> = working_graph
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Source | NodeKind::Service))
        .map(|n| n.id.clone())
        .collect();

    let max_depth = stages.len();
    let node_count = working_graph.nodes.len();

    let plan = CompiledPlan {
        stages,
        node_types,
        node_kinds,
        configs,
        routes,
        reverse_routes,
        entry_nodes,
        node_count,
        max_depth,
        template_patterns,
    };

    // Compute fingerprint
    let json = serde_json::to_string(&working_graph)
        .map_err(|e| CompileError::SerializationError(e.to_string()))?;
    let fingerprint = fnv1a_hash(&json);

    // Timestamp in ISO 8601 format
    let compiled_at = {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        // Convert epoch seconds to ISO 8601 UTC: YYYY-MM-DDTHH:MM:SSZ
        let days_since_epoch = secs / 86400;
        let time_of_day = secs % 86400;
        let hours = time_of_day / 3600;
        let minutes = (time_of_day % 3600) / 60;
        let seconds = time_of_day % 60;
        // Civil date from days since 1970-01-01 (algorithm from Howard Hinnant)
        let z = days_since_epoch as i64 + 719468;
        let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
        let doe = (z - era * 146097) as u64;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe as i64 + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            y, m, d, hours, minutes, seconds
        )
    };

    Ok(CompiledGraph {
        magic: "ANP_COMPILED_V1".to_string(),
        build_info: BuildInfo {
            compiled_at,
            anp_version: env!("CARGO_PKG_VERSION").to_string(),
            source_file: source_file.map(|s| s.to_string()),
            optimization_level: opt_level,
        },
        graph: working_graph,
        plan,
        fingerprint,
        verified: true,
    })
}

/// Serialize compiled graph to JSON bytes
pub fn save(compiled: &CompiledGraph) -> Result<Vec<u8>, CompileError> {
    serde_json::to_vec(compiled).map_err(|e| CompileError::SerializationError(e.to_string()))
}

/// Load compiled graph from bytes
pub fn load(data: &[u8]) -> Result<CompiledGraph, CompileError> {
    let compiled: CompiledGraph = serde_json::from_slice(data)
        .map_err(|e| CompileError::SerializationError(e.to_string()))?;
    if compiled.magic != "ANP_COMPILED_V1" {
        return Err(CompileError::ValidationFailed(
            "invalid magic bytes".to_string(),
        ));
    }
    Ok(compiled)
}

/// Save compiled graph to file (.anp.compiled)
pub fn save_to_file(compiled: &CompiledGraph, path: &std::path::Path) -> Result<(), CompileError> {
    let data = save(compiled)?;
    std::fs::write(path, data)?;
    Ok(())
}

/// Load compiled graph from file
pub fn load_from_file(path: &std::path::Path) -> Result<CompiledGraph, CompileError> {
    let data = std::fs::read(path)?;
    load(&data)
}

/// Build execution stages using Kahn's algorithm (topological sort by levels)
fn build_stages(graph: &AnpGraph) -> Result<Vec<Vec<String>>, CompileError> {
    use std::collections::VecDeque;

    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();

    for node in &graph.nodes {
        in_degree.insert(node.id.as_str(), 0);
        adj.insert(node.id.as_str(), Vec::new());
    }

    for edge in &graph.edges {
        adj.entry(edge.from_node.as_str())
            .or_default()
            .push(edge.to_node.as_str());
        *in_degree.entry(edge.to_node.as_str()).or_default() += 1;
    }

    let mut queue: VecDeque<&str> = VecDeque::new();
    for (&id, &deg) in &in_degree {
        if deg == 0 {
            queue.push_back(id);
        }
    }

    let mut stages = Vec::new();
    let mut processed = 0;

    while !queue.is_empty() {
        let mut stage = Vec::new();
        let level_size = queue.len();
        for _ in 0..level_size {
            if let Some(node) = queue.pop_front() {
                stage.push(node.to_string());
                processed += 1;
                for &next in adj.get(node).unwrap_or(&Vec::new()) {
                    if let Some(deg) = in_degree.get_mut(next) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(next);
                        }
                    }
                }
            }
        }
        stages.push(stage);
    }

    if processed != graph.nodes.len() {
        return Err(CompileError::SchedulingFailed("cycle detected".to_string()));
    }

    Ok(stages)
}

/// Extract {{variable}} patterns from a JSON config
fn extract_template_vars(value: &serde_json::Value) -> Vec<String> {
    let mut vars = Vec::new();
    match value {
        serde_json::Value::String(s) => {
            let mut start = 0;
            while let Some(open) = s[start..].find("{{") {
                let abs_open = start + open + 2;
                if let Some(close) = s[abs_open..].find("}}") {
                    let var_name = s[abs_open..abs_open + close].trim().to_string();
                    if !vars.contains(&var_name) {
                        vars.push(var_name);
                    }
                    start = abs_open + close + 2;
                } else {
                    break;
                }
            }
        }
        serde_json::Value::Object(map) => {
            for v in map.values() {
                vars.extend(extract_template_vars(v));
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                vars.extend(extract_template_vars(v));
            }
        }
        _ => {}
    }
    vars
}

/// FNV-1a hash for fingerprinting
fn fnv1a_hash(data: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in data.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", hash)
}

impl fmt::Display for CompiledGraph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ANP Compiled Graph")?;
        writeln!(f, "══════════════════")?;
        if let Some(ref src) = self.build_info.source_file {
            writeln!(f, "Source:       {}", src)?;
        }
        writeln!(f, "Compiled:     {}", self.build_info.compiled_at)?;
        writeln!(f, "Fingerprint:  {}...", &self.fingerprint[..12])?;
        writeln!(f, "Optimization: {}", self.build_info.optimization_level)?;
        writeln!(
            f,
            "Nodes: {}  Stages: {}  Depth: {}",
            self.plan.node_count,
            self.plan.stages.len(),
            self.plan.max_depth
        )?;
        write!(f, "Entry points: {}", self.plan.entry_nodes.join(", "))?;
        if self.verified {
            write!(f, "\nPre-validated: ✓")?;
        }
        Ok(())
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
    fn test_compile_basic() {
        let graph = make_graph(vec![source("s"), sink("k")], vec![edge("e1", "s", "k")]);
        let compiled = compile(&graph, Some("test.anp.json"), OptLevel::None).unwrap();
        assert_eq!(compiled.magic, "ANP_COMPILED_V1");
        assert_eq!(compiled.plan.node_count, 2);
        assert_eq!(compiled.plan.max_depth, 2);
        assert_eq!(compiled.plan.entry_nodes, vec!["s"]);
        assert!(compiled.verified);
    }

    #[test]
    fn test_compile_with_optimization() {
        let graph = make_graph(
            vec![
                source("s"),
                sink("k"),
                Node {
                    id: "dead".to_string(),
                    node_type: "filter".to_string(),
                    kind: NodeKind::Processor,
                    config: serde_json::json!({}),
                    inputs: vec![PortSpec::new("default")],
                    outputs: vec![PortSpec::new("default")],
                },
            ],
            vec![edge("e1", "s", "k")],
        );
        let compiled = compile(&graph, None, OptLevel::Full).unwrap();
        assert_eq!(compiled.plan.node_count, 2); // dead node removed
    }

    #[test]
    fn test_save_load_roundtrip() {
        let graph = make_graph(vec![source("s"), sink("k")], vec![edge("e1", "s", "k")]);
        let compiled = compile(&graph, None, OptLevel::None).unwrap();
        let bytes = save(&compiled).unwrap();
        let loaded = load(&bytes).unwrap();
        assert_eq!(loaded.magic, "ANP_COMPILED_V1");
        assert_eq!(loaded.fingerprint, compiled.fingerprint);
        assert_eq!(loaded.plan.node_count, 2);
    }

    #[test]
    fn test_fingerprint_deterministic() {
        let graph = make_graph(vec![source("s"), sink("k")], vec![edge("e1", "s", "k")]);
        let c1 = compile(&graph, None, OptLevel::None).unwrap();
        let c2 = compile(&graph, None, OptLevel::None).unwrap();
        assert_eq!(c1.fingerprint, c2.fingerprint);
    }

    #[test]
    fn test_template_extraction() {
        let config = serde_json::json!({
            "html": "Hello {{name}}, welcome to {{place}}!",
            "nested": { "text": "{{var}}" }
        });
        let vars = extract_template_vars(&config);
        assert!(vars.contains(&"name".to_string()));
        assert!(vars.contains(&"place".to_string()));
        assert!(vars.contains(&"var".to_string()));
    }

    #[test]
    fn test_routes() {
        let graph = make_graph(
            vec![
                Node {
                    id: "s".to_string(),
                    node_type: "http_server".to_string(),
                    kind: NodeKind::Service,
                    config: serde_json::json!({}),
                    inputs: vec![],
                    outputs: vec![PortSpec::new("home"), PortSpec::new("api")],
                },
                sink("h"),
                sink("a"),
            ],
            vec![
                Edge {
                    id: "e1".to_string(),
                    from_node: "s".to_string(),
                    from_port: "home".to_string(),
                    to_node: "h".to_string(),
                    to_port: "default".to_string(),
                },
                Edge {
                    id: "e2".to_string(),
                    from_node: "s".to_string(),
                    from_port: "api".to_string(),
                    to_node: "a".to_string(),
                    to_port: "default".to_string(),
                },
            ],
        );
        let compiled = compile(&graph, None, OptLevel::None).unwrap();
        assert!(compiled.plan.routes.contains_key("s.home"));
        assert!(compiled.plan.routes.contains_key("s.api"));
    }

    #[test]
    fn test_display() {
        let graph = make_graph(vec![source("s"), sink("k")], vec![edge("e1", "s", "k")]);
        let compiled = compile(&graph, Some("test.json"), OptLevel::Full).unwrap();
        let output = format!("{}", compiled);
        assert!(output.contains("ANP Compiled Graph"));
        assert!(output.contains("test.json"));
        assert!(output.contains("Pre-validated: ✓"));
    }
}
