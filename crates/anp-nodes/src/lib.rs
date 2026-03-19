pub mod accumulator;
pub mod aes_node;
pub mod agent_loop_node;
pub mod aggregate_node;
pub mod api_key_node;
pub mod array_node;
pub mod base64_node;
pub mod batch_node;
pub mod breadcrumb_node;
pub mod cache_node;
pub mod chart_node;
pub mod circuit_breaker_node;
pub mod classify_node;
pub mod conditional;
pub mod console;
pub mod cors_node;
pub mod counter_node;
pub mod cron_node;
pub mod csv_node;
pub mod database;
pub mod debounce_node;
pub mod deduplicate_node;
pub mod diff_node;
pub mod email_node;
pub mod embedding_node;
pub mod encrypt_node;
pub mod env_node;
pub mod event_emitter_node;
pub mod extract_node;
pub mod file_io;
pub mod filter;
pub mod foreach;
pub mod form_node;
pub mod group_by_node;
pub mod gzip_node;
pub mod health_check_node;
pub mod hash_node;
pub mod html_template;
pub mod http_client;
pub mod http_server;
pub mod image_resize;
pub mod json_parse;
pub mod jwt_node;
pub mod kv_store_node;
pub mod layout_node;
pub mod llm_call;
pub mod log_node;
pub mod lookup_node;
pub mod oauth_node;
pub mod markdown_node;
pub mod markdown_render_node;
pub mod math_node;
pub mod memory;
pub mod merge;
pub mod message_broker_node;
pub mod meta_node;
pub mod metrics_node;
pub mod multipart_node;
pub mod paginate_node;
pub mod parallel_node;
pub mod password_node;
pub mod pivot_node;
pub mod priority_queue_node;
pub mod prompt_chain_node;
pub mod queue_node;
pub mod random_node;
pub mod rate_limit;
pub mod redirect_node;
pub mod request_id_node;
pub mod redis_client_node;
pub mod regex_node;
pub mod retry_node;
pub mod sanitize_node;
pub mod semaphore_node;
pub mod sentiment_node;
pub mod session_node;
pub mod sleep_node;
pub mod sql_query_node;
pub mod sse_node;
pub mod state_machine_node;
pub mod static_file;
pub mod string_node;
pub mod summarize_node;
pub mod switch_node;
pub mod table_node;
pub mod template;
pub mod template_engine;
pub mod throttle_node;
pub mod timestamp_node;
pub mod toml_node;
pub mod tool_router;
pub mod totp_node;
pub mod transform;
pub mod translate_node;
pub mod try_catch;
pub mod url_encode_node;
pub mod uuid_node;
pub mod validate_node;
pub mod webhook_node;
pub mod websocket_enhanced_node;
pub mod websocket_node;
pub mod xml_node;
pub mod yaml_node;

use anp_core::graph::spec::{NodeKind, PortSpec};
use anp_core::node::plugin::NodePlugin;
use anp_core::node::registry::{NodeInfo, NodeRegistry, NodeSchema};

/// Built-in node plugin that ships with ANP.
/// Implements `NodePlugin` trait for clean registration.
pub struct BuiltinPlugin;

impl NodePlugin for BuiltinPlugin {
    fn name(&self) -> &str {
        "anp-builtins"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn register(&self, registry: &mut NodeRegistry) {
        register_all(registry);
    }
}

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
            description: "HTTP server with route-based output ports. Supports path params (/users/:id), query params (?key=val), and HTML responses via _content_type.".to_string(),
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

