use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

use super::spec::AnpGraph;

// ─── Port Type ──────────────────────────────────────────────────────────────

/// Data types that can flow through graph ports.
///
/// ANP graphs are dynamically wired but statically checkable: every port
/// declares the shape of JSON it produces or consumes, and the type checker
/// verifies that every edge connects compatible ports *before* execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum PortType {
    /// Top type — compatible with every other type.
    Any,
    /// JSON string.
    String,
    /// JSON number (integer or float).
    Number,
    /// JSON boolean.
    Boolean,
    /// JSON null.
    Null,
    /// JSON object, optionally carrying a field‐level schema.
    Object(Option<HashMap<std::string::String, PortType>>),
    /// JSON array, optionally carrying the element type.
    Array(Option<Box<PortType>>),
    /// HTML content (subtype of String).
    Html,
    /// Binary data transported as base64‐encoded string.
    Binary,
    /// Union (sum) of multiple types.
    Union(Vec<PortType>),
}

impl PortType {
    /// Check if `self` (the producer / output type) is compatible with
    /// `other` (the consumer / input type).
    ///
    /// Compatibility rules (directed — "can this output flow into that input"):
    ///
    /// 1. `Any` on either side is always compatible.
    /// 2. Identical primitive types are compatible.
    /// 3. `Object(None)` (untyped object) is compatible with any `Object(…)`.
    /// 4. `Object(Some(a))` is compatible with `Object(Some(b))` when every
    ///    field required by `b` exists in `a` with a compatible type
    ///    (structural subtyping / width subtyping).
    /// 5. `Array(None)` is compatible with any `Array(…)` and vice‐versa.
    /// 6. `Array(Some(a))` ↔ `Array(Some(b))` when element types are compatible.
    /// 7. `Html` is a subtype of `String` — an `Html` output can flow into a
    ///    `String` input (but not the other way around by default).
    /// 8. A `Union` is compatible with `other` if *any* variant is compatible.
    ///    Dually, `self` is compatible with `Union(variants)` if `self` is
    ///    compatible with *any* variant.
    pub fn is_compatible_with(&self, other: &PortType) -> bool {
        // Rule 1 — Any is the universal adapter.
        if matches!(self, PortType::Any) || matches!(other, PortType::Any) {
            return true;
        }

        // Rule 8 — Union on either side.
        if let PortType::Union(variants) = self {
            return variants.iter().any(|v| v.is_compatible_with(other));
        }
        if let PortType::Union(variants) = other {
            return variants.iter().any(|v| self.is_compatible_with(v));
        }

        match (self, other) {
            // Rule 2 — identical primitives.
            (PortType::String, PortType::String)
            | (PortType::Number, PortType::Number)
            | (PortType::Boolean, PortType::Boolean)
            | (PortType::Null, PortType::Null)
            | (PortType::Binary, PortType::Binary)
            | (PortType::Html, PortType::Html) => true,

            // Rule 7 — Html is a subtype of String.
            (PortType::Html, PortType::String) => true,

            // Rule 3 + 4 — Object compatibility.
            (PortType::Object(a), PortType::Object(b)) => match (a, b) {
                (_, None) | (None, _) => true,
                (Some(a_fields), Some(b_fields)) => {
                    // Width subtyping: every field the consumer requires must
                    // exist in the producer with a compatible type.
                    b_fields.iter().all(|(key, b_ty)| {
                        a_fields
                            .get(key)
                            .is_some_and(|a_ty| a_ty.is_compatible_with(b_ty))
                    })
                }
            },

            // Rule 5 + 6 — Array compatibility.
            (PortType::Array(a), PortType::Array(b)) => match (a, b) {
                (_, None) | (None, _) => true,
                (Some(a_elem), Some(b_elem)) => a_elem.is_compatible_with(b_elem),
            },

            // Everything else is incompatible.
            _ => false,
        }
    }
}

