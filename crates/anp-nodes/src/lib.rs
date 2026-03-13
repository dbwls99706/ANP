pub mod console;
pub mod database;
pub mod file_io;
pub mod filter;
pub mod http_server;
pub mod transform;

use anp_core::graph::spec::{NodeKind, PortSpec};
use anp_core::node::registry::{NodeInfo, NodeRegistry, NodeSchema};

/// Register all built-in nodes with the registry
pub fn register_all(registry: &mut NodeRegistry) {
    // Console (Sink)
    registry.register(NodeInfo {
        factory: || Box::new(console::ConsoleNode::new()),
        schema: NodeSchema {
            node_type: "console".to_string(),
            kind: NodeKind::Sink,
            description: "Prints data to stdout".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "prefix": { "type": "string", "description": "Optional prefix for output" }
                }
            }),
        },
    });

    // FileRead (Source)
    registry.register(NodeInfo {
        factory: || Box::new(file_io::FileReadNode::new()),
        schema: NodeSchema {
            node_type: "file_read".to_string(),
            kind: NodeKind::Source,
            description: "Reads content from a file".to_string(),
            inputs: vec![],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to read" }
                },
                "required": ["path"]
            }),
        },
    });

    // FileWrite (Sink)
    registry.register(NodeInfo {
        factory: || Box::new(file_io::FileWriteNode::new()),
        schema: NodeSchema {
            node_type: "file_write".to_string(),
            kind: NodeKind::Sink,
            description: "Writes data to a file".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to write" }
                },
                "required": ["path"]
            }),
        },
    });

    // Transform (Processor)
    registry.register(NodeInfo {
        factory: || Box::new(transform::TransformNode::new()),
        schema: NodeSchema {
            node_type: "transform".to_string(),
            kind: NodeKind::Processor,
            description: "Transforms data using dot-path field mapping".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "type": { "type": "string", "enum": ["map"] },
                    "mapping": {
                        "type": "object",
                        "description": "Output field name → input dot-path (e.g., 'name': 'user.name')"
                    }
                },
                "required": ["mapping"]
            }),
        },
    });

    // Filter (Processor)
    registry.register(NodeInfo {
        factory: || Box::new(filter::FilterNode::new()),
        schema: NodeSchema {
            node_type: "filter".to_string(),
            kind: NodeKind::Processor,
            description: "Filters data based on conditions (eq, gt, lt, contains, etc.)".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "field": { "type": "string", "description": "Dot-path to the field to filter on" },
                    "op": { "type": "string", "enum": ["eq", "neq", "gt", "gte", "lt", "lte", "contains"] },
                    "value": { "description": "Value to compare against" }
                },
                "required": ["field", "op", "value"]
            }),
        },
    });

    // HttpServer (Service)
    registry.register(NodeInfo {
        factory: || Box::new(http_server::HttpServerNode::new()),
        schema: NodeSchema {
            node_type: "http_server".to_string(),
            kind: NodeKind::Service,
            description: "HTTP server with route-based output ports".to_string(),
            inputs: vec![],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "port": { "type": "integer", "default": 3000 },
                    "routes": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string" },
                                "method": { "type": "string", "default": "ANY" },
                                "output_port": { "type": "string" }
                            },
                            "required": ["path", "output_port"]
                        }
                    }
                }
            }),
        },
    });

    // Database (Processor)
    registry.register(NodeInfo {
        factory: || Box::new(database::DatabaseNode::new()),
        schema: NodeSchema {
            node_type: "database".to_string(),
            kind: NodeKind::Processor,
            description: "In-memory key-value store (get/set/delete/list)".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "initial": {
                        "type": "object",
                        "description": "Initial key-value pairs to pre-populate"
                    }
                }
            }),
        },
    });
}
