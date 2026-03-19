use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::graph::spec::{AnpGraph, Edge, Node};

/// Types of changes between two graphs
#[derive(Debug, Clone)]
pub enum GraphChange {
    /// A node was added
    NodeAdded {
        node_id: String,
        node_type: String,
        kind: String,
    },
    /// A node was removed
    NodeRemoved {
        node_id: String,
        node_type: String,
        kind: String,
    },
    /// A node's type changed
    NodeTypeChanged {
        node_id: String,
        old_type: String,
        new_type: String,
    },
    /// A node's kind changed
    NodeKindChanged {
        node_id: String,
        old_kind: String,
        new_kind: String,
    },
    /// A node's config changed
    NodeConfigChanged {
        node_id: String,
        field: String,
        old_value: String,
        new_value: String,
    },
    /// A node's ports changed
    NodePortsChanged {
        node_id: String,
        port_type: String,
        added: Vec<String>,
        removed: Vec<String>,
    },
    /// An edge was added
    EdgeAdded {
        from_node: String,
        from_port: String,
        to_node: String,
        to_port: String,
    },
    /// An edge was removed
    EdgeRemoved {
        from_node: String,
        from_port: String,
        to_node: String,
        to_port: String,
    },
    /// Metadata changed
    MetadataChanged {
        field: String,
        old_value: String,
        new_value: String,
    },
}

/// Complete diff between two graphs
#[derive(Debug)]
pub struct GraphDiff {
    pub changes: Vec<GraphChange>,
    pub summary: DiffSummary,
}

/// Summary statistics for a graph diff
#[derive(Debug)]
pub struct DiffSummary {
    pub nodes_added: usize,
    pub nodes_removed: usize,
    pub nodes_modified: usize,
    pub edges_added: usize,
    pub edges_removed: usize,
    pub config_changes: usize,
    pub is_identical: bool,
}

/// Compute semantic diff between two graphs.
///
/// Compares nodes by id, edges by their connection endpoints, configs
/// recursively by JSON structure, and metadata by field.
pub fn diff(old: &AnpGraph, new: &AnpGraph) -> GraphDiff {
    let mut changes: Vec<GraphChange> = Vec::new();

    // --- Metadata ---
    diff_metadata(old, new, &mut changes);

    // --- Nodes ---
    let old_nodes: HashMap<&str, &Node> = old.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let new_nodes: HashMap<&str, &Node> = new.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    let old_ids: HashSet<&str> = old_nodes.keys().copied().collect();
    let new_ids: HashSet<&str> = new_nodes.keys().copied().collect();

    // Removed nodes
    for id in old_ids.difference(&new_ids) {
        let node = old_nodes[id];
        changes.push(GraphChange::NodeRemoved {
            node_id: node.id.clone(),
            node_type: node.node_type.clone(),
            kind: node.kind.to_string(),
        });
    }

    // Added nodes
    for id in new_ids.difference(&old_ids) {
        let node = new_nodes[id];
        changes.push(GraphChange::NodeAdded {
            node_id: node.id.clone(),
            node_type: node.node_type.clone(),
            kind: node.kind.to_string(),
        });
    }

    // Modified nodes
    for id in old_ids.intersection(&new_ids) {
        let old_node = old_nodes[id];
        let new_node = new_nodes[id];
        diff_node(old_node, new_node, &mut changes);
    }

    // --- Edges ---
    diff_edges(&old.edges, &new.edges, &mut changes);

    // --- Summary ---
    let summary = compute_summary(&changes);

    GraphDiff { changes, summary }
}

fn diff_metadata(old: &AnpGraph, new: &AnpGraph, changes: &mut Vec<GraphChange>) {
    if old.metadata.name != new.metadata.name {
        changes.push(GraphChange::MetadataChanged {
            field: "name".to_string(),
            old_value: old.metadata.name.clone(),
            new_value: new.metadata.name.clone(),
        });
    }
    if old.metadata.description != new.metadata.description {
        changes.push(GraphChange::MetadataChanged {
            field: "description".to_string(),
            old_value: old.metadata.description.clone(),
            new_value: new.metadata.description.clone(),
        });
    }
}