impl fmt::Display for PortType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PortType::Any => write!(f, "any"),
            PortType::String => write!(f, "string"),
            PortType::Number => write!(f, "number"),
            PortType::Boolean => write!(f, "boolean"),
            PortType::Null => write!(f, "null"),
            PortType::Html => write!(f, "html"),
            PortType::Binary => write!(f, "binary"),
            PortType::Object(None) => write!(f, "object"),
            PortType::Object(Some(fields)) => {
                write!(f, "object{{")?;
                let mut sorted: Vec<_> = fields.iter().collect();
                sorted.sort_by_key(|(k, _)| (*k).clone());
                for (i, (key, ty)) in sorted.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", key, ty)?;
                }
                write!(f, "}}")
            }
            PortType::Array(None) => write!(f, "array"),
            PortType::Array(Some(elem)) => write!(f, "array<{}>", elem),
            PortType::Union(variants) => {
                for (i, v) in variants.iter().enumerate() {
                    if i > 0 {
                        write!(f, " | ")?;
                    }
                    write!(f, "{}", v)?;
                }
                Ok(())
            }
        }
    }
}

// ─── Node Type Signatures ───────────────────────────────────────────────────

/// The type‐level signature of a node: the type of every named port.
#[derive(Debug, Clone)]
pub struct NodeTypeSignature {
    pub inputs: HashMap<String, PortType>,
    pub outputs: HashMap<String, PortType>,
}

/// Return the canonical type signature for a built‐in node type.
///
/// Unknown node types get a permissive default (`Any` → `Any`).
pub fn get_node_signature(node_type: &str) -> NodeTypeSignature {
    match node_type {
        "file_read" => NodeTypeSignature {
            inputs: HashMap::new(),
            outputs: HashMap::from([("default".into(), PortType::String)]),
        },
        "console" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Any)]),
            outputs: HashMap::new(),
        },
        "http_server" => NodeTypeSignature {
            inputs: HashMap::new(),
            outputs: HashMap::from([("default".into(), PortType::Object(None))]),
        },
        "transform" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Any)]),
            outputs: HashMap::from([("default".into(), PortType::Object(None))]),
        },
        "filter" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Object(None))]),
            outputs: HashMap::from([("default".into(), PortType::Object(None))]),
        },
        "template" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Object(None))]),
            outputs: HashMap::from([("default".into(), PortType::String)]),
        },
        "html_template" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Object(None))]),
            outputs: HashMap::from([("default".into(), PortType::Html)]),
        },
        "llm_call" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Any)]),
            outputs: HashMap::from([("default".into(), PortType::String)]),
        },
        "database" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Object(None))]),
            outputs: HashMap::from([("default".into(), PortType::Object(None))]),
        },
        "json_parse" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::String)]),
            outputs: HashMap::from([("default".into(), PortType::Object(None))]),
        },
        "csv" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::String)]),
            outputs: HashMap::from([(
                "default".into(),
                PortType::Array(Some(Box::new(PortType::Object(None)))),
            )]),
        },
        "log" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Any)]),
            outputs: HashMap::new(),
        },
        "file_write" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::String)]),
            outputs: HashMap::new(),
        },
        "merge" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Object(None))]),
            outputs: HashMap::from([("default".into(), PortType::Object(None))]),
        },
        "conditional" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Any)]),
            outputs: HashMap::from([
                ("true".into(), PortType::Any),
                ("false".into(), PortType::Any),
            ]),
        },
        "foreach" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Array(None))]),
            outputs: HashMap::from([("default".into(), PortType::Any)]),
        },
        "validate" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Any)]),
            outputs: HashMap::from([
                ("valid".into(), PortType::Any),
                ("invalid".into(), PortType::Any),
            ]),
        },
        "base64" => NodeTypeSignature {
            inputs: HashMap::from([(
                "default".into(),
                PortType::Union(vec![PortType::String, PortType::Binary]),
            )]),
            outputs: HashMap::from([("default".into(), PortType::String)]),
        },
        "env" => NodeTypeSignature {
            inputs: HashMap::new(),
            outputs: HashMap::from([("default".into(), PortType::String)]),
        },
        // Default: permissive signature for unknown nodes.
        _ => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Any)]),
            outputs: HashMap::from([("default".into(), PortType::Any)]),
        },
    }
}

// ─── Type Check Results ─────────────────────────────────────────────────────

