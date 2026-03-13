use std::sync::Arc;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult, PipelineTrigger};
use anp_core::runtime::context::ExecutionContext;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{Method, StatusCode};
use axum::response::IntoResponse;
use axum::Router;
use serde::Deserialize;
use tracing::{error, info};

/// HTTP Server node — a Service node that runs an axum-based HTTP server
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

/// Shared state passed to axum handlers
#[derive(Clone)]
struct AppState {
    trigger: Arc<dyn PipelineTrigger>,
    node_id: String,
    routes: Vec<RouteConfig>,
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

    async fn start_service(
        &self,
        node_id: &str,
        trigger: Arc<dyn PipelineTrigger>,
    ) -> NodeResult<()> {
        let state = AppState {
            trigger,
            node_id: node_id.to_string(),
            routes: self.routes.clone(),
        };

        // Use a single fallback handler that matches all routes
        // Limit request body to 1MB to prevent memory exhaustion
        let app = Router::new()
            .fallback(handle_request)
            .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024))
            .with_state(state);

        let port = self.port;
        let addr = format!("0.0.0.0:{}", port);

        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| NodeError::Other(format!("failed to bind to {}: {}", addr, e)))?;

        info!(port = port, "HTTP server listening");

        // Spawn the server in a background task
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                error!(error = %e, "HTTP server error");
            }
        });

        Ok(())
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        // Service nodes should not be executed directly in service mode.
        // In batch mode, return server info as a mock.
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

        Ok(NodeOutput::data(serde_json::json!({
            "type": "http_server",
            "port": self.port,
            "routes": route_info,
            "status": "mock"
        })))
    }
}

/// Handle all incoming HTTP requests by matching against configured routes
async fn handle_request(
    State(state): State<AppState>,
    method: Method,
    uri: axum::http::Uri,
    body: Bytes,
) -> impl IntoResponse {
    let path = uri.path();
    let method_str = method.as_str();

    // Find matching route
    let matched_route = state.routes.iter().find(|r| {
        let path_match = r.path == path;
        let method_match = r.method.eq_ignore_ascii_case("ANY")
            || r.method.eq_ignore_ascii_case(method_str);
        path_match && method_match
    });

    let output_port = match matched_route {
        Some(route) => route.output_port.clone(),
        None => {
            return (
                StatusCode::NOT_FOUND,
                axum::Json(serde_json::json!({
                    "error": "not found",
                    "path": path,
                    "method": method_str
                })),
            );
        }
    };

    // Parse body as JSON (or empty object)
    let body_json: serde_json::Value = if body.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_slice(&body).unwrap_or_else(|_| {
            serde_json::json!({ "raw": String::from_utf8_lossy(&body) })
        })
    };

    // Build the trigger data: merge body with request metadata.
    // The body fields are at top-level so downstream nodes can consume them directly.
    let mut trigger_data = if body_json.is_object() {
        body_json
    } else {
        serde_json::json!({ "body": body_json })
    };

    // Add request metadata
    if let Some(obj) = trigger_data.as_object_mut() {
        obj.insert("_method".to_string(), serde_json::json!(method_str));
        obj.insert("_path".to_string(), serde_json::json!(path));
        obj.insert("_port".to_string(), serde_json::json!(output_port));
    }

    match state.trigger.trigger(&state.node_id, &output_port, trigger_data).await {
        Ok(result) => (StatusCode::OK, axum::Json(result)),
        Err(e) => {
            error!(error = %e, "pipeline trigger failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    }
}
