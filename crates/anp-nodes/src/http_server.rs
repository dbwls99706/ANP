use std::collections::HashMap;
use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;
use serde::Deserialize;

/// HTTP Server node — a Service node that runs an axum-based HTTP server
///
/// Config example:
/// ```json
/// {
///   "port": 3000,
///   "routes": [
///     { "path": "/users", "method": "GET", "output_port": "users_get" },
///     { "path": "/users", "method": "POST", "output_port": "users_post" },
///     { "path": "/health", "output_port": "health" }
///   ]
/// }
/// ```
///
/// Note: In v0.1, the HTTP server node runs in batch mode as a simple mock.
/// Full Service node event-loop behavior is planned for v0.2.
pub struct HttpServerNode {
    port: u16,
    routes: Vec<RouteConfig>,
}

#[derive(Debug, Clone, Deserialize)]
struct RouteConfig {
    path: String,
    #[serde(default = "default_method")]
    method: String,
    output_port: String,
}

fn default_method() -> String {
    "ANY".to_string()
}

impl Default for HttpServerNode {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpServerNode {
    pub fn new() -> Self {
        Self {
            port: 3000,
            routes: Vec::new(),
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for HttpServerNode {
    fn node_type(&self) -> &str {
        "http_server"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Service
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        self.port = config
            .get("port")
            .and_then(|v| v.as_u64())
            .unwrap_or(3000) as u16;

        if let Some(routes) = config.get("routes") {
            self.routes = serde_json::from_value(routes.clone())
                .map_err(|e| NodeError::InvalidConfig(format!("invalid routes config: {}", e)))?;
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        // In v0.1 batch mode, the http_server produces a description of itself
        // Full event-loop Service behavior is planned for v0.2
        let route_info: Vec<serde_json::Value> = self
            .routes
            .iter()
            .map(|r| {
                serde_json::json!({
                    "path": r.path,
                    "method": r.method,
                    "output_port": r.output_port
                })
            })
            .collect();

        let mut ports = HashMap::new();

        // If input has a simulated request, route it
        if let Some(req_path) = input.get("path").and_then(|v| v.as_str()) {
            let req_method = input
                .get("method")
                .and_then(|v| v.as_str())
                .unwrap_or("GET");

            for route in &self.routes {
                let method_matches =
                    route.method == "ANY" || route.method.eq_ignore_ascii_case(req_method);
                if route.path == req_path && method_matches {
                    ports.insert(route.output_port.clone(), input.clone());
                }
            }
        }

        // Always include server info on default port
        ports.insert(
            "default".to_string(),
            serde_json::json!({
                "type": "http_server",
                "port": self.port,
                "routes": route_info,
                "status": "ready"
            }),
        );

        Ok(NodeOutput::multi(ports)
            .with_log(format!(
                "HTTP server configured on port {} with {} routes",
                self.port,
                self.routes.len()
            )))
    }
}
