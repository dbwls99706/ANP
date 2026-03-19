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
        "http_client" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Object(None))]),
            outputs: HashMap::from([("default".into(), PortType::Object(None))]),
        },
        "memory" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Any)]),
            outputs: HashMap::from([("default".into(), PortType::Object(None))]),
        },
        "tool_router" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Any)]),
            outputs: HashMap::from([("default".into(), PortType::Any)]),
        },
        "switch" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Any)]),
            outputs: HashMap::from([("default".into(), PortType::Any)]),
        },
        "try_catch" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Any)]),
            outputs: HashMap::from([("default".into(), PortType::Any)]),
        },
        "parallel" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Any)]),
            outputs: HashMap::from([("default".into(), PortType::Array(None))]),
        },
        "accumulator" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Any)]),
            outputs: HashMap::from([("default".into(), PortType::Array(None))]),
        },
        "sleep" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Any)]),
            outputs: HashMap::from([("default".into(), PortType::Any)]),
        },
        "debounce" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Any)]),
            outputs: HashMap::from([("default".into(), PortType::Any)]),
        },
        "rate_limit" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Any)]),
            outputs: HashMap::from([("default".into(), PortType::Any)]),
        },
        "retry" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Any)]),
            outputs: HashMap::from([("default".into(), PortType::Any)]),
        },
        "circuit_breaker" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Any)]),
            outputs: HashMap::from([("default".into(), PortType::Any)]),
        },
        "cache" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Any)]),
            outputs: HashMap::from([("default".into(), PortType::Any)]),
        },
        "queue" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Any)]),
            outputs: HashMap::from([("default".into(), PortType::Any)]),
        },
        "redis" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Object(None))]),
            outputs: HashMap::from([("default".into(), PortType::Object(None))]),
        },
        "sql_query" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Object(None))]),
            outputs: HashMap::from([(
                "default".into(),
                PortType::Array(Some(Box::new(PortType::Object(None)))),
            )]),
        },
        "jwt" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::Object(None))]),
            outputs: HashMap::from([("default".into(), PortType::Object(None))]),
        },
        "hash" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::String)]),
            outputs: HashMap::from([("default".into(), PortType::String)]),
        },
        "encrypt" => NodeTypeSignature {
            inputs: HashMap::from([("default".into(), PortType::String)]),
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

// ─── Type Unification ───────────────────────────────────────────────────────

/// Unify two types into the most specific compatible type.
///
/// This implements a simplified Hindley-Milner-style unification for ANP's
/// dataflow type system. `Any` acts as a type variable — it unifies with
/// everything. Concrete types unify only with themselves (or structurally
/// compatible compound types).
///
/// Returns `None` if the types are incompatible.
pub fn unify(a: &PortType, b: &PortType) -> Option<PortType> {
    // Any acts as a type variable — unifies with anything.
    if matches!(a, PortType::Any) {
        return Some(b.clone());
    }
    if matches!(b, PortType::Any) {
        return Some(a.clone());
    }

    match (a, b) {
        // Identical primitives.
        (PortType::String, PortType::String) => Some(PortType::String),
        (PortType::Number, PortType::Number) => Some(PortType::Number),
        (PortType::Boolean, PortType::Boolean) => Some(PortType::Boolean),
        (PortType::Null, PortType::Null) => Some(PortType::Null),
        (PortType::Binary, PortType::Binary) => Some(PortType::Binary),
        (PortType::Html, PortType::Html) => Some(PortType::Html),

        // Html is a subtype of String — Html is more specific.
        (PortType::Html, PortType::String) | (PortType::String, PortType::Html) => {
            Some(PortType::Html)
        }

        // Object unification.
        (PortType::Object(a_fields), PortType::Object(b_fields)) => match (a_fields, b_fields) {
            (None, None) => Some(PortType::Object(None)),
            (Some(f), None) | (None, Some(f)) => Some(PortType::Object(Some(f.clone()))),
            (Some(fa), Some(fb)) => {
                // Merge fields with recursive unification.
                let mut merged = fa.clone();
                for (key, b_ty) in fb {
                    if let Some(a_ty) = merged.get(key) {
                        // Both sides have this field — unify recursively.
                        match unify(a_ty, b_ty) {
                            Some(unified) => {
                                merged.insert(key.clone(), unified);
                            }
                            None => return None, // Field type conflict.
                        }
                    } else {
                        merged.insert(key.clone(), b_ty.clone());
                    }
                }
                Some(PortType::Object(Some(merged)))
            }
        },

        // Array unification.
        (PortType::Array(a_elem), PortType::Array(b_elem)) => match (a_elem, b_elem) {
            (None, None) => Some(PortType::Array(None)),
            (Some(e), None) | (None, Some(e)) => Some(PortType::Array(Some(e.clone()))),
            (Some(ea), Some(eb)) => {
                unify(ea, eb).map(|u| PortType::Array(Some(Box::new(u))))
            }
        },

        // Union unification: unify if there is at least one compatible variant.
        (PortType::Union(variants), other) | (other, PortType::Union(variants)) => {
            // Try to find a variant that unifies with the other type.
            for v in variants {
                if let Some(u) = unify(v, other) {
                    return Some(u);
                }
            }
            None
        }

        // Everything else is incompatible.
        _ => None,
    }
}

// ─── Type Inference ─────────────────────────────────────────────────────────

/// Result of type inference over a graph.
#[derive(Debug, Clone)]
pub struct TypeInferenceResult {
    /// Inferred type for each (node_id, port_name) pair.
    pub port_types: HashMap<(String, String), PortType>,
    /// Whether inference found any contradictions.
    pub has_contradictions: bool,
    /// Contradiction details.
    pub contradictions: Vec<String>,
    /// Number of ports with types narrowed from Any.
    pub ports_narrowed: usize,
    /// Total ports analyzed.
    pub total_ports: usize,
}

impl fmt::Display for TypeInferenceResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ANP Type Inference")?;
        writeln!(f, "\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}")?;

        // Count unique nodes from ports.
        let node_ids: std::collections::HashSet<&str> = self
            .port_types
            .keys()
            .map(|(node_id, _)| node_id.as_str())
            .collect();
        writeln!(
            f,
            "Analyzed {} ports across {} nodes",
            self.total_ports,
            node_ids.len()
        )?;
        writeln!(f)?;

        writeln!(f, "Inferred Types:")?;
        // Sort entries for deterministic output.
        let mut entries: Vec<_> = self.port_types.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for ((node_id, port_name), ty) in &entries {
            write!(f, "  {}.{:<12} \u{2192} {}", node_id, port_name, ty)?;
            // Check if this was narrowed (we mark it in the display).
            // We don't store the original type separately, so we rely on the
            // narrowed count for the summary.
            writeln!(f)?;
        }

        writeln!(f)?;
        writeln!(
            f,
            "Narrowed: {} port{} (from any \u{2192} specific type)",
            self.ports_narrowed,
            if self.ports_narrowed == 1 { "" } else { "s" }
        )?;
        write!(
            f,
            "Contradictions: {}",
            self.contradictions.len()
        )?;

        if !self.contradictions.is_empty() {
            writeln!(f)?;
            for c in &self.contradictions {
                write!(f, "  \u{2717} {}", c)?;
            }
        }

        Ok(())
    }
}

