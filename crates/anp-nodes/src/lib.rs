pub mod conditional;
pub mod console;
pub mod database;
pub mod file_io;
pub mod filter;
pub mod http_client;
pub mod http_server;
pub mod llm_call;
pub mod memory;
pub mod template;
pub mod tool_router;
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

    // LLM Call (Processor) — AI calling AI within a graph
    registry.register(NodeInfo {
        factory: || Box::new(llm_call::LlmCallNode::new()),
        schema: NodeSchema {
            node_type: "llm_call".to_string(),
            kind: NodeKind::Processor,
            description: "Calls an LLM (OpenAI or Anthropic) — AI invoking AI within a graph".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "provider": { "type": "string", "enum": ["openai", "anthropic"], "default": "openai" },
                    "model": { "type": "string", "description": "Model name (e.g., gpt-4o-mini, claude-sonnet-4-20250514)" },
                    "system_prompt": { "type": "string", "description": "System prompt for the LLM" },
                    "temperature": { "type": "number", "minimum": 0, "maximum": 2, "default": 0.7 },
                    "max_tokens": { "type": "integer", "minimum": 1, "maximum": 128000, "default": 1024 },
                    "timeout_secs": { "type": "integer", "minimum": 1, "maximum": 300, "default": 60 }
                }
            }),
        },
    });

    // HTTP Client (Processor) — outbound HTTP requests
    registry.register(NodeInfo {
        factory: || Box::new(http_client::HttpClientNode::new()),
        schema: NodeSchema {
            node_type: "http_client".to_string(),
            kind: NodeKind::Processor,
            description: "Makes outbound HTTP requests (GET, POST, PUT, DELETE, etc.)".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "Target URL" },
                    "method": { "type": "string", "enum": ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"], "default": "GET" },
                    "headers": { "type": "object", "description": "HTTP headers as key-value pairs" },
                    "timeout_secs": { "type": "integer", "minimum": 1, "maximum": 300, "default": 30 }
                }
            }),
        },
    });

    // Template (Processor) — renders string templates with {{variable}} placeholders
    registry.register(NodeInfo {
        factory: || Box::new(template::TemplateNode::new()),
        schema: NodeSchema {
            node_type: "template".to_string(),
            kind: NodeKind::Processor,
            description: "Renders string templates with {{variable}} placeholders and dot-path support".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "template": { "type": "string", "description": "Template string with {{placeholders}}" }
                },
                "required": ["template"]
            }),
        },
    });

    // Conditional (Processor) — routes data to different ports based on conditions
    registry.register(NodeInfo {
        factory: || Box::new(conditional::ConditionalNode::new()),
        schema: NodeSchema {
            node_type: "conditional".to_string(),
            kind: NodeKind::Processor,
            description: "Routes data to different output ports based on field conditions (if/else branching)".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "field": { "type": "string", "description": "Dot-path to the field to evaluate" },
                    "branches": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "op": { "type": "string", "enum": ["eq", "neq", "gt", "gte", "lt", "lte", "contains", "exists"] },
                                "value": { "description": "Value to compare against" },
                                "port": { "type": "string", "description": "Output port name if condition matches" }
                            },
                            "required": ["op", "port"]
                        }
                    },
                    "default_port": { "type": "string", "default": "default" }
                },
                "required": ["field", "branches"]
            }),
        },
    });

    // Memory (Processor) — session-based state and conversation history
    registry.register(NodeInfo {
        factory: || Box::new(memory::MemoryNode::new()),
        schema: NodeSchema {
            node_type: "memory".to_string(),
            kind: NodeKind::Processor,
            description: "Session-based memory: conversation history and key-value state persistence".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "max_history": { "type": "integer", "minimum": 1, "maximum": 1000, "default": 50 },
                    "initial_state": { "type": "object", "description": "Initial key-value pairs" }
                }
            }),
        },
    });

    // Tool Router (Processor) — LLM-powered agentic routing
    registry.register(NodeInfo {
        factory: || Box::new(tool_router::ToolRouterNode::new()),
        schema: NodeSchema {
            node_type: "tool_router".to_string(),
            kind: NodeKind::Processor,
            description: "LLM-powered agentic routing — AI decides which tool/port to route data to".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "provider": { "type": "string", "enum": ["openai", "anthropic"], "default": "openai" },
                    "model": { "type": "string", "description": "Model name" },
                    "tools": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string", "description": "Tool/port name" },
                                "description": { "type": "string", "description": "What this tool does" }
                            },
                            "required": ["name", "description"]
                        }
                    },
                    "system_prompt": { "type": "string" },
                    "max_tokens": { "type": "integer", "default": 64 },
                    "timeout_secs": { "type": "integer", "default": 30 }
                },
                "required": ["tools"]
            }),
        },
    });
}