/// The result of type‐checking an entire graph.
#[derive(Debug, Clone)]
pub struct TypeCheckResult {
    /// Hard errors — connections that are provably unsound.
    pub errors: Vec<TypeCheckError>,
    /// Soft warnings (e.g. nodes with no outgoing edges).
    pub warnings: Vec<TypeCheckWarning>,
    /// `true` iff `errors` is empty.
    pub is_sound: bool,
}

/// A single type‐incompatibility on an edge.
#[derive(Debug, Clone)]
pub struct TypeCheckError {
    pub edge_id: String,
    pub from_node: String,
    pub from_port: String,
    pub from_type: PortType,
    pub to_node: String,
    pub to_port: String,
    pub to_type: PortType,
    pub message: String,
}

/// A non‐fatal observation about a node or edge.
#[derive(Debug, Clone)]
pub struct TypeCheckWarning {
    pub node_id: String,
    pub message: String,
}

impl fmt::Display for TypeCheckResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ANP Type Check")?;
        writeln!(f, "\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}")?;

        // Collect all edge results: we need to render good edges too, so we
        // gather them during check and stash them alongside errors.  However,
        // our public API only stores errors.  For Display we re‐derive the
        // pass/fail per edge by checking membership in the error set.
        //
        // Since we don't keep the full list of edges here (TypeCheckResult is
        // post‐hoc), we print errors and warnings only.

        for err in &self.errors {
            writeln!(
                f,
                "\u{2717} {}.{} \u{2192} {}.{}  ({} \u{2192} {})  INCOMPATIBLE",
                err.from_node, err.from_port, err.to_node, err.to_port, err.from_type, err.to_type,
            )?;
        }

        for warn in &self.warnings {
            writeln!(f, "  warning [{}]: {}", warn.node_id, warn.message)?;
        }

        writeln!(f)?;
        write!(
            f,
            "Result: {} type error{}, {} warning{}",
            self.errors.len(),
            if self.errors.len() == 1 { "" } else { "s" },
            self.warnings.len(),
            if self.warnings.len() == 1 { "" } else { "s" },
        )
    }
}

impl fmt::Display for TypeCheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}.{} ({}) -> {}.{} ({}): {}",
            self.from_node,
            self.from_port,
            self.from_type,
            self.to_node,
            self.to_port,
            self.to_type,
            self.message,
        )
    }
}

// ─── Type Checker ───────────────────────────────────────────────────────────