/// Infer concrete types for ports by propagating known types through the graph.
///
/// Uses a constraint-based approach inspired by Hindley-Milner:
/// 1. Collect initial constraints from node signatures
/// 2. Propagate type information along edges using unification
/// 3. Iterate until fixed point (no more changes)
/// 4. Report inferred types and any contradictions
pub fn infer_types(graph: &super::spec::AnpGraph) -> TypeInferenceResult {
    // Phase 1: Initialize each port with its node signature type.
    let mut port_types: HashMap<(String, String), PortType> = HashMap::new();

    // Track which ports started as Any so we can count narrowings.
    let mut initially_any: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();

    for node in &graph.nodes {
        let sig = get_node_signature(&node.node_type);
        for (port_name, port_type) in &sig.inputs {
            let key = (node.id.clone(), port_name.clone());
            if matches!(port_type, PortType::Any) {
                initially_any.insert(key.clone());
            }
            port_types.insert(key, port_type.clone());
        }
        for (port_name, port_type) in &sig.outputs {
            let key = (node.id.clone(), port_name.clone());
            if matches!(port_type, PortType::Any) {
                initially_any.insert(key.clone());
            }
            port_types.insert(key, port_type.clone());
        }
    }

    let total_ports = port_types.len();
    let mut contradictions: Vec<String> = Vec::new();

    // Phase 2: Fixed-point iteration — propagate types along edges.
    // We iterate until no port type changes.
    let max_iterations = 100; // Safety bound to prevent infinite loops.
    for _iteration in 0..max_iterations {
        let mut changed = false;

        for edge in &graph.edges {
            let from_key = (edge.from_node.clone(), edge.from_port.clone());
            let to_key = (edge.to_node.clone(), edge.to_port.clone());

            let from_type = match port_types.get(&from_key) {
                Some(t) => t.clone(),
                None => continue, // Unknown port, skip.
            };
            let to_type = match port_types.get(&to_key) {
                Some(t) => t.clone(),
                None => continue,
            };

            // Try to unify the output type with the input type.
            match unify(&from_type, &to_type) {
                Some(unified) => {
                    // Propagate: if the input side was less specific, narrow it.
                    if unified != to_type {
                        port_types.insert(to_key.clone(), unified.clone());
                        changed = true;
                    }
                    // Back-propagate: if the output side was less specific, narrow it.
                    if unified != from_type {
                        port_types.insert(from_key.clone(), unified);
                        changed = true;
                    }
                }
                None => {
                    // Types are incompatible — record contradiction.
                    let msg = format!(
                        "Edge {}: {}.{} ({}) conflicts with {}.{} ({})",
                        edge.id, edge.from_node, edge.from_port, from_type,
                        edge.to_node, edge.to_port, to_type,
                    );
                    if !contradictions.contains(&msg) {
                        contradictions.push(msg);
                    }
                }
            }
        }

        if !changed {
            break;
        }
    }

    // Phase 3: Count how many ports were narrowed from Any.
    let ports_narrowed = initially_any
        .iter()
        .filter(|key| {
            port_types
                .get(*key)
                .is_some_and(|t| !matches!(t, PortType::Any))
        })
        .count();

    let has_contradictions = !contradictions.is_empty();

    TypeInferenceResult {
        port_types,
        has_contradictions,
        contradictions,
        ports_narrowed,
        total_ports,
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

    // ── Type Unification ────────────────────────────────────────────────

    #[test]
    fn unify_any_with_concrete() {
        assert_eq!(unify(&PortType::Any, &PortType::String), Some(PortType::String));
        assert_eq!(unify(&PortType::Number, &PortType::Any), Some(PortType::Number));
        assert_eq!(unify(&PortType::Any, &PortType::Any), Some(PortType::Any));
    }

    #[test]
    fn unify_identical_primitives() {
        assert_eq!(unify(&PortType::String, &PortType::String), Some(PortType::String));
        assert_eq!(unify(&PortType::Number, &PortType::Number), Some(PortType::Number));
        assert_eq!(unify(&PortType::Boolean, &PortType::Boolean), Some(PortType::Boolean));
        assert_eq!(unify(&PortType::Null, &PortType::Null), Some(PortType::Null));
        assert_eq!(unify(&PortType::Binary, &PortType::Binary), Some(PortType::Binary));
    }

    #[test]
    fn unify_incompatible_primitives() {
        assert_eq!(unify(&PortType::String, &PortType::Number), None);
        assert_eq!(unify(&PortType::Boolean, &PortType::String), None);
        assert_eq!(unify(&PortType::Number, &PortType::Boolean), None);
    }

    #[test]
    fn unify_html_and_string() {
        // Html is more specific than String — unification yields Html.
        assert_eq!(unify(&PortType::Html, &PortType::String), Some(PortType::Html));
        assert_eq!(unify(&PortType::String, &PortType::Html), Some(PortType::Html));
    }

    #[test]
    fn unify_objects_none_with_some() {
        let mut fields = HashMap::new();
        fields.insert("name".into(), PortType::String);
        let typed = PortType::Object(Some(fields.clone()));

        assert_eq!(
            unify(&PortType::Object(None), &typed),
            Some(PortType::Object(Some(fields.clone())))
        );
        assert_eq!(
            unify(&typed, &PortType::Object(None)),
            Some(PortType::Object(Some(fields)))
        );
    }

    #[test]
    fn unify_objects_merge_fields() {
        let mut fa = HashMap::new();
        fa.insert("name".into(), PortType::String);

        let mut fb = HashMap::new();
        fb.insert("age".into(), PortType::Number);

        let result = unify(
            &PortType::Object(Some(fa)),
            &PortType::Object(Some(fb)),
        );

        let mut expected = HashMap::new();
        expected.insert("name".into(), PortType::String);
        expected.insert("age".into(), PortType::Number);
        assert_eq!(result, Some(PortType::Object(Some(expected))));
    }

    #[test]
    fn unify_objects_recursive_field_conflict() {
        let mut fa = HashMap::new();
        fa.insert("x".into(), PortType::String);

        let mut fb = HashMap::new();
        fb.insert("x".into(), PortType::Number);

        // "x" is String in one, Number in the other — conflict.
        assert_eq!(
            unify(&PortType::Object(Some(fa)), &PortType::Object(Some(fb))),
            None
        );
    }

    #[test]
    fn unify_objects_recursive_field_narrowing() {
        let mut fa = HashMap::new();
        fa.insert("data".into(), PortType::Any);

        let mut fb = HashMap::new();
        fb.insert("data".into(), PortType::String);

        let result = unify(
            &PortType::Object(Some(fa)),
            &PortType::Object(Some(fb)),
        );

        let mut expected = HashMap::new();
        expected.insert("data".into(), PortType::String);
        assert_eq!(result, Some(PortType::Object(Some(expected))));
    }

    #[test]
    fn unify_arrays() {
        assert_eq!(
            unify(&PortType::Array(None), &PortType::Array(Some(Box::new(PortType::String)))),
            Some(PortType::Array(Some(Box::new(PortType::String))))
        );
        assert_eq!(
            unify(
                &PortType::Array(Some(Box::new(PortType::String))),
                &PortType::Array(Some(Box::new(PortType::String)))
            ),
            Some(PortType::Array(Some(Box::new(PortType::String))))
        );
        assert_eq!(
            unify(
                &PortType::Array(Some(Box::new(PortType::String))),
                &PortType::Array(Some(Box::new(PortType::Number)))
            ),
            None
        );
    }

    #[test]
    fn unify_union_with_concrete() {
        let u = PortType::Union(vec![PortType::String, PortType::Number]);
        // String is in the union — unifies to String.
        assert_eq!(unify(&u, &PortType::String), Some(PortType::String));
        assert_eq!(unify(&PortType::Number, &u), Some(PortType::Number));
        // Boolean is not in the union — fails.
        assert_eq!(unify(&u, &PortType::Boolean), None);
    }

    // ── Type Inference ──────────────────────────────────────────────────

    #[test]
    fn infer_types_narrows_any_from_upstream() {
        // file_read (output: String) → console (input: Any)
        // Console's input should be narrowed from Any to String.
        let graph = AnpGraph {
            version: GraphVersion::V0_1,
            metadata: GraphMetadata {
                name: "narrow".into(),
                description: "".into(),
            },
            nodes: vec![
                make_node("reader", "file_read", NodeKind::Source),
                make_node("out", "console", NodeKind::Sink),
            ],
            edges: vec![make_edge("e1", "reader", "out")],
        };

        let result = infer_types(&graph);
        assert!(!result.has_contradictions);
        assert!(result.ports_narrowed >= 1);

        // Console's default input should now be String, not Any.
        let console_input = result
            .port_types
            .get(&("out".into(), "default".into()))
            .unwrap();
        assert_eq!(*console_input, PortType::String);
    }

    #[test]
    fn infer_types_propagates_through_chain() {
        // file_read (String) → llm_call (Any→String) → console (Any)
        // llm_call's input should be narrowed to String,
        // console's input should be narrowed to String.
        let graph = AnpGraph {
            version: GraphVersion::V0_1,
            metadata: GraphMetadata {
                name: "chain".into(),
                description: "".into(),
            },
            nodes: vec![
                make_node("reader", "file_read", NodeKind::Source),
                make_node("llm", "llm_call", NodeKind::Processor),
                make_node("out", "console", NodeKind::Sink),
            ],
            edges: vec![
                make_edge("e1", "reader", "llm"),
                make_edge("e2", "llm", "out"),
            ],
        };

        let result = infer_types(&graph);
        assert!(!result.has_contradictions);

        // llm_call input narrowed from Any to String.
        let llm_input = result
            .port_types
            .get(&("llm".into(), "default".into()))
            .unwrap();
        assert_eq!(*llm_input, PortType::String);

        // Console input narrowed from Any to String (from llm_call output).
        let console_input = result
            .port_types
            .get(&("out".into(), "default".into()))
            .unwrap();
        assert_eq!(*console_input, PortType::String);
    }

    #[test]
    fn infer_types_detects_contradiction() {
        // file_read (String) → filter (Object) — incompatible.
        let graph = AnpGraph {
            version: GraphVersion::V0_1,
            metadata: GraphMetadata {
                name: "conflict".into(),
                description: "".into(),
            },
            nodes: vec![
                make_node("reader", "file_read", NodeKind::Source),
                make_node("filt", "filter", NodeKind::Processor),
            ],
            edges: vec![make_edge("e1", "reader", "filt")],
        };

        let result = infer_types(&graph);
        assert!(result.has_contradictions);
        assert_eq!(result.contradictions.len(), 1);
        assert!(result.contradictions[0].contains("conflicts"));
    }

    #[test]
    fn infer_types_object_narrowing() {
        // http_server (Object(None)) → transform (Any→Object(None))
        // Transform input should be narrowed from Any to Object(None).
        let graph = AnpGraph {
            version: GraphVersion::V0_1,
            metadata: GraphMetadata {
                name: "obj".into(),
                description: "".into(),
            },
            nodes: vec![
                make_node("server", "http_server", NodeKind::Service),
                make_node("xform", "transform", NodeKind::Processor),
            ],
            edges: vec![make_edge("e1", "server", "xform")],
        };

        let result = infer_types(&graph);
        assert!(!result.has_contradictions);

        let xform_input = result
            .port_types
            .get(&("xform".into(), "default".into()))
            .unwrap();
        assert_eq!(*xform_input, PortType::Object(None));
        assert!(result.ports_narrowed >= 1);
    }

    #[test]
    fn infer_types_back_propagation() {
        // sleep (Any→Any) → file_write (String input)
        // sleep's output should be back-propagated to String.
        let graph = AnpGraph {
            version: GraphVersion::V0_1,
            metadata: GraphMetadata {
                name: "backprop".into(),
                description: "".into(),
            },
            nodes: vec![
                make_node("wait", "sleep", NodeKind::Processor),
                make_node("writer", "file_write", NodeKind::Sink),
            ],
            edges: vec![make_edge("e1", "wait", "writer")],
        };

        let result = infer_types(&graph);
        assert!(!result.has_contradictions);

        // sleep's output should be narrowed from Any to String.
        let sleep_output = result
            .port_types
            .get(&("wait".into(), "default".into()))
            .unwrap();
        assert_eq!(*sleep_output, PortType::String);
    }

    #[test]
    fn infer_types_empty_graph() {
        let graph = AnpGraph {
            version: GraphVersion::V0_1,
            metadata: GraphMetadata {
                name: "empty".into(),
                description: "".into(),
            },
            nodes: vec![],
            edges: vec![],
        };

        let result = infer_types(&graph);
        assert!(!result.has_contradictions);
        assert_eq!(result.ports_narrowed, 0);
        assert_eq!(result.total_ports, 0);
    }

    #[test]
    fn infer_types_display_format() {
        let graph = AnpGraph {
            version: GraphVersion::V0_1,
            metadata: GraphMetadata {
                name: "display".into(),
                description: "".into(),
            },
            nodes: vec![
                make_node("reader", "file_read", NodeKind::Source),
                make_node("out", "console", NodeKind::Sink),
            ],
            edges: vec![make_edge("e1", "reader", "out")],
        };

        let result = infer_types(&graph);
        let text = result.to_string();
        assert!(text.contains("ANP Type Inference"));
        assert!(text.contains("Analyzed"));
        assert!(text.contains("Inferred Types:"));
        assert!(text.contains("Narrowed:"));
        assert!(text.contains("Contradictions: 0"));
    }

    #[test]
    fn infer_types_multi_output_conditional() {
        // http_server → conditional (Any→Any true/false) → console
        // conditional's input should narrow to Object(None) from the server.
        let graph = AnpGraph {
            version: GraphVersion::V0_1,
            metadata: GraphMetadata {
                name: "cond".into(),
                description: "".into(),
            },
            nodes: vec![
                make_node("server", "http_server", NodeKind::Service),
                make_node("cond", "conditional", NodeKind::Processor),
                make_node("out", "console", NodeKind::Sink),
            ],
            edges: vec![
                make_edge("e1", "server", "cond"),
                Edge {
                    id: "e2".into(),
                    from_node: "cond".into(),
                    from_port: "true".into(),
                    to_node: "out".into(),
                    to_port: "default".into(),
                },
            ],
        };

        let result = infer_types(&graph);
        assert!(!result.has_contradictions);

        // Conditional's input should narrow to Object(None).
        let cond_input = result
            .port_types
            .get(&("cond".into(), "default".into()))
            .unwrap();
        assert_eq!(*cond_input, PortType::Object(None));
    }
}