fn diff_node(old: &Node, new: &Node, changes: &mut Vec<GraphChange>) {
    // Type change
    if old.node_type != new.node_type {
        changes.push(GraphChange::NodeTypeChanged {
            node_id: old.id.clone(),
            old_type: old.node_type.clone(),
            new_type: new.node_type.clone(),
        });
    }

    // Kind change
    if old.kind != new.kind {
        changes.push(GraphChange::NodeKindChanged {
            node_id: old.id.clone(),
            old_kind: old.kind.to_string(),
            new_kind: new.kind.to_string(),
        });
    }

    // Config change (recursive JSON diff)
    diff_json(&old.config, &new.config, &old.id, "", changes);

    // Input ports
    diff_ports(old, new, "input", changes);

    // Output ports
    diff_ports_output(old, new, changes);
}

fn diff_ports(old: &Node, new: &Node, port_type: &str, changes: &mut Vec<GraphChange>) {
    let (old_ports, new_ports) = if port_type == "input" {
        (&old.inputs, &new.inputs)
    } else {
        (&old.outputs, &new.outputs)
    };

    let old_names: HashSet<&str> = old_ports.iter().map(|p| p.name.as_str()).collect();
    let new_names: HashSet<&str> = new_ports.iter().map(|p| p.name.as_str()).collect();

    let added: Vec<String> = new_names
        .difference(&old_names)
        .map(|s| s.to_string())
        .collect();
    let removed: Vec<String> = old_names
        .difference(&new_names)
        .map(|s| s.to_string())
        .collect();

    if !added.is_empty() || !removed.is_empty() {
        changes.push(GraphChange::NodePortsChanged {
            node_id: old.id.clone(),
            port_type: port_type.to_string(),
            added,
            removed,
        });
    }
}

fn diff_ports_output(old: &Node, new: &Node, changes: &mut Vec<GraphChange>) {
    let old_names: HashSet<&str> = old.outputs.iter().map(|p| p.name.as_str()).collect();
    let new_names: HashSet<&str> = new.outputs.iter().map(|p| p.name.as_str()).collect();

    let added: Vec<String> = new_names
        .difference(&old_names)
        .map(|s| s.to_string())
        .collect();
    let removed: Vec<String> = old_names
        .difference(&new_names)
        .map(|s| s.to_string())
        .collect();

    if !added.is_empty() || !removed.is_empty() {
        changes.push(GraphChange::NodePortsChanged {
            node_id: old.id.clone(),
            port_type: "output".to_string(),
            added,
            removed,
        });
    }
}

/// Canonical key for edge identity: (from_node, from_port, to_node, to_port)
type EdgeKey = (String, String, String, String);

fn edge_key(e: &Edge) -> EdgeKey {
    (
        e.from_node.clone(),
        e.from_port.clone(),
        e.to_node.clone(),
        e.to_port.clone(),
    )
}

fn diff_edges(old_edges: &[Edge], new_edges: &[Edge], changes: &mut Vec<GraphChange>) {
    let old_set: HashSet<EdgeKey> = old_edges.iter().map(edge_key).collect();
    let new_set: HashSet<EdgeKey> = new_edges.iter().map(edge_key).collect();

    for key in old_set.difference(&new_set) {
        changes.push(GraphChange::EdgeRemoved {
            from_node: key.0.clone(),
            from_port: key.1.clone(),
            to_node: key.2.clone(),
            to_port: key.3.clone(),
        });
    }

    for key in new_set.difference(&old_set) {
        changes.push(GraphChange::EdgeAdded {
            from_node: key.0.clone(),
            from_port: key.1.clone(),
            to_node: key.2.clone(),
            to_port: key.3.clone(),
        });
    }
}

/// Maximum recursion depth for JSON diffing to prevent stack overflow
/// on deeply nested or adversarial inputs.
const MAX_DIFF_DEPTH: usize = 32;

/// Recursively diff two JSON values, emitting `NodeConfigChanged` entries
/// with dot-path notation for nested fields and bracket notation for arrays.
fn diff_json(
    old: &serde_json::Value,
    new: &serde_json::Value,
    node_id: &str,
    prefix: &str,
    changes: &mut Vec<GraphChange>,
) {
    diff_json_values(old, new, node_id, prefix, changes, 0);
}