/// Type‐check every edge in `graph` and return a complete report.
///
/// For each edge we resolve:
///   1. The output port type of the source node.
///   2. The input port type of the destination node.
///
/// Port types come from `get_node_signature`. If an edge references a port
/// name that is not in the signature we fall back to `Any` (the node may
/// create dynamic ports at runtime — we emit a warning instead of an error).
pub fn type_check(graph: &AnpGraph) -> TypeCheckResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Build node‐id → node lookup.
    let node_map: HashMap<&str, &super::spec::Node> =
        graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    // Check for sink nodes that have no incoming edges.
    let nodes_with_incoming: std::collections::HashSet<&str> =
        graph.edges.iter().map(|e| e.to_node.as_str()).collect();

    for node in &graph.nodes {
        if node.kind == super::spec::NodeKind::Sink
            && !nodes_with_incoming.contains(node.id.as_str())
        {
            warnings.push(TypeCheckWarning {
                node_id: node.id.clone(),
                message: "Sink node has no incoming edges".into(),
            });
        }
    }

    // Check for source nodes that have no outgoing edges.
    let nodes_with_outgoing: std::collections::HashSet<&str> =
        graph.edges.iter().map(|e| e.from_node.as_str()).collect();

    for node in &graph.nodes {
        if node.kind == super::spec::NodeKind::Source
            && !nodes_with_outgoing.contains(node.id.as_str())
        {
            warnings.push(TypeCheckWarning {
                node_id: node.id.clone(),
                message: "Source node has no outgoing edges".into(),
            });
        }
    }

    for edge in &graph.edges {
        let from_node = match node_map.get(edge.from_node.as_str()) {
            Some(n) => n,
            None => {
                warnings.push(TypeCheckWarning {
                    node_id: edge.from_node.clone(),
                    message: format!(
                        "Edge {} references unknown source node '{}'",
                        edge.id, edge.from_node
                    ),
                });
                continue;
            }
        };

        let to_node = match node_map.get(edge.to_node.as_str()) {
            Some(n) => n,
            None => {
                warnings.push(TypeCheckWarning {
                    node_id: edge.to_node.clone(),
                    message: format!(
                        "Edge {} references unknown target node '{}'",
                        edge.id, edge.to_node
                    ),
                });
                continue;
            }
        };

        let from_sig = get_node_signature(&from_node.node_type);
        let to_sig = get_node_signature(&to_node.node_type);

        let from_type = match from_sig.outputs.get(&edge.from_port) {
            Some(ty) => ty.clone(),
            None => {
                warnings.push(TypeCheckWarning {
                    node_id: from_node.id.clone(),
                    message: format!(
                        "Port '{}' not in signature for node type '{}'; assuming Any",
                        edge.from_port, from_node.node_type
                    ),
                });
                PortType::Any
            }
        };

        let to_type = match to_sig.inputs.get(&edge.to_port) {
            Some(ty) => ty.clone(),
            None => {
                warnings.push(TypeCheckWarning {
                    node_id: to_node.id.clone(),
                    message: format!(
                        "Port '{}' not in signature for node type '{}'; assuming Any",
                        edge.to_port, to_node.node_type
                    ),
                });
                PortType::Any
            }
        };

        if !from_type.is_compatible_with(&to_type) {
            errors.push(TypeCheckError {
                edge_id: edge.id.clone(),
                from_node: from_node.id.clone(),
                from_port: edge.from_port.clone(),
                from_type,
                to_node: to_node.id.clone(),
                to_port: edge.to_port.clone(),
                to_type,
                message: "Incompatible port types".into(),
            });
        }
    }

    let is_sound = errors.is_empty();
    TypeCheckResult {
        errors,
        warnings,
        is_sound,
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::spec::*;

    // ── Display ──────────────────────────────────────────────────────────

    #[test]
    fn display_primitives() {
        assert_eq!(PortType::Any.to_string(), "any");
        assert_eq!(PortType::String.to_string(), "string");
        assert_eq!(PortType::Number.to_string(), "number");
        assert_eq!(PortType::Boolean.to_string(), "boolean");
        assert_eq!(PortType::Null.to_string(), "null");
        assert_eq!(PortType::Html.to_string(), "html");
        assert_eq!(PortType::Binary.to_string(), "binary");
    }

    #[test]
    fn display_object_untyped() {
        assert_eq!(PortType::Object(None).to_string(), "object");
    }

    #[test]
    fn display_object_typed() {
        let mut fields = HashMap::new();
        fields.insert("name".into(), PortType::String);
        let t = PortType::Object(Some(fields));
        assert_eq!(t.to_string(), "object{name: string}");
    }

    #[test]
    fn display_array() {
        assert_eq!(PortType::Array(None).to_string(), "array");
        assert_eq!(
            PortType::Array(Some(Box::new(PortType::String))).to_string(),
            "array<string>"
        );
    }

    #[test]
    fn display_union() {
        let u = PortType::Union(vec![PortType::String, PortType::Number]);
        assert_eq!(u.to_string(), "string | number");
    }

    // ── Compatibility: basics ────────────────────────────────────────────

    #[test]
    fn any_compatible_with_everything() {
        let types = vec![
            PortType::String,
            PortType::Number,
            PortType::Boolean,
            PortType::Object(None),
            PortType::Array(None),
            PortType::Html,
            PortType::Binary,
        ];
        for t in &types {
            assert!(PortType::Any.is_compatible_with(t));
            assert!(t.is_compatible_with(&PortType::Any));
        }
    }

    #[test]
    fn same_primitives_compatible() {
        assert!(PortType::String.is_compatible_with(&PortType::String));
        assert!(PortType::Number.is_compatible_with(&PortType::Number));
        assert!(PortType::Boolean.is_compatible_with(&PortType::Boolean));
        assert!(PortType::Null.is_compatible_with(&PortType::Null));
    }

    #[test]
    fn different_primitives_incompatible() {
        assert!(!PortType::String.is_compatible_with(&PortType::Number));
        assert!(!PortType::Number.is_compatible_with(&PortType::Boolean));
        assert!(!PortType::Boolean.is_compatible_with(&PortType::Null));
    }

    // ── Html subtyping ───────────────────────────────────────────────────

    #[test]
    fn html_is_subtype_of_string() {
        assert!(PortType::Html.is_compatible_with(&PortType::String));
    }

    #[test]
    fn string_is_not_subtype_of_html() {
        assert!(!PortType::String.is_compatible_with(&PortType::Html));
    }

    // ── Object compatibility ─────────────────────────────────────────────

    #[test]
    fn untyped_objects_compatible() {
        assert!(PortType::Object(None).is_compatible_with(&PortType::Object(None)));
    }

    #[test]
    fn untyped_object_compatible_with_typed() {
        let mut fields = HashMap::new();
        fields.insert("x".into(), PortType::Number);
        let typed = PortType::Object(Some(fields));

        assert!(PortType::Object(None).is_compatible_with(&typed));
        assert!(typed.is_compatible_with(&PortType::Object(None)));
    }

    #[test]
    fn object_structural_subtyping() {
        let mut producer = HashMap::new();
        producer.insert("name".into(), PortType::String);
        producer.insert("age".into(), PortType::Number);

        let mut consumer = HashMap::new();
        consumer.insert("name".into(), PortType::String);

        // Producer has more fields — width subtyping OK.
        assert!(PortType::Object(Some(producer.clone()))
            .is_compatible_with(&PortType::Object(Some(consumer.clone()))));

        // Consumer wants a field the producer lacks — fail.
        assert!(
            !PortType::Object(Some(consumer)).is_compatible_with(&PortType::Object(Some(producer)))
        );
    }

    // ── Array compatibility ──────────────────────────────────────────────

    #[test]
    fn array_compatibility() {
        assert!(PortType::Array(None).is_compatible_with(&PortType::Array(None)));
        assert!(PortType::Array(Some(Box::new(PortType::String)))
            .is_compatible_with(&PortType::Array(None)));
        assert!(PortType::Array(None)
            .is_compatible_with(&PortType::Array(Some(Box::new(PortType::Number)))));
        assert!(PortType::Array(Some(Box::new(PortType::String)))
            .is_compatible_with(&PortType::Array(Some(Box::new(PortType::String)))));
        assert!(!PortType::Array(Some(Box::new(PortType::String)))
            .is_compatible_with(&PortType::Array(Some(Box::new(PortType::Number)))));
    }

    // ── Union compatibility ──────────────────────────────────────────────

    #[test]
    fn union_producer_compatible() {
        let u = PortType::Union(vec![PortType::String, PortType::Number]);
        // String is in the union, so it is compatible with String.
        assert!(u.is_compatible_with(&PortType::String));
        assert!(u.is_compatible_with(&PortType::Number));
        assert!(!u.is_compatible_with(&PortType::Boolean));
    }

    #[test]
    fn union_consumer_compatible() {
        let u = PortType::Union(vec![PortType::String, PortType::Number]);
        assert!(PortType::String.is_compatible_with(&u));
        assert!(PortType::Number.is_compatible_with(&u));
        assert!(!PortType::Boolean.is_compatible_with(&u));
    }

    // ── Node signatures ──────────────────────────────────────────────────

    #[test]
    fn known_signatures() {
        let sig = get_node_signature("file_read");
        assert!(sig.inputs.is_empty());
        assert_eq!(sig.outputs.get("default"), Some(&PortType::String));

        let sig = get_node_signature("console");
        assert_eq!(sig.inputs.get("default"), Some(&PortType::Any));

        let sig = get_node_signature("csv");
        assert_eq!(
            sig.outputs.get("default"),
            Some(&PortType::Array(Some(Box::new(PortType::Object(None)))))
        );
    }

    #[test]
    fn unknown_node_gets_default_signature() {
        let sig = get_node_signature("my_custom_node");
        assert_eq!(sig.inputs.get("default"), Some(&PortType::Any));
        assert_eq!(sig.outputs.get("default"), Some(&PortType::Any));
    }

    // ── Full graph type checking ─────────────────────────────────────────

    fn make_node(id: &str, node_type: &str, kind: NodeKind) -> Node {
        Node {
            id: id.into(),
            node_type: node_type.into(),
            kind,
            config: serde_json::Value::Object(serde_json::Map::new()),
            inputs: vec![],
            outputs: vec![],
        }
    }

    fn make_edge(id: &str, from: &str, to: &str) -> Edge {
        Edge {
            id: id.into(),
            from_node: from.into(),
            from_port: "default".into(),
            to_node: to.into(),
            to_port: "default".into(),
        }
    }

    #[test]
    fn type_check_sound_graph() {
        let graph = AnpGraph {
            version: GraphVersion::V0_1,
            metadata: GraphMetadata {
                name: "test".into(),
                description: "".into(),
            },
            nodes: vec![
                make_node("reader", "file_read", NodeKind::Source),
                make_node("parser", "json_parse", NodeKind::Processor),
                make_node("out", "console", NodeKind::Sink),
            ],
            edges: vec![
                make_edge("e1", "reader", "parser"),
                make_edge("e2", "parser", "out"),
            ],
        };

        let result = type_check(&graph);
        assert!(result.is_sound);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn type_check_incompatible_edge() {
        // file_read outputs String, filter expects Object — incompatible.
        let graph = AnpGraph {
            version: GraphVersion::V0_1,
            metadata: GraphMetadata {
                name: "bad".into(),
                description: "".into(),
            },
            nodes: vec![
                make_node("reader", "file_read", NodeKind::Source),
                make_node("filt", "filter", NodeKind::Processor),
            ],
            edges: vec![make_edge("e1", "reader", "filt")],
        };

        let result = type_check(&graph);
        assert!(!result.is_sound);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].from_type, PortType::String);
        assert_eq!(result.errors[0].to_type, PortType::Object(None));
    }

    #[test]
    fn type_check_html_to_string_sink() {
        // html_template outputs Html, file_write expects String — should be OK
        // because Html is a subtype of String.
        let graph = AnpGraph {
            version: GraphVersion::V0_1,
            metadata: GraphMetadata {
                name: "html_pipe".into(),
                description: "".into(),
            },
            nodes: vec![
                make_node("tmpl", "html_template", NodeKind::Processor),
                make_node("writer", "file_write", NodeKind::Sink),
            ],
            edges: vec![make_edge("e1", "tmpl", "writer")],
        };

        let result = type_check(&graph);
        assert!(result.is_sound);
    }

    #[test]
    fn type_check_warns_on_disconnected_sink() {
        let graph = AnpGraph {
            version: GraphVersion::V0_1,
            metadata: GraphMetadata {
                name: "lonely".into(),
                description: "".into(),
            },
            nodes: vec![make_node("out", "console", NodeKind::Sink)],
            edges: vec![],
        };

        let result = type_check(&graph);
        assert!(result.is_sound);
        assert!(!result.warnings.is_empty());
        assert!(result.warnings[0].message.contains("no incoming edges"));
    }

    #[test]
    fn type_check_display() {
        let result = TypeCheckResult {
            errors: vec![TypeCheckError {
                edge_id: "e1".into(),
                from_node: "parser".into(),
                from_port: "default".into(),
                from_type: PortType::Number,
                to_node: "template".into(),
                to_port: "default".into(),
                to_type: PortType::Object(None),
                message: "Incompatible port types".into(),
            }],
            warnings: vec![],
            is_sound: false,
        };
        let text = result.to_string();
        assert!(text.contains("ANP Type Check"));
        assert!(text.contains("INCOMPATIBLE"));
        assert!(text.contains("1 type error"));
    }

    // ── Serde round‐trip ─────────────────────────────────────────────────

    #[test]
    fn port_type_serde_roundtrip() {
        let types = vec![
            PortType::Any,
            PortType::String,
            PortType::Number,
            PortType::Object(None),
            PortType::Array(Some(Box::new(PortType::String))),
            PortType::Union(vec![PortType::String, PortType::Number]),
        ];
        for t in types {
            let json = serde_json::to_string(&t).unwrap();
            let back: PortType = serde_json::from_str(&json).unwrap();
            assert_eq!(t, back);
        }
    }
}