    // HTML Template (Processor) — renders full HTML pages with variable substitution
    registry.register(NodeInfo {
        factory: || Box::new(html_template::HtmlTemplateNode::new()),
        schema: NodeSchema {
            node_type: "html_template".to_string(),
            kind: NodeKind::Processor,
            description: "Renders HTML templates with {{variable}} placeholders. Outputs {html, _content_type} for http_server to serve as text/html. Use {{{triple}}} for raw/unescaped output.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "html": { "type": "string", "description": "HTML template with {{placeholders}} for variable substitution" }
                },
                "required": ["html"]
            }),
        },
    });

    // Static File (Processor) — reads and serves static files with content type detection
    registry.register(NodeInfo {
        factory: || Box::new(static_file::StaticFileNode::new()),
        schema: NodeSchema {
            node_type: "static_file".to_string(),
            kind: NodeKind::Processor,
            description: "Reads static files (CSS, JS, images) and outputs {body, _content_type} for http_server. Auto-detects content type from extension.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to serve" },
                    "base_dir": { "type": "string", "description": "Base directory for path resolution (security boundary)" }
                }
            }),
        },
    });

    // ForEach (Processor) — iterates over arrays (graph-native loop)
    registry.register(NodeInfo {
        factory: || Box::new(foreach::ForEachNode::new()),
        schema: NodeSchema {
            node_type: "foreach".to_string(),
            kind: NodeKind::Processor,
            description: "Iterates over arrays — the graph-native replacement for for-loops. Extracts array from input field and emits items for downstream processing.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "field": { "type": "string", "description": "Dot-path to the array field (optional, input is used directly if omitted)" },
                    "collect": { "type": "boolean", "default": true, "description": "If true, wraps items with {items, count} metadata" }
                }
            }),
        },
    });

    // Merge (Processor) — combines multiple inputs into one
    registry.register(NodeInfo {
        factory: || Box::new(merge::MergeNode::new()),
        schema: NodeSchema {
            node_type: "merge".to_string(),
            kind: NodeKind::Processor,
            description: "Merges multiple JSON objects into one. Strategies: shallow (top-level merge), deep (recursive merge, arrays concatenated), array (wrap in array).".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "strategy": { "type": "string", "enum": ["shallow", "deep", "array"], "default": "shallow" }
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

    // Switch (Processor) — multi-branch routing like switch/case
    registry.register(NodeInfo {
        factory: || Box::new(switch_node::SwitchNode::new()),
        schema: NodeSchema {
            node_type: "switch".to_string(),
            kind: NodeKind::Processor,
            description: "Multi-branch routing like a switch/case statement. Routes data to a port based on field value matching.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "field": { "type": "string", "description": "Dot-path to the field to match on" },
                    "cases": { "type": "object", "description": "Map of value → output port name" },
                    "default_port": { "type": "string", "default": "default" }
                },
                "required": ["field", "cases"]
            }),
        },
    });

    // TryCatch (Processor) — error handling with fallback routing
    registry.register(NodeInfo {
        factory: || Box::new(try_catch::TryCatchNode::new()),
        schema: NodeSchema {
            node_type: "try_catch".to_string(),
            kind: NodeKind::Processor,
            description: "Error handling with fallback. Routes to catch port if _error present, otherwise passes through. Optional finally port always fires.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "catch_port": { "type": "string", "default": "error", "description": "Port to route errors to" },
                    "finally_port": { "type": "string", "description": "Port that always fires (optional)" }
                }
            }),
        },
    });

    // Retry (Processor) — retry logic with exponential backoff
    registry.register(NodeInfo {
        factory: || Box::new(retry_node::RetryNode::new()),
        schema: NodeSchema {
            node_type: "retry".to_string(),
            kind: NodeKind::Processor,
            description: "Retry logic with exponential backoff. Routes to retry/success/failed ports based on error state and retry count.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("retry"), PortSpec::new("success"), PortSpec::new("failed")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "max_retries": { "type": "integer", "default": 3 },
                    "backoff_ms": { "type": "integer", "default": 1000 },
                    "backoff_multiplier": { "type": "number", "default": 2.0 },
                    "max_backoff_ms": { "type": "integer", "default": 30000 }
                }
            }),
        },
    });

    // Accumulator (Processor) — collects multiple inputs into array
    registry.register(NodeInfo {
        factory: || Box::new(accumulator::AccumulatorNode::new()),
        schema: NodeSchema {
            node_type: "accumulator".to_string(),
            kind: NodeKind::Processor,
            description: "Collects multiple inputs into an array. Emits when target_count is reached, skips otherwise.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "target_count": { "type": "integer", "default": 10, "minimum": 1 },
                    "flush_on": { "type": "string", "enum": ["count", "timeout"], "default": "count" },
                    "timeout_ms": { "type": "integer", "default": 5000 }
                }
            }),
        },
    });

    // Debounce (Processor) — debounces rapid inputs
    registry.register(NodeInfo {
        factory: || Box::new(debounce_node::DebounceNode::new()),
        schema: NodeSchema {
            node_type: "debounce".to_string(),
            kind: NodeKind::Processor,
            description: "Debounces rapid inputs. Only emits if delay_ms has passed since last input.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "delay_ms": { "type": "integer", "default": 500, "description": "Debounce delay in milliseconds" }
                }
            }),
        },
    });

    // Parallel (Processor) — fan-out to multiple ports simultaneously
    registry.register(NodeInfo {
        factory: || Box::new(parallel_node::ParallelNode::new()),
        schema: NodeSchema {
            node_type: "parallel".to_string(),
            kind: NodeKind::Processor,
            description: "Fan-out to multiple ports simultaneously. Sends the same input data to all configured output ports for parallel execution paths.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "ports": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of output port names to fan-out to"
                    }
                },
                "required": ["ports"]
            }),
        },
    });

    // JWT (Processor) — simplified JWT token creation and verification
    registry.register(NodeInfo {
        factory: || Box::new(jwt_node::JwtNode::new()),
        schema: NodeSchema {
            node_type: "jwt".to_string(),
            kind: NodeKind::Processor,
            description: "Simplified JWT token creation and verification (FNV-based, NOT production crypto)".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "string", "enum": ["sign", "verify", "decode"], "default": "sign" },
                    "secret_env": { "type": "string", "default": "JWT_SECRET" },
                    "algorithm": { "type": "string", "default": "HS256" },
                    "expires_in": { "type": "integer", "default": 3600 }
                }
            }),
        },
    });

    // Rate Limit (Processor) — sliding window rate limiter
    registry.register(NodeInfo {
        factory: || Box::new(rate_limit::RateLimitNode::new()),
        schema: NodeSchema {
            node_type: "rate_limit".to_string(),
            kind: NodeKind::Processor,
            description: "Sliding window rate limiter. Multi-port: 'allowed' and 'rejected'.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("allowed"), PortSpec::new("rejected")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "max_requests": { "type": "integer", "default": 100 },
                    "window_ms": { "type": "integer", "default": 60000 },
                    "key_field": { "type": "string", "description": "Dot-path to rate limit key field" }
                }
            }),
        },
    });

    // CORS (Processor) — adds CORS headers to HTTP responses
    registry.register(NodeInfo {
        factory: || Box::new(cors_node::CorsNode::new()),
        schema: NodeSchema {
            node_type: "cors".to_string(),
            kind: NodeKind::Processor,
            description: "Adds CORS headers to HTTP responses via _cors_headers field".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "allowed_origins": { "type": "array", "items": { "type": "string" }, "default": ["*"] },
                    "allowed_methods": { "type": "array", "items": { "type": "string" }, "default": ["GET", "POST", "PUT", "DELETE"] },
                    "allowed_headers": { "type": "array", "items": { "type": "string" }, "default": ["Content-Type", "Authorization"] },
                    "max_age": { "type": "integer", "default": 86400 }
                }
            }),
        },
    });

    // Encrypt (Processor) — simple XOR-based encryption/encoding
    registry.register(NodeInfo {
        factory: || Box::new(encrypt_node::EncryptNode::new()),
        schema: NodeSchema {
            node_type: "encrypt".to_string(),
            kind: NodeKind::Processor,
            description: "Simple XOR-based encryption/encoding (NOT production crypto)".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "string", "enum": ["encrypt", "decrypt"], "default": "encrypt" },
                    "key_env": { "type": "string", "default": "ENCRYPT_KEY" }
                }
            }),
        },
    });

    // Cron (Service) — scheduled execution at fixed intervals
    registry.register(NodeInfo {
        factory: || Box::new(cron_node::CronNode::new()),
        schema: NodeSchema {
            node_type: "cron".to_string(),
            kind: NodeKind::Service,
            description: "Scheduled execution at fixed intervals. Triggers downstream with tick count and timestamp.".to_string(),
            inputs: vec![],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "interval_ms": { "type": "integer", "default": 60000 },
                    "max_iterations": { "type": "integer", "default": 0 }
                }
            }),
        },
    });

    // Email (Sink) — prepares email payloads for sending via HTTP API
    registry.register(NodeInfo {
        factory: || Box::new(email_node::EmailNode::new()),
        schema: NodeSchema {
            node_type: "email".to_string(),
            kind: NodeKind::Sink,
            description: "Prepares email payloads (generic + SendGrid-compatible) for sending via http_client".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "from": { "type": "string", "default": "noreply@example.com" }
                }
            }),
        },
    });

    // WebSocket (Service) — WebSocket server endpoint
    registry.register(NodeInfo {
        factory: || Box::new(websocket_node::WebSocketNode::new()),
        schema: NodeSchema {
            node_type: "websocket".to_string(),
            kind: NodeKind::Service,
            description: "WebSocket server endpoint. Full WS upgrade requires axum 'ws' feature.".to_string(),
            inputs: vec![],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "port": { "type": "integer", "default": 8080 },
                    "path": { "type": "string", "default": "/ws" }
                }
            }),
        },
    });

    // SSE (Processor) — Server-Sent Events formatter
    registry.register(NodeInfo {
        factory: || Box::new(sse_node::SseNode::new()),
        schema: NodeSchema {
            node_type: "sse".to_string(),
            kind: NodeKind::Processor,
            description: "Formats data as Server-Sent Events for http_server SSE endpoints".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "event_type": { "type": "string", "default": "message" },
                    "retry_ms": { "type": "integer", "default": 3000 }
                }
            }),
        },
    });

    // Regex (Processor) — pattern matching, extraction, replacement
    registry.register(NodeInfo {
        factory: || Box::new(regex_node::RegexNode::new()),
        schema: NodeSchema {
            node_type: "regex".to_string(),
            kind: NodeKind::Processor,
            description: "Pattern matching, extraction, and replacement on strings using regex-like patterns.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Regex pattern to match" },
                    "operation": { "type": "string", "enum": ["match", "extract", "replace", "split"], "default": "match" },
                    "replacement": { "type": "string", "description": "Replacement string (for replace operation)" }
                },
                "required": ["pattern"]
            }),
        },
    });

    // String (Processor) — string manipulation operations
    registry.register(NodeInfo {
        factory: || Box::new(string_node::StringNode::new()),
        schema: NodeSchema {
            node_type: "string".to_string(),
            kind: NodeKind::Processor,
            description: "String manipulation: split, join, trim, replace, uppercase, lowercase, slice, pad, repeat, contains, reverse, substring, length.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "string", "enum": ["split", "join", "trim", "replace", "uppercase", "lowercase", "slice", "pad_start", "pad_end", "repeat", "contains", "starts_with", "ends_with", "length", "reverse", "substring"] },
                    "delimiter": { "type": "string", "default": "," },
                    "from": { "type": "string" },
                    "to": { "type": "string" },
                    "start": { "type": "integer" },
                    "end": { "type": "integer" },
                    "length": { "type": "integer" },
                    "char": { "type": "string" },
                    "count": { "type": "integer" },
                    "search": { "type": "string" }
                },
                "required": ["operation"]
            }),
        },
    });

    // Math (Processor) — arithmetic and aggregation
    registry.register(NodeInfo {
        factory: || Box::new(math_node::MathNode::new()),
        schema: NodeSchema {
            node_type: "math".to_string(),
            kind: NodeKind::Processor,
            description: "Arithmetic and aggregation: add, subtract, multiply, divide, modulo, power, abs, floor, ceil, round, min, max, sum, avg, count, sqrt, log.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "string", "enum": ["add", "subtract", "multiply", "divide", "modulo", "power", "abs", "floor", "ceil", "round", "min", "max", "sum", "avg", "count", "sqrt", "log"] },
                    "value": { "type": "number", "description": "Operand for binary operations" }
                },
                "required": ["operation"]
            }),
        },
    });

    // Array (Processor) — array manipulation operations
    registry.register(NodeInfo {
        factory: || Box::new(array_node::ArrayNode::new()),
        schema: NodeSchema {
            node_type: "array".to_string(),
            kind: NodeKind::Processor,
            description: "Array operations: sort, reverse, slice, flatten, unique, find, length, first, last, push, pop, compact, zip, chunk, map_field, pluck, index_of, includes, take, skip.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "string", "enum": ["sort", "reverse", "slice", "flatten", "unique", "find", "length", "first", "last", "push", "pop", "compact", "zip", "chunk", "map_field", "pluck", "index_of", "includes", "take", "skip"] },
                    "start": { "type": "integer" },
                    "end": { "type": "integer" },
                    "field": { "type": "string" },
                    "value": { "description": "Value for find/push/index_of/includes" },
                    "size": { "type": "integer", "description": "Chunk size" },
                    "count": { "type": "integer", "description": "Count for take/skip" },
                    "order": { "type": "string", "enum": ["asc", "desc"], "default": "asc" }
                },
                "required": ["operation"]
            }),
        },
    });

    // JSON Parse (Processor) — parse, stringify, query, manipulate JSON
    registry.register(NodeInfo {
        factory: || Box::new(json_parse::JsonParseNode::new()),
        schema: NodeSchema {
            node_type: "json_parse".to_string(),
            kind: NodeKind::Processor,
            description: "Parse, stringify, query, set, delete JSON. Also: keys, values, entries, flatten, unflatten.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "string", "enum": ["parse", "stringify", "query", "set", "delete", "keys", "values", "entries", "flatten", "unflatten"] },
                    "path": { "type": "string", "description": "Dot-path for query/set/delete" },
                    "value": { "description": "Value to set (for set operation)" },
                    "pretty": { "type": "boolean", "default": false }
                },
                "required": ["operation"]
            }),
        },
    });

    // CSV (Processor) — parse and generate CSV
    registry.register(NodeInfo {
        factory: || Box::new(csv_node::CsvNode::new()),
        schema: NodeSchema {
            node_type: "csv".to_string(),
            kind: NodeKind::Processor,
            description: "Parse CSV strings to arrays of objects, or stringify arrays to CSV format.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "string", "enum": ["parse", "stringify"], "default": "parse" },
                    "headers": { "type": "boolean", "default": true },
                    "delimiter": { "type": "string", "default": "," }
                }
            }),
        },
    });

    // YAML (Processor) — parse and generate simple YAML
    registry.register(NodeInfo {
        factory: || Box::new(yaml_node::YamlNode::new()),
        schema: NodeSchema {
            node_type: "yaml".to_string(),
            kind: NodeKind::Processor,
            description: "Parse simple YAML to JSON, or stringify JSON to YAML format.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "string", "enum": ["parse", "stringify"], "default": "parse" }
                }
            }),
        },
    });

    // Base64 (Processor) — encode and decode base64
    registry.register(NodeInfo {
        factory: || Box::new(base64_node::Base64Node::new()),
        schema: NodeSchema {
            node_type: "base64".to_string(),
            kind: NodeKind::Processor,
            description: "Encode and decode base64 strings. Supports standard and URL-safe alphabets.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "string", "enum": ["encode", "decode"], "default": "encode" },
                    "url_safe": { "type": "boolean", "default": false }
                }
            }),
        },
    });

    // Hash (Processor) — non-cryptographic hashing (FNV-1a)
    registry.register(NodeInfo {
        factory: || Box::new(hash_node::HashNode::new()),
        schema: NodeSchema {
            node_type: "hash".to_string(),
            kind: NodeKind::Processor,
            description: "Non-cryptographic hashing using FNV-1a. Useful for partitioning and deduplication.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "algorithm": { "type": "string", "enum": ["fnv1a", "simple", "hash"], "default": "fnv1a" }
                }
            }),
        },
    });

    // === UTILITY NODES ===

    registry.register(NodeInfo {
        factory: || Box::new(uuid_node::UuidNode::new()),
        schema: NodeSchema { node_type: "uuid".to_string(), kind: NodeKind::Processor, description: "Generates UUID v4 values".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"field": {"type": "string", "default": "id"}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(timestamp_node::TimestampNode::new()),
        schema: NodeSchema { node_type: "timestamp".to_string(), kind: NodeKind::Processor, description: "Date/time operations: now, unix, from_unix, add, subtract, diff".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"operation": {"type": "string", "enum": ["now", "unix", "from_unix", "add", "subtract", "diff"]}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(env_node::EnvNode::new()),
        schema: NodeSchema { node_type: "env".to_string(), kind: NodeKind::Source, description: "Reads environment variables".to_string(), inputs: vec![], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"variables": {"type": "array"}, "variable": {"type": "string"}, "prefix": {"type": "string"}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(log_node::LogNode::new()),
        schema: NodeSchema { node_type: "log".to_string(), kind: NodeKind::Sink, description: "Structured logging with levels. Passes through data.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![], config_schema: serde_json::json!({"type": "object", "properties": {"level": {"type": "string", "enum": ["info","warn","error","debug","trace"]}, "message": {"type": "string"}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(validate_node::ValidateNode::new()),
        schema: NodeSchema { node_type: "validate".to_string(), kind: NodeKind::Processor, description: "Validates data against rules. Routes to valid/invalid port.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("valid"), PortSpec::new("invalid")], config_schema: serde_json::json!({"type": "object", "properties": {"rules": {"type": "object"}}, "required": ["rules"]}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(random_node::RandomNode::new()),
        schema: NodeSchema { node_type: "random".to_string(), kind: NodeKind::Processor, description: "Generates random values: integer, float, boolean, string, choice".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"type": {"type": "string", "enum": ["integer","float","boolean","string","choice"]}, "min": {"type": "number"}, "max": {"type": "number"}, "length": {"type": "integer"}, "choices": {"type": "array"}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(counter_node::CounterNode::new()),
        schema: NodeSchema { node_type: "counter".to_string(), kind: NodeKind::Processor, description: "Atomic counter: increment, decrement, get, reset, set".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"operation": {"type": "string", "enum": ["increment","decrement","get","reset","set"]}, "step": {"type": "integer"}, "initial": {"type": "integer"}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(sleep_node::SleepNode::new()),
        schema: NodeSchema { node_type: "sleep".to_string(), kind: NodeKind::Processor, description: "Delays execution then passes through input".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"duration_ms": {"type": "integer", "default": 1000}, "max_duration_ms": {"type": "integer", "default": 30000}}}) },
    });

    // === INTEGRATION NODES ===

    registry.register(NodeInfo {
        factory: || Box::new(cache_node::CacheNode::new()),
        schema: NodeSchema { node_type: "cache".to_string(), kind: NodeKind::Processor, description: "In-memory cache with TTL: get, set, delete, clear, has".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"operation": {"type": "string", "enum": ["get","set","delete","clear","has"]}, "ttl_ms": {"type": "integer", "default": 60000}}, "required": ["operation"]}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(queue_node::QueueNode::new()),
        schema: NodeSchema { node_type: "queue".to_string(), kind: NodeKind::Processor, description: "In-memory FIFO queue: enqueue, dequeue, peek, size, clear, drain".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"operation": {"type": "string", "enum": ["enqueue","dequeue","peek","size","clear","drain"]}, "max_size": {"type": "integer"}}, "required": ["operation"]}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(redirect_node::RedirectNode::new()),
        schema: NodeSchema { node_type: "redirect".to_string(), kind: NodeKind::Processor, description: "HTTP redirect response with URL template support".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"url": {"type": "string"}, "status": {"type": "integer", "default": 302}}, "required": ["url"]}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(session_node::SessionNode::new()),
        schema: NodeSchema { node_type: "session".to_string(), kind: NodeKind::Processor, description: "In-memory session management: create, get, set, delete, destroy".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"operation": {"type": "string", "enum": ["create","get","set","delete","destroy"]}, "ttl_ms": {"type": "integer"}}, "required": ["operation"]}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(image_resize::ImageResizeNode::new()),
        schema: NodeSchema { node_type: "image_resize".to_string(), kind: NodeKind::Processor, description: "Image metadata detection and resize/crop parameter generation".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"operation": {"type": "string", "enum": ["metadata","resize_params","crop_params","format_convert_params"]}, "width": {"type": "integer"}, "height": {"type": "integer"}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(webhook_node::WebhookNode::new()),
        schema: NodeSchema { node_type: "webhook".to_string(), kind: NodeKind::Processor, description: "Sends webhook HTTP requests with signing and retry".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"url": {"type": "string"}, "method": {"type": "string"}, "secret_env": {"type": "string"}, "retry_count": {"type": "integer"}}, "required": ["url"]}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(template_engine::TemplateEngineNode::new()),
        schema: NodeSchema { node_type: "template_engine".to_string(), kind: NodeKind::Processor, description: "Advanced templates with {{#each}}, {{#if}}/{{else}}, {{@index}}".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"template": {"type": "string"}}, "required": ["template"]}) },
    });

    // === AI/ML NODES ===

    registry.register(NodeInfo {
        factory: || Box::new(embedding_node::EmbeddingNode::new()),
        schema: NodeSchema { node_type: "embedding".to_string(), kind: NodeKind::Processor, description: "Generates text embeddings via character frequency analysis. Operations: embed, similarity, distance.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"operation": {"type": "string", "enum": ["embed","similarity","distance"]}, "dimensions": {"type": "integer", "default": 26}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(sentiment_node::SentimentNode::new()),
        schema: NodeSchema { node_type: "sentiment".to_string(), kind: NodeKind::Processor, description: "Keyword-based sentiment analysis. Returns score (-1 to 1), label, and word counts.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"field": {"type": "string", "default": "text"}, "custom_positive": {"type": "array"}, "custom_negative": {"type": "array"}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(summarize_node::SummarizeNode::new()),
        schema: NodeSchema { node_type: "summarize".to_string(), kind: NodeKind::Processor, description: "Extractive text summarization: sentences, truncate, keywords.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"operation": {"type": "string", "enum": ["sentences","truncate","keywords"], "default": "sentences"}, "max_sentences": {"type": "integer", "default": 3}, "max_length": {"type": "integer", "default": 200}, "top_n": {"type": "integer", "default": 10}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(classify_node::ClassifyNode::new()),
        schema: NodeSchema { node_type: "classify".to_string(), kind: NodeKind::Processor, description: "Rule-based text classification with configurable keyword categories.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"field": {"type": "string", "default": "text"}, "categories": {"type": "object"}, "threshold": {"type": "integer", "default": 1}}, "required": ["categories"]}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(extract_node::ExtractNode::new()),
        schema: NodeSchema { node_type: "extract".to_string(), kind: NodeKind::Processor, description: "Pattern-based extraction: emails, urls, numbers, dates, mentions, hashtags, IPs.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"extractors": {"type": "array"}, "field": {"type": "string", "default": "text"}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(translate_node::TranslateNode::new()),
        schema: NodeSchema { node_type: "translate".to_string(), kind: NodeKind::Processor, description: "Text transformation: reverse, pig_latin, rot13, morse, leetspeak.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"operation": {"type": "string", "enum": ["reverse","pig_latin","rot13","morse","leetspeak"]}, "field": {"type": "string", "default": "text"}}, "required": ["operation"]}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(prompt_chain_node::PromptChainNode::new()),
        schema: NodeSchema { node_type: "prompt_chain".to_string(), kind: NodeKind::Processor, description: "Chains multiple template prompts with {{variable}} substitution and context propagation.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"steps": {"type": "array"}, "initial_context": {"type": "object"}}, "required": ["steps"]}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(agent_loop_node::AgentLoopNode::new()),
        schema: NodeSchema { node_type: "agent_loop".to_string(), kind: NodeKind::Processor, description: "Agentic loop pattern: start, iterate, check, reset with max_iterations and termination.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"max_iterations": {"type": "integer", "default": 10}, "termination_field": {"type": "string"}, "operation": {"type": "string", "enum": ["start","iterate","check","reset"], "default": "iterate"}}}) },
    });

    // === AUTH/SECURITY NODES ===

    registry.register(NodeInfo {
        factory: || Box::new(api_key_node::ApiKeyNode::new()),
        schema: NodeSchema { node_type: "api_key".to_string(), kind: NodeKind::Processor, description: "API key validation, generation, and masking.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("valid"), PortSpec::new("invalid")], config_schema: serde_json::json!({"type": "object", "properties": {"operation": {"type": "string", "enum": ["validate","generate","mask"]}, "valid_keys": {"type": "array"}, "header_field": {"type": "string", "default": "x-api-key"}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(password_node::PasswordNode::new()),
        schema: NodeSchema { node_type: "password".to_string(), kind: NodeKind::Processor, description: "Password hashing (FNV), verification, and strength analysis.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"operation": {"type": "string", "enum": ["hash","verify","strength"]}, "salt": {"type": "string"}}, "required": ["operation"]}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(totp_node::TotpNode::new()),
        schema: NodeSchema { node_type: "totp".to_string(), kind: NodeKind::Processor, description: "Time-based OTP generation and verification (simplified).".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"operation": {"type": "string", "enum": ["generate","verify"], "default": "generate"}, "secret_env": {"type": "string", "default": "TOTP_SECRET"}, "interval": {"type": "integer", "default": 30}, "digits": {"type": "integer", "default": 6}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(circuit_breaker_node::CircuitBreakerNode::new()),
        schema: NodeSchema { node_type: "circuit_breaker".to_string(), kind: NodeKind::Processor, description: "Circuit breaker pattern: closed/open/half_open state transitions for fault tolerance.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"failure_threshold": {"type": "integer", "default": 5}, "reset_timeout_ms": {"type": "integer", "default": 30000}, "half_open_max": {"type": "integer", "default": 1}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(semaphore_node::SemaphoreNode::new()),
        schema: NodeSchema { node_type: "semaphore".to_string(), kind: NodeKind::Processor, description: "Concurrency limiter: acquire/release/status with atomic permit tracking.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"max_permits": {"type": "integer", "default": 5}, "operation": {"type": "string", "enum": ["acquire","release","status"]}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(deduplicate_node::DeduplicateNode::new()),
        schema: NodeSchema { node_type: "deduplicate".to_string(), kind: NodeKind::Processor, description: "Deduplication by key field with configurable window and TTL.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"key_field": {"type": "string"}, "window_size": {"type": "integer", "default": 1000}, "ttl_ms": {"type": "integer", "default": 0}}, "required": ["key_field"]}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(sanitize_node::SanitizeNode::new()),
        schema: NodeSchema { node_type: "sanitize".to_string(), kind: NodeKind::Processor, description: "Input sanitization: html, sql, xss, trim, all. Recursive string processing.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"operation": {"type": "string", "enum": ["html","sql","xss","trim","all"], "default": "all"}, "field": {"type": "string"}, "max_length": {"type": "integer"}}}) },
    });

    // === DATA PROCESSING NODES ===

    registry.register(NodeInfo {
        factory: || Box::new(xml_node::XmlNode::new()),
        schema: NodeSchema { node_type: "xml".to_string(), kind: NodeKind::Processor, description: "Simple XML parse/stringify for flat and one-level-nested elements.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"operation": {"type": "string", "enum": ["parse","stringify"], "default": "parse"}, "root_tag": {"type": "string", "default": "root"}, "field": {"type": "string", "default": "text"}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(markdown_node::MarkdownNode::new()),
        schema: NodeSchema { node_type: "markdown".to_string(), kind: NodeKind::Processor, description: "Markdown to HTML conversion: headings, bold, italic, code, lists, links, hr.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"field": {"type": "string", "default": "text"}, "wrap": {"type": "boolean", "default": true}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(diff_node::DiffNode::new()),
        schema: NodeSchema { node_type: "diff".to_string(), kind: NodeKind::Processor, description: "JSON diffing, patching, and equality checking.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"operation": {"type": "string", "enum": ["diff","patch","equals"], "default": "diff"}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(group_by_node::GroupByNode::new()),
        schema: NodeSchema { node_type: "group_by".to_string(), kind: NodeKind::Processor, description: "Group array items by field value with optional aggregations (count, sum, avg, min, max).".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"field": {"type": "string"}, "aggregate": {"type": "string", "enum": ["count","sum","avg","min","max","list"]}, "aggregate_field": {"type": "string"}}, "required": ["field"]}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(pivot_node::PivotNode::new()),
        schema: NodeSchema { node_type: "pivot".to_string(), kind: NodeKind::Processor, description: "Pivot (rows→columns), unpivot (columns→rows), and transpose operations.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"operation": {"type": "string", "enum": ["pivot","unpivot","transpose"]}, "row_field": {"type": "string"}, "column_field": {"type": "string"}, "value_field": {"type": "string"}, "key_name": {"type": "string"}, "value_name": {"type": "string"}, "exclude": {"type": "array"}}, "required": ["operation"]}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(lookup_node::LookupNode::new()),
        schema: NodeSchema { node_type: "lookup".to_string(), kind: NodeKind::Processor, description: "Lookup/join between input and static dataset. Modes: first (merge), all (array).".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"lookup_data": {"type": "array"}, "key_field": {"type": "string"}, "lookup_key": {"type": "string"}, "result_fields": {"type": "array"}, "mode": {"type": "string", "enum": ["first","all"], "default": "first"}}, "required": ["lookup_data", "key_field"]}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(url_encode_node::UrlEncodeNode::new()),
        schema: NodeSchema { node_type: "url_encode".to_string(), kind: NodeKind::Processor, description: "URL encode/decode, parse URLs to components, build URLs from parts.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"operation": {"type": "string", "enum": ["encode","decode","parse","build"], "default": "encode"}, "field": {"type": "string"}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(toml_node::TomlNode::new()),
        schema: NodeSchema { node_type: "toml".to_string(), kind: NodeKind::Processor, description: "TOML parse/stringify with sections, strings, numbers, booleans.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"operation": {"type": "string", "enum": ["parse","stringify"], "default": "parse"}, "field": {"type": "string", "default": "text"}}}) },
    });

    // === UI/FRONTEND NODES ===

    registry.register(NodeInfo {
        factory: || Box::new(form_node::FormNode::new()),
        schema: NodeSchema { node_type: "form".to_string(), kind: NodeKind::Processor, description: "HTML form builder with field types: text, email, password, number, textarea, select, checkbox, hidden.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"action": {"type": "string"}, "method": {"type": "string", "default": "POST"}, "fields": {"type": "array"}, "css_class": {"type": "string"}, "submit_text": {"type": "string", "default": "Submit"}}, "required": ["action", "fields"]}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(chart_node::ChartNode::new()),
        schema: NodeSchema { node_type: "chart".to_string(), kind: NodeKind::Processor, description: "SVG chart generation: bar, line, pie charts from data arrays.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"chart_type": {"type": "string", "enum": ["bar","line","pie"], "default": "bar"}, "title": {"type": "string"}, "x_field": {"type": "string"}, "y_field": {"type": "string"}, "width": {"type": "integer", "default": 400}, "height": {"type": "integer", "default": 300}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(table_node::TableNode::new()),
        schema: NodeSchema { node_type: "table".to_string(), kind: NodeKind::Processor, description: "HTML table generation from arrays with auto-column detection, striped rows, XSS escaping.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"columns": {"type": "array"}, "css_class": {"type": "string"}, "striped": {"type": "boolean", "default": true}, "sortable": {"type": "boolean", "default": false}, "caption": {"type": "string"}, "empty_text": {"type": "string", "default": "No data available"}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(markdown_render_node::MarkdownRenderNode::new()),
        schema: NodeSchema { node_type: "markdown_render".to_string(), kind: NodeKind::Processor, description: "Styled markdown rendering with light/dark themes for web display.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"theme": {"type": "string", "enum": ["light","dark"], "default": "light"}, "max_width": {"type": "string", "default": "800px"}, "font_family": {"type": "string"}, "code_theme": {"type": "string"}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(paginate_node::PaginateNode::new()),
        schema: NodeSchema { node_type: "paginate".to_string(), kind: NodeKind::Processor, description: "Array pagination with page metadata (total_pages, has_next, has_prev).".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"page_size": {"type": "integer", "default": 10}, "page_field": {"type": "string", "default": "page"}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(breadcrumb_node::BreadcrumbNode::new()),
        schema: NodeSchema { node_type: "breadcrumb".to_string(), kind: NodeKind::Processor, description: "Breadcrumb navigation HTML generator with path parsing.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"separator": {"type": "string", "default": " / "}, "items": {"type": "array"}, "home_label": {"type": "string", "default": "Home"}, "home_url": {"type": "string", "default": "/"}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(layout_node::LayoutNode::new()),
        schema: NodeSchema { node_type: "layout".to_string(), kind: NodeKind::Processor, description: "HTML5 page layout wrapper with header/nav/footer, CSS, and {{variable}} support.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"title": {"type": "string"}, "head_extra": {"type": "string"}, "body_class": {"type": "string"}, "header_html": {"type": "string"}, "footer_html": {"type": "string"}, "nav_html": {"type": "string"}, "css": {"type": "string"}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(meta_node::MetaNode::new()),
        schema: NodeSchema { node_type: "meta".to_string(), kind: NodeKind::Processor, description: "HTML meta tags and SEO metadata generation (Open Graph, canonical, robots).".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"defaults": {"type": "object"}}}) },
    });

    // === ADVANCED WORKFLOW NODES ===

    registry.register(NodeInfo {
        factory: || Box::new(batch_node::BatchNode::new()),
        schema: NodeSchema { node_type: "batch".to_string(), kind: NodeKind::Processor, description: "Batch processing: collects inputs into batches, emits when full or flushed.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"batch_size": {"type": "integer"}, "flush_field": {"type": "string"}}, "required": ["batch_size"]}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(throttle_node::ThrottleNode::new()),
        schema: NodeSchema { node_type: "throttle".to_string(), kind: NodeKind::Processor, description: "Throttle execution rate with configurable interval and drop/queue modes.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"interval_ms": {"type": "integer", "default": 1000}, "drop_mode": {"type": "string", "enum": ["skip","queue"], "default": "skip"}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(priority_queue_node::PriorityQueueNode::new()),
        schema: NodeSchema { node_type: "priority_queue".to_string(), kind: NodeKind::Processor, description: "Priority queue: enqueue/dequeue/peek/size/clear with priority-based ordering.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"priority_field": {"type": "string", "default": "priority"}, "max_size": {"type": "integer", "default": 100}, "operation": {"type": "string", "enum": ["enqueue","dequeue","peek","size","clear"]}}, "required": ["operation"]}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(aggregate_node::AggregateNode::new()),
        schema: NodeSchema { node_type: "aggregate".to_string(), kind: NodeKind::Processor, description: "Multi-field aggregation pipeline: sum, avg, min, max, count, first, last, concat.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"aggregations": {"type": "array"}, "input_field": {"type": "string"}}, "required": ["aggregations"]}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(event_emitter_node::EventEmitterNode::new()),
        schema: NodeSchema { node_type: "event_emitter".to_string(), kind: NodeKind::Processor, description: "Pub/sub event system: emit, subscribe, unsubscribe, list events.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"operation": {"type": "string", "enum": ["emit","subscribe","unsubscribe","list"]}, "event_name": {"type": "string"}, "events": {"type": "array"}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(metrics_node::MetricsNode::new()),
        schema: NodeSchema { node_type: "metrics".to_string(), kind: NodeKind::Processor, description: "Metrics collection: counters, gauges, histograms with percentile summaries.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"operation": {"type": "string", "enum": ["increment","decrement","set","get","record","summary"]}, "metric_name": {"type": "string"}, "metric_type": {"type": "string", "enum": ["counter","gauge","histogram"]}}}) },
    });
    registry.register(NodeInfo {
        factory: || Box::new(kv_store_node::KvStoreNode::new()),
        schema: NodeSchema { node_type: "kv_store".to_string(), kind: NodeKind::Processor, description: "Key-value store with namespaces: get, set, delete, list, has, count, clear.".to_string(), inputs: vec![PortSpec::new("default")], outputs: vec![PortSpec::new("default")], config_schema: serde_json::json!({"type": "object", "properties": {"namespace": {"type": "string", "default": "default"}, "operation": {"type": "string", "enum": ["get","set","delete","list","has","count","clear"]}}, "required": ["operation"]}) },
    });

    // === NEW NODES: Filling capability gaps ===

    // OAuth 2.0 (Processor) — authorization code flow, client credentials, token refresh, PKCE
    registry.register(NodeInfo {
        factory: || Box::new(oauth_node::OAuthNode::new()),
        schema: NodeSchema {
            node_type: "oauth".to_string(),
            kind: NodeKind::Processor,
            description: "OAuth 2.0 authorization: authorize_url, token_exchange, refresh, client_credentials, PKCE. Works with http_client for actual HTTP calls. Supports Google, GitHub, Microsoft presets.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "string", "enum": ["authorize_url", "token_exchange", "refresh", "client_credentials", "validate", "revoke", "pkce_authorize"] },
                    "provider": { "type": "string", "enum": ["google", "github", "microsoft", "custom"], "default": "custom" },
                    "authorize_url": { "type": "string", "description": "Authorization endpoint URL" },
                    "token_url": { "type": "string", "description": "Token endpoint URL" },
                    "redirect_uri": { "type": "string", "default": "http://localhost:3000/callback" },
                    "client_id_env": { "type": "string", "default": "OAUTH_CLIENT_ID" },
                    "client_secret_env": { "type": "string", "default": "OAUTH_CLIENT_SECRET" },
                    "scopes": { "type": "array", "items": { "type": "string" } }
                }
            }),
        },
    });

    // AES-256-CBC (Processor) — real AES encryption with PKCS7 padding
    registry.register(NodeInfo {
        factory: || Box::new(aes_node::AesNode::new()),
        schema: NodeSchema {
            node_type: "aes".to_string(),
            kind: NodeKind::Processor,
            description: "AES-256-CBC encryption/decryption. Pure Rust implementation with PKCS7 padding, key derivation from passphrase, hex-encoded output. Production-grade crypto.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "string", "enum": ["encrypt", "decrypt"], "default": "encrypt" },
                    "key_env": { "type": "string", "default": "AES_KEY", "description": "Environment variable containing the encryption passphrase" }
                }
            }),
        },
    });

    // Multipart (Processor) — build/parse multipart form data for file uploads
    registry.register(NodeInfo {
        factory: || Box::new(multipart_node::MultipartNode::new()),
        schema: NodeSchema {
            node_type: "multipart".to_string(),
            kind: NodeKind::Processor,
            description: "Multipart form data builder and parser. Build multipart/form-data bodies for file uploads via http_client, parse multipart responses, or URL-encode form fields.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "string", "enum": ["build", "parse", "url_encode"], "default": "build" }
                }
            }),
        },
    });

    // SQL Query (Processor) — in-process SQL engine over JSON data
    registry.register(NodeInfo {
        factory: || Box::new(sql_query_node::SqlQueryNode::new()),
        schema: NodeSchema {
            node_type: "sql_query".to_string(),
            kind: NodeKind::Processor,
            description: "In-process SQL engine over JSON data. SELECT, INSERT, UPDATE, DELETE with WHERE, ORDER BY, GROUP BY, JOIN, LIMIT, OFFSET, and aggregates (SUM, AVG, MIN, MAX, COUNT).".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "string", "enum": ["query", "select", "insert", "update", "delete", "create_table", "drop_table", "describe"] },
                    "tables": { "type": "object", "description": "Pre-populate tables with initial data (table_name → array of rows)" }
                }
            }),
        },
    });

    // Redis (Processor) — Redis-compatible in-memory data structure store
    registry.register(NodeInfo {
        factory: || Box::new(redis_client_node::RedisClientNode::new()),
        schema: NodeSchema {
            node_type: "redis".to_string(),
            kind: NodeKind::Processor,
            description: "Redis-compatible in-memory data structure store. Supports strings (GET/SET/INCR), hashes (HSET/HGET/HGETALL), lists (LPUSH/RPUSH/LPOP/RPOP/LRANGE), sets (SADD/SMEMBERS), sorted sets (ZADD/ZRANGE), TTL/EXPIRE, KEYS, MGET/MSET.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "string", "description": "Default Redis command" },
                    "prefix": { "type": "string", "description": "Key prefix for namespacing" },
                    "initial_data": { "type": "object", "description": "Pre-populate string keys" }
                }
            }),
        },
    });

    // Message Broker (Processor) — topic-based pub/sub with dead letter queues
    registry.register(NodeInfo {
        factory: || Box::new(message_broker_node::MessageBrokerNode::new()),
        schema: NodeSchema {
            node_type: "message_broker".to_string(),
            kind: NodeKind::Processor,
            description: "Message broker with topics, subscriptions, and delivery guarantees. Publish/subscribe, consume, ack/nack, dead letter queues, topic management, and stats.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "string", "enum": ["publish", "subscribe", "consume", "ack", "nack", "create_topic", "delete_topic", "list_topics", "unsubscribe", "dead_letters", "purge", "stats", "peek"] },
                    "topics": { "type": "array", "items": { "type": "string" }, "description": "Pre-create topics" },
                    "max_queue_size": { "type": "integer", "default": 10000 }
                }
            }),
        },
    });

    // State Machine (Processor) — finite state machine with guards and history
    registry.register(NodeInfo {
        factory: || Box::new(state_machine_node::StateMachineNode::new()),
        schema: NodeSchema {
            node_type: "state_machine".to_string(),
            kind: NodeKind::Processor,
            description: "Finite State Machine (FSM) for stateful workflows. Define states, transitions with events, guard conditions, and context data. Supports create, transition, get_state, history, reset. Overcomes DAG limitations for cyclic/iterative processes.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "string", "enum": ["create", "transition", "get_state", "set_context", "reset", "can_transition", "history", "destroy"] },
                    "states": { "type": "array", "items": { "type": "string" }, "description": "Valid state names" },
                    "initial_state": { "type": "string", "default": "initial" },
                    "transitions": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "from": { "type": "string", "description": "Source state (or * for any)" },
                                "event": { "type": "string", "description": "Event trigger" },
                                "to": { "type": "string", "description": "Target state" },
                                "guard_field": { "type": "string" },
                                "guard_op": { "type": "string", "enum": ["eq", "neq", "gt", "gte", "lt", "lte", "exists", "is_null", "contains"] },
                                "guard_value": { "description": "Value for guard comparison" }
                            },
                            "required": ["from", "event", "to"]
                        }
                    }
                }
            }),
        },
    });

    // WebSocket Enhanced (Processor) — rooms, broadcasting, connection management
    registry.register(NodeInfo {
        factory: || Box::new(websocket_enhanced_node::WebSocketEnhancedNode::new()),
        schema: NodeSchema {
            node_type: "ws".to_string(),
            kind: NodeKind::Processor,
            description: "Enhanced WebSocket manager with rooms, broadcasting, direct messaging, connection tracking, and message history. Operations: connect, disconnect, send, broadcast, join_room, leave_room, room_send, connections, rooms, history, kick.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "string", "enum": ["connect", "disconnect", "send", "broadcast", "join_room", "leave_room", "room_send", "connections", "rooms", "history", "kick"] },
                    "max_history": { "type": "integer", "default": 100 }
                }
            }),
        },
    });

    // === Production Readiness Nodes ===

    // Gzip compression (Processor) — gzip compress/decompress for HTTP responses
    registry.register(NodeInfo {
        factory: || Box::new(gzip_node::GzipNode::new()),
        schema: NodeSchema {
            node_type: "gzip".to_string(),
            kind: NodeKind::Processor,
            description: "Gzip compression/decompression using flate2. Compress text/JSON for HTTP responses (70-90% size reduction) or decompress incoming data. Base64 encoding for transport.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "string", "enum": ["compress", "decompress"], "default": "compress" },
                    "level": { "type": "integer", "minimum": 0, "maximum": 9, "default": 6, "description": "Compression level (0=none, 1=fast, 9=best)" }
                }
            }),
        },
    });

    // Health Check (Processor) — K8s/load balancer health probes + Prometheus metrics
    registry.register(NodeInfo {
        factory: || Box::new(health_check_node::HealthCheckNode::new()),
        schema: NodeSchema {
            node_type: "health_check".to_string(),
            kind: NodeKind::Processor,
            description: "Health check / readiness probe for production deployments. Tracks uptime, request count, error rate. Supports K8s liveness/readiness probes, custom health checks, and Prometheus metrics export.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "string", "enum": ["health", "ready", "live", "metrics", "set_ready", "set_alive", "record_request", "record_error", "register_check", "update_check"] },
                    "version": { "type": "string", "description": "Application version string" }
                }
            }),
        },
    });

    // Request ID (Processor) — correlation ID for distributed tracing
    registry.register(NodeInfo {
        factory: || Box::new(request_id_node::RequestIdNode::new()),
        schema: NodeSchema {
            node_type: "request_id".to_string(),
            kind: NodeKind::Processor,
            description: "Request ID / correlation ID generator for distributed tracing. Generates unique IDs, propagates existing IDs from upstream, adds response headers. Essential for production debugging.".to_string(),
            inputs: vec![PortSpec::new("default")],
            outputs: vec![PortSpec::new("default")],
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "prefix": { "type": "string", "default": "req", "description": "ID prefix" },
                    "header_name": { "type": "string", "default": "x-request-id", "description": "HTTP header name for the request ID" }
                }
            }),
        },
    });
}