fn diff_json_values(
    old: &serde_json::Value,
    new: &serde_json::Value,
    node_id: &str,
    prefix: &str,
    changes: &mut Vec<GraphChange>,
    depth: usize,
) {
    use serde_json::Value;

    if old == new {
        return;
    }

    if depth >= MAX_DIFF_DEPTH {
        let field = if prefix.is_empty() {
            "<root>".to_string()
        } else {
            prefix.to_string()
        };
        changes.push(GraphChange::NodeConfigChanged {
            node_id: node_id.to_string(),
            field,
            old_value: format_json_value(old),
            new_value: format_json_value(new),
        });
        return;
    }

    match (old, new) {
        (Value::Object(old_map), Value::Object(new_map)) => {
            let old_keys: HashSet<&String> = old_map.keys().collect();
            let new_keys: HashSet<&String> = new_map.keys().collect();

            // Removed keys
            for key in old_keys.difference(&new_keys) {
                let path = make_path(prefix, key);
                changes.push(GraphChange::NodeConfigChanged {
                    node_id: node_id.to_string(),
                    field: path,
                    old_value: format_json_value(&old_map[key.as_str()]),
                    new_value: "<removed>".to_string(),
                });
            }

            // Added keys
            for key in new_keys.difference(&old_keys) {
                let path = make_path(prefix, key);
                changes.push(GraphChange::NodeConfigChanged {
                    node_id: node_id.to_string(),
                    field: path,
                    old_value: "<absent>".to_string(),
                    new_value: format_json_value(&new_map[key.as_str()]),
                });
            }

            // Common keys — recurse
            for key in old_keys.intersection(&new_keys) {
                let path = make_path(prefix, key);
                diff_json_values(
                    &old_map[key.as_str()],
                    &new_map[key.as_str()],
                    node_id,
                    &path,
                    changes,
                    depth + 1,
                );
            }
        }
        (Value::Array(old_arr), Value::Array(new_arr)) => {
            let max_len = old_arr.len().max(new_arr.len());
            for i in 0..max_len {
                let path = if prefix.is_empty() {
                    format!("[{i}]")
                } else {
                    format!("{prefix}[{i}]")
                };
                match (old_arr.get(i), new_arr.get(i)) {
                    (Some(o), Some(n)) => {
                        diff_json_values(o, n, node_id, &path, changes, depth + 1);
                    }
                    (Some(o), None) => {
                        changes.push(GraphChange::NodeConfigChanged {
                            node_id: node_id.to_string(),
                            field: path,
                            old_value: format_json_value(o),
                            new_value: "<removed>".to_string(),
                        });
                    }
                    (None, Some(n)) => {
                        changes.push(GraphChange::NodeConfigChanged {
                            node_id: node_id.to_string(),
                            field: path,
                            old_value: "<absent>".to_string(),
                            new_value: format_json_value(n),
                        });
                    }
                    (None, None) => unreachable!(),
                }
            }
        }
        _ => {
            // Leaf or type mismatch — report as changed
            let field = if prefix.is_empty() {
                "<root>".to_string()
            } else {
                prefix.to_string()
            };
            changes.push(GraphChange::NodeConfigChanged {
                node_id: node_id.to_string(),
                field,
                old_value: format_json_value(old),
                new_value: format_json_value(new),
            });
        }
    }
}

fn make_path(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_string()
    } else {
        format!("{prefix}.{key}")
    }
}

fn format_json_value(v: &serde_json::Value) -> String {
    use serde_json::Value;
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("\"{s}\""),
        Value::Array(_) | Value::Object(_) => {
            serde_json::to_string(v).unwrap_or_else(|_| format!("{v:?}"))
        }
    }
}

fn compute_summary(changes: &[GraphChange]) -> DiffSummary {
    let mut nodes_added = 0usize;
    let mut nodes_removed = 0usize;
    let mut modified_node_ids: HashSet<String> = HashSet::new();
    let mut edges_added = 0usize;
    let mut edges_removed = 0usize;
    let mut config_changes = 0usize;

    for change in changes {
        match change {
            GraphChange::NodeAdded { .. } => nodes_added += 1,
            GraphChange::NodeRemoved { .. } => nodes_removed += 1,
            GraphChange::NodeTypeChanged { node_id, .. }
            | GraphChange::NodeKindChanged { node_id, .. } => {
                modified_node_ids.insert(node_id.clone());
            }
            GraphChange::NodeConfigChanged { node_id, .. } => {
                modified_node_ids.insert(node_id.clone());
                config_changes += 1;
            }
            GraphChange::NodePortsChanged { node_id, .. } => {
                modified_node_ids.insert(node_id.clone());
            }
            GraphChange::EdgeAdded { .. } => edges_added += 1,
            GraphChange::EdgeRemoved { .. } => edges_removed += 1,
            GraphChange::MetadataChanged { .. } => {}
        }
    }

    DiffSummary {
        nodes_added,
        nodes_removed,
        nodes_modified: modified_node_ids.len(),
        edges_added,
        edges_removed,
        config_changes,
        is_identical: changes.is_empty(),
    }
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

impl fmt::Display for GraphDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ANP Graph Diff")?;
        writeln!(f, "\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}")?;

        if self.summary.is_identical {
            writeln!(f, "  (no changes)")?;
            return Ok(());
        }

        // Group config changes by node for compact display
        let mut config_by_node: HashMap<String, Vec<(String, String, String)>> = HashMap::new();
        // Track which nodes have config-only changes so we can display them
        // after the structural changes.
        let mut displayed_nodes: HashSet<String> = HashSet::new();

        for change in &self.changes {
            match change {
                GraphChange::MetadataChanged {
                    field,
                    old_value,
                    new_value,
                } => {
                    writeln!(f, "  ~ Metadata '{field}': {old_value} -> {new_value}")?;
                }
                GraphChange::NodeAdded {
                    node_id,
                    node_type,
                    kind,
                } => {
                    writeln!(f, "  + Node '{node_id}' ({node_type}, {kind})")?;
                    displayed_nodes.insert(node_id.clone());
                }
                GraphChange::NodeRemoved {
                    node_id,
                    node_type,
                    kind,
                } => {
                    writeln!(f, "  - Node '{node_id}' ({node_type}, {kind})")?;
                    displayed_nodes.insert(node_id.clone());
                }
                GraphChange::NodeTypeChanged {
                    node_id,
                    old_type,
                    new_type,
                } => {
                    writeln!(
                        f,
                        "  ~ Node '{node_id}' type changed: {old_type} -> {new_type}"
                    )?;
                    displayed_nodes.insert(node_id.clone());
                }
                GraphChange::NodeKindChanged {
                    node_id,
                    old_kind,
                    new_kind,
                } => {
                    writeln!(
                        f,
                        "  ~ Node '{node_id}' kind changed: {old_kind} -> {new_kind}"
                    )?;
                    displayed_nodes.insert(node_id.clone());
                }
                GraphChange::NodeConfigChanged {
                    node_id,
                    field,
                    old_value,
                    new_value,
                } => {
                    config_by_node.entry(node_id.clone()).or_default().push((
                        field.clone(),
                        old_value.clone(),
                        new_value.clone(),
                    ));
                }
                GraphChange::NodePortsChanged {
                    node_id,
                    port_type,
                    added,
                    removed,
                } => {
                    write!(f, "  ~ Node '{node_id}' {port_type} ports changed:")?;
                    if !added.is_empty() {
                        write!(f, " +{}", added.join(", +"))?;
                    }
                    if !removed.is_empty() {
                        write!(f, " -{}", removed.join(", -"))?;
                    }
                    writeln!(f)?;
                    displayed_nodes.insert(node_id.clone());
                }
                GraphChange::EdgeAdded {
                    from_node,
                    from_port,
                    to_node,
                    to_port,
                } => {
                    writeln!(
                        f,
                        "  + Edge: {from_node}.{from_port} -> {to_node}.{to_port}"
                    )?;
                }
                GraphChange::EdgeRemoved {
                    from_node,
                    from_port,
                    to_node,
                    to_port,
                } => {
                    writeln!(
                        f,
                        "  - Edge: {from_node}.{from_port} -> {to_node}.{to_port}"
                    )?;
                }
            }
        }

        // Print grouped config changes
        // Sort keys for deterministic output
        let mut node_ids: Vec<&String> = config_by_node.keys().collect();
        node_ids.sort();
        for node_id in node_ids {
            let fields = &config_by_node[node_id];
            writeln!(f, "  ~ Node '{node_id}' config changed:")?;
            for (field, old_val, new_val) in fields {
                writeln!(f, "      {field}: {old_val} -> {new_val}")?;
            }
        }

        // Summary line
        writeln!(f)?;
        write!(f, "Summary:")?;
        let s = &self.summary;
        let mut parts: Vec<String> = Vec::new();
        if s.nodes_added > 0 {
            parts.push(format!("+{} node{}", s.nodes_added, plural(s.nodes_added)));
        }
        if s.nodes_removed > 0 {
            parts.push(format!(
                "-{} node{}",
                s.nodes_removed,
                plural(s.nodes_removed)
            ));
        }
        if s.nodes_modified > 0 {
            parts.push(format!("~{} modified", s.nodes_modified));
        }
        if s.edges_added > 0 {
            parts.push(format!("+{} edge{}", s.edges_added, plural(s.edges_added)));
        }
        if s.edges_removed > 0 {
            parts.push(format!(
                "-{} edge{}",
                s.edges_removed,
                plural(s.edges_removed)
            ));
        }
        writeln!(f, " {}", parts.join(", "))?;

        Ok(())
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::spec::*;

    fn make_graph(name: &str, desc: &str, nodes: Vec<Node>, edges: Vec<Edge>) -> AnpGraph {
        AnpGraph {
            version: GraphVersion::V0_1,
            metadata: GraphMetadata {
                name: name.to_string(),
                description: desc.to_string(),
            },
            nodes,
            edges,
        }
    }

    fn make_node(id: &str, node_type: &str, kind: NodeKind) -> Node {
        Node {
            id: id.to_string(),
            node_type: node_type.to_string(),
            kind,
            config: serde_json::json!({}),
            inputs: vec![],
            outputs: vec![],
        }
    }

    fn make_node_with_config(
        id: &str,
        node_type: &str,
        kind: NodeKind,
        config: serde_json::Value,
    ) -> Node {
        Node {
            id: id.to_string(),
            node_type: node_type.to_string(),
            kind,
            config,
            inputs: vec![],
            outputs: vec![],
        }
    }

    fn make_edge(id: &str, from: &str, from_port: &str, to: &str, to_port: &str) -> Edge {
        Edge {
            id: id.to_string(),
            from_node: from.to_string(),
            from_port: from_port.to_string(),
            to_node: to.to_string(),
            to_port: to_port.to_string(),
        }
    }

    // -----------------------------------------------------------------------
    // Identical graphs
    // -----------------------------------------------------------------------
    #[test]
    fn test_identical_graphs() {
        let g = make_graph(
            "test",
            "desc",
            vec![make_node("a", "console", NodeKind::Sink)],
            vec![],
        );
        let d = diff(&g, &g);
        assert!(d.summary.is_identical);
        assert!(d.changes.is_empty());
        assert_eq!(d.summary.nodes_added, 0);
        assert_eq!(d.summary.nodes_removed, 0);
        assert_eq!(d.summary.nodes_modified, 0);
    }

    // -----------------------------------------------------------------------
    // Node additions
    // -----------------------------------------------------------------------
    #[test]
    fn test_node_added() {
        let old = make_graph("g", "", vec![], vec![]);
        let new = make_graph(
            "g",
            "",
            vec![make_node("cache", "cache", NodeKind::Processor)],
            vec![],
        );
        let d = diff(&old, &new);
        assert_eq!(d.summary.nodes_added, 1);
        assert_eq!(d.summary.nodes_removed, 0);
        assert!(!d.summary.is_identical);

        let added = d
            .changes
            .iter()
            .find(|c| matches!(c, GraphChange::NodeAdded { .. }));
        assert!(added.is_some());
        if let Some(GraphChange::NodeAdded {
            node_id,
            node_type,
            kind,
        }) = added
        {
            assert_eq!(node_id, "cache");
            assert_eq!(node_type, "cache");
            assert_eq!(kind, "processor");
        }
    }

    // -----------------------------------------------------------------------
    // Node removals
    // -----------------------------------------------------------------------
    #[test]
    fn test_node_removed() {
        let old = make_graph(
            "g",
            "",
            vec![make_node("old_filter", "filter", NodeKind::Processor)],
            vec![],
        );
        let new = make_graph("g", "", vec![], vec![]);
        let d = diff(&old, &new);
        assert_eq!(d.summary.nodes_removed, 1);
        assert_eq!(d.summary.nodes_added, 0);

        let removed = d
            .changes
            .iter()
            .find(|c| matches!(c, GraphChange::NodeRemoved { .. }));
        assert!(removed.is_some());
    }

    // -----------------------------------------------------------------------
    // Node type changed
    // -----------------------------------------------------------------------
    #[test]
    fn test_node_type_changed() {
        let old = make_graph(
            "g",
            "",
            vec![make_node("n1", "http_client", NodeKind::Processor)],
            vec![],
        );
        let new = make_graph(
            "g",
            "",
            vec![make_node("n1", "http_server", NodeKind::Service)],
            vec![],
        );
        let d = diff(&old, &new);
        assert_eq!(d.summary.nodes_modified, 1);

        let type_change = d
            .changes
            .iter()
            .find(|c| matches!(c, GraphChange::NodeTypeChanged { .. }));
        assert!(type_change.is_some());
        if let Some(GraphChange::NodeTypeChanged {
            old_type, new_type, ..
        }) = type_change
        {
            assert_eq!(old_type, "http_client");
            assert_eq!(new_type, "http_server");
        }
    }

    // -----------------------------------------------------------------------
    // Node kind changed
    // -----------------------------------------------------------------------
    #[test]
    fn test_node_kind_changed() {
        let old = make_graph(
            "g",
            "",
            vec![make_node("n1", "transform", NodeKind::Processor)],
            vec![],
        );
        let new = make_graph(
            "g",
            "",
            vec![make_node("n1", "transform", NodeKind::Sink)],
            vec![],
        );
        let d = diff(&old, &new);

        let kind_change = d
            .changes
            .iter()
            .find(|c| matches!(c, GraphChange::NodeKindChanged { .. }));
        assert!(kind_change.is_some());
        if let Some(GraphChange::NodeKindChanged {
            old_kind, new_kind, ..
        }) = kind_change
        {
            assert_eq!(old_kind, "processor");
            assert_eq!(new_kind, "sink");
        }
    }

    // -----------------------------------------------------------------------
    // Config changes — flat
    // -----------------------------------------------------------------------
    #[test]
    fn test_config_changed_flat() {
        let old = make_graph(
            "g",
            "",
            vec![make_node_with_config(
                "server",
                "http_server",
                NodeKind::Service,
                serde_json::json!({"port": 3000, "host": "localhost"}),
            )],
            vec![],
        );
        let new = make_graph(
            "g",
            "",
            vec![make_node_with_config(
                "server",
                "http_server",
                NodeKind::Service,
                serde_json::json!({"port": 8080, "host": "localhost"}),
            )],
            vec![],
        );
        let d = diff(&old, &new);
        assert_eq!(d.summary.config_changes, 1);
        assert_eq!(d.summary.nodes_modified, 1);

        let cfg = d
            .changes
            .iter()
            .find(|c| matches!(c, GraphChange::NodeConfigChanged { field, .. } if field == "port"));
        assert!(cfg.is_some());
        if let Some(GraphChange::NodeConfigChanged {
            old_value,
            new_value,
            ..
        }) = cfg
        {
            assert_eq!(old_value, "3000");
            assert_eq!(new_value, "8080");
        }
    }

    // -----------------------------------------------------------------------
    // Config changes — nested JSON
    // -----------------------------------------------------------------------
    #[test]
    fn test_config_changed_nested() {
        let old = make_graph(
            "g",
            "",
            vec![make_node_with_config(
                "router",
                "http_server",
                NodeKind::Service,
                serde_json::json!({
                    "routes": [
                        {"path": "/api", "method": "GET"},
                        {"path": "/health", "method": "GET"}
                    ]
                }),
            )],
            vec![],
        );
        let new = make_graph(
            "g",
            "",
            vec![make_node_with_config(
                "router",
                "http_server",
                NodeKind::Service,
                serde_json::json!({
                    "routes": [
                        {"path": "/v2/api", "method": "GET"},
                        {"path": "/health", "method": "GET"}
                    ]
                }),
            )],
            vec![],
        );
        let d = diff(&old, &new);
        assert_eq!(d.summary.config_changes, 1);

        let cfg = d.changes.iter().find(|c| {
            matches!(c, GraphChange::NodeConfigChanged { field, .. } if field == "routes[0].path")
        });
        assert!(cfg.is_some());
        if let Some(GraphChange::NodeConfigChanged {
            old_value,
            new_value,
            ..
        }) = cfg
        {
            assert_eq!(old_value, "\"/api\"");
            assert_eq!(new_value, "\"/v2/api\"");
        }
    }

    // -----------------------------------------------------------------------
    // Config — key added / removed
    // -----------------------------------------------------------------------
    #[test]
    fn test_config_key_added_removed() {
        let old = make_graph(
            "g",
            "",
            vec![make_node_with_config(
                "n",
                "transform",
                NodeKind::Processor,
                serde_json::json!({"a": 1, "b": 2}),
            )],
            vec![],
        );
        let new = make_graph(
            "g",
            "",
            vec![make_node_with_config(
                "n",
                "transform",
                NodeKind::Processor,
                serde_json::json!({"b": 2, "c": 3}),
            )],
            vec![],
        );
        let d = diff(&old, &new);
        // "a" removed, "c" added
        assert_eq!(d.summary.config_changes, 2);

        let removed = d.changes.iter().find(|c| {
            matches!(c, GraphChange::NodeConfigChanged { field, new_value, .. }
                if field == "a" && new_value == "<removed>")
        });
        assert!(removed.is_some());

        let added = d.changes.iter().find(|c| {
            matches!(c, GraphChange::NodeConfigChanged { field, old_value, .. }
                if field == "c" && old_value == "<absent>")
        });
        assert!(added.is_some());
    }

    // -----------------------------------------------------------------------
    // Config — array length change
    // -----------------------------------------------------------------------
    #[test]
    fn test_config_array_length_change() {
        let old = make_graph(
            "g",
            "",
            vec![make_node_with_config(
                "n",
                "transform",
                NodeKind::Processor,
                serde_json::json!({"items": [1, 2]}),
            )],
            vec![],
        );
        let new = make_graph(
            "g",
            "",
            vec![make_node_with_config(
                "n",
                "transform",
                NodeKind::Processor,
                serde_json::json!({"items": [1, 2, 3]}),
            )],
            vec![],
        );
        let d = diff(&old, &new);
        assert_eq!(d.summary.config_changes, 1);

        let added = d.changes.iter().find(|c| {
            matches!(c, GraphChange::NodeConfigChanged { field, .. }
                if field == "items[2]")
        });
        assert!(added.is_some());
    }

    // -----------------------------------------------------------------------
    // Edge additions and removals
    // -----------------------------------------------------------------------
    #[test]
    fn test_edge_added() {
        let nodes = vec![
            make_node("a", "transform", NodeKind::Processor),
            make_node("b", "console", NodeKind::Sink),
        ];
        let old = make_graph("g", "", nodes.clone(), vec![]);
        let new = make_graph(
            "g",
            "",
            nodes,
            vec![make_edge("e1", "a", "default", "b", "default")],
        );
        let d = diff(&old, &new);
        assert_eq!(d.summary.edges_added, 1);
        assert_eq!(d.summary.edges_removed, 0);
    }

    #[test]
    fn test_edge_removed() {
        let nodes = vec![
            make_node("a", "transform", NodeKind::Processor),
            make_node("b", "console", NodeKind::Sink),
        ];
        let old = make_graph(
            "g",
            "",
            nodes.clone(),
            vec![make_edge("e1", "a", "default", "b", "default")],
        );
        let new = make_graph("g", "", nodes, vec![]);
        let d = diff(&old, &new);
        assert_eq!(d.summary.edges_removed, 1);
        assert_eq!(d.summary.edges_added, 0);
    }

    #[test]
    fn test_edge_replaced() {
        let nodes = vec![
            make_node("a", "transform", NodeKind::Processor),
            make_node("b", "console", NodeKind::Sink),
            make_node("c", "log", NodeKind::Sink),
        ];
        let old = make_graph(
            "g",
            "",
            nodes.clone(),
            vec![make_edge("e1", "a", "default", "b", "default")],
        );
        let new = make_graph(
            "g",
            "",
            nodes,
            vec![make_edge("e2", "a", "default", "c", "default")],
        );
        let d = diff(&old, &new);
        assert_eq!(d.summary.edges_added, 1);
        assert_eq!(d.summary.edges_removed, 1);
    }

    // -----------------------------------------------------------------------
    // Port changes
    // -----------------------------------------------------------------------
    #[test]
    fn test_port_changes() {
        let mut old_node = make_node("n", "transform", NodeKind::Processor);
        old_node.inputs = vec![PortSpec::new("in1"), PortSpec::new("in2")];
        old_node.outputs = vec![PortSpec::new("out1")];

        let mut new_node = make_node("n", "transform", NodeKind::Processor);
        new_node.inputs = vec![PortSpec::new("in1"), PortSpec::new("in3")];
        new_node.outputs = vec![PortSpec::new("out1"), PortSpec::new("out2")];

        let old = make_graph("g", "", vec![old_node], vec![]);
        let new = make_graph("g", "", vec![new_node], vec![]);
        let d = diff(&old, &new);

        // Input ports: in2 removed, in3 added
        let input_change = d.changes.iter().find(|c| {
            matches!(c, GraphChange::NodePortsChanged { port_type, .. } if port_type == "input")
        });
        assert!(input_change.is_some());
        if let Some(GraphChange::NodePortsChanged { added, removed, .. }) = input_change {
            assert!(added.contains(&"in3".to_string()));
            assert!(removed.contains(&"in2".to_string()));
        }

        // Output ports: out2 added
        let output_change = d.changes.iter().find(|c| {
            matches!(c, GraphChange::NodePortsChanged { port_type, .. } if port_type == "output")
        });
        assert!(output_change.is_some());
        if let Some(GraphChange::NodePortsChanged { added, removed, .. }) = output_change {
            assert!(added.contains(&"out2".to_string()));
            assert!(removed.is_empty());
        }
    }

    // -----------------------------------------------------------------------
    // Metadata changes
    // -----------------------------------------------------------------------
    #[test]
    fn test_metadata_changed() {
        let old = make_graph("old_name", "old desc", vec![], vec![]);
        let new = make_graph("new_name", "new desc", vec![], vec![]);
        let d = diff(&old, &new);

        let name_change = d
            .changes
            .iter()
            .find(|c| matches!(c, GraphChange::MetadataChanged { field, .. } if field == "name"));
        assert!(name_change.is_some());

        let desc_change = d.changes.iter().find(
            |c| matches!(c, GraphChange::MetadataChanged { field, .. } if field == "description"),
        );
        assert!(desc_change.is_some());
    }

    // -----------------------------------------------------------------------
    // Display output
    // -----------------------------------------------------------------------
    #[test]
    fn test_display_identical() {
        let g = make_graph("g", "", vec![], vec![]);
        let d = diff(&g, &g);
        let output = format!("{d}");
        assert!(output.contains("no changes"));
    }

    #[test]
    fn test_display_mixed_changes() {
        let old = make_graph(
            "g",
            "",
            vec![
                make_node_with_config(
                    "server",
                    "http_server",
                    NodeKind::Service,
                    serde_json::json!({"port": 3000}),
                ),
                make_node("old_filter", "filter", NodeKind::Processor),
            ],
            vec![make_edge("e1", "server", "filter", "old_filter", "default")],
        );
        let new = make_graph(
            "g",
            "",
            vec![
                make_node_with_config(
                    "server",
                    "http_server",
                    NodeKind::Service,
                    serde_json::json!({"port": 8080}),
                ),
                make_node("cache", "cache", NodeKind::Processor),
            ],
            vec![make_edge("e2", "server", "cached", "cache", "default")],
        );
        let d = diff(&old, &new);
        let output = format!("{d}");
        assert!(output.contains("ANP Graph Diff"));
        assert!(output.contains("+ Node 'cache'"));
        assert!(output.contains("- Node 'old_filter'"));
        assert!(output.contains("config changed"));
        assert!(output.contains("port"));
        assert!(output.contains("3000"));
        assert!(output.contains("8080"));
        assert!(output.contains("Summary:"));
    }

    // -----------------------------------------------------------------------
    // Comprehensive scenario
    // -----------------------------------------------------------------------
    #[test]
    fn test_comprehensive_diff() {
        let old = make_graph(
            "api",
            "API server",
            vec![
                make_node_with_config(
                    "server",
                    "http_server",
                    NodeKind::Service,
                    serde_json::json!({"port": 3000, "routes": [{"path": "/api"}]}),
                ),
                make_node("logger", "log", NodeKind::Sink),
                make_node("old_auth", "jwt", NodeKind::Processor),
            ],
            vec![
                make_edge("e1", "server", "default", "logger", "default"),
                make_edge("e2", "server", "auth", "old_auth", "default"),
            ],
        );

        let new = make_graph(
            "api-v2",
            "API server v2",
            vec![
                make_node_with_config(
                    "server",
                    "http_server",
                    NodeKind::Service,
                    serde_json::json!({"port": 8080, "routes": [{"path": "/v2/api"}]}),
                ),
                make_node("logger", "log", NodeKind::Sink),
                make_node("new_auth", "oauth", NodeKind::Processor),
                make_node("cache", "cache", NodeKind::Processor),
            ],
            vec![
                make_edge("e1", "server", "default", "logger", "default"),
                make_edge("e3", "server", "auth", "new_auth", "default"),
                make_edge("e4", "new_auth", "default", "cache", "default"),
            ],
        );

        let d = diff(&old, &new);

        // Metadata: name and description changed
        assert!(d.changes.iter().any(|c| matches!(c,
            GraphChange::MetadataChanged { field, .. } if field == "name"
        )));
        assert!(d.changes.iter().any(|c| matches!(c,
            GraphChange::MetadataChanged { field, .. } if field == "description"
        )));

        // Nodes: old_auth removed, new_auth + cache added
        assert_eq!(d.summary.nodes_added, 2);
        assert_eq!(d.summary.nodes_removed, 1);

        // Config: port changed, routes[0].path changed
        assert!(d.summary.config_changes >= 2);

        // Edges: e2 removed, e3 + e4 added
        assert_eq!(d.summary.edges_added, 2);
        assert_eq!(d.summary.edges_removed, 1);

        assert!(!d.summary.is_identical);

        // Verify display doesn't panic
        let _output = format!("{d}");
    }
}
