use std::sync::Arc;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult, PipelineTrigger};
use anp_core::runtime::context::ExecutionContext;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{Method, StatusCode};
use axum::http::HeaderValue;
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
        let port_val = config
            .get("port")
            .and_then(|v| v.as_u64())
            .unwrap_or(3000);

        if port_val > u16::MAX as u64 {
            return Err(NodeError::InvalidConfig(format!(
                "port {} exceeds maximum value {}",
                port_val,
                u16::MAX
            )));
        }
        self.port = port_val as u16;

        if let Some(routes) = config.get("routes") {
            self.routes = serde_json::from_value(routes.clone())
                .map_err(|e| NodeError::InvalidConfig(format!("invalid routes config: {}", e)))?;

            // Validate route output_ports are non-empty
            for route in &self.routes {
                if route.output_port.is_empty() {
                    return Err(NodeError::InvalidConfig(format!(
                        "route {} {} has empty output_port",
                        route.method, route.path
                    )));
                }
            }
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

/// Simple percent-decoding for query parameters
fn percent_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next().unwrap_or(b'0');
            let lo = chars.next().unwrap_or(b'0');
            let hex = [hi, lo];
            if let Ok(s) = std::str::from_utf8(&hex) {
                if let Ok(val) = u8::from_str_radix(s, 16) {
                    result.push(val as char);
                    continue;
                }
            }
            result.push('%');
            result.push(hi as char);
            result.push(lo as char);
        } else if b == b'+' {
            result.push(' ');
        } else {
            result.push(b as char);
        }
    }
    result
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

    // Find matching route — supports exact match and path params (e.g., /users/:id)
    let mut path_params = serde_json::Map::new();
    let matched_route = state.routes.iter().find(|r| {
        let method_match = r.method.eq_ignore_ascii_case("ANY")
            || r.method.eq_ignore_ascii_case(method_str);
        if !method_match {
            return false;
        }

        // Exact match
        if r.path == path {
            return true;
        }

        // Path parameter matching (e.g., /users/:id matches /users/42)
        let route_parts: Vec<&str> = r.path.split('/').collect();
        let path_parts: Vec<&str> = path.split('/').collect();
        if route_parts.len() != path_parts.len() {
            return false;
        }

        let mut params = serde_json::Map::new();
        for (route_part, path_part) in route_parts.iter().zip(path_parts.iter()) {
            if let Some(param_name) = route_part.strip_prefix(':') {
                params.insert(param_name.to_string(), serde_json::json!(path_part));
            } else if route_part != path_part {
                return false;
            }
        }

        path_params = params;
        true
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
            )
                .into_response();
        }
    };

    // Parse body as JSON — return 400 for invalid JSON instead of silently accepting
    let body_json: serde_json::Value = if body.is_empty() {
        serde_json::json!({})
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    axum::Json(serde_json::json!({
                        "error": "invalid JSON body",
                        "detail": e.to_string()
                    })),
                )
                    .into_response();
            }
        }
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

        // Add path parameters (e.g., /users/:id → {"_params": {"id": "42"}})
        if !path_params.is_empty() {
            // Also inject path params at top level for easy access
            for (k, v) in &path_params {
                obj.insert(k.clone(), v.clone());
            }
            obj.insert(
                "_params".to_string(),
                serde_json::Value::Object(path_params),
            );
        }

        // Parse query parameters (e.g., ?page=1&limit=10)
        if let Some(query) = uri.query() {
            let mut query_map = serde_json::Map::new();
            for pair in query.split('&') {
                if let Some((key, value)) = pair.split_once('=') {
                    let decoded_key = percent_decode(key);
                    let decoded_value = percent_decode(value);
                    query_map
                        .insert(decoded_key, serde_json::Value::String(decoded_value));
                }
            }
            if !query_map.is_empty() {
                obj.insert("_query".to_string(), serde_json::Value::Object(query_map));
            }
        }
    }

    match state
        .trigger
        .trigger(&state.node_id, &output_port, trigger_data)
        .await
    {
        Ok(result) => {
            // Check if the pipeline result requests a specific content type
            // (e.g., html_template node sets _content_type to "text/html")
            if let Some(content_type) = result.get("_content_type").and_then(|v| v.as_str()) {
                let body = if content_type.starts_with("text/html") {
                    result.get("html").and_then(|v| v.as_str()).unwrap_or("").to_string()
                } else {
                    result.get("body").and_then(|v| v.as_str()).unwrap_or("").to_string()
                };

                let mut response = axum::response::Response::new(axum::body::Body::from(body));
                if let Ok(val) = HeaderValue::from_str(content_type) {
                    response.headers_mut().insert("content-type", val);
                }
                response.into_response()
            } else {
                (StatusCode::OK, axum::Json(result)).into_response()
            }
        }
        Err(e) => {
            error!(error = %e, "pipeline trigger failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_init_default_port() {
        let mut node = HttpServerNode::new();
        node.init(&serde_json::json!({})).await.unwrap();
        assert_eq!(node.port, 3000);
    }

    #[tokio::test]
    async fn test_init_custom_port() {
        let mut node = HttpServerNode::new();
        node.init(&serde_json::json!({"port": 8080})).await.unwrap();
        assert_eq!(node.port, 8080);
    }

    #[tokio::test]
    async fn test_init_port_too_large() {
        let mut node = HttpServerNode::new();
        let result = node.init(&serde_json::json!({"port": 99999})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_init_routes() {
        let mut node = HttpServerNode::new();
        node.init(&serde_json::json!({
            "routes": [
                {"path": "/api", "method": "GET", "output_port": "api"},
                {"path": "/data", "output_port": "data"}
            ]
        }))
        .await
        .unwrap();
        assert_eq!(node.routes.len(), 2);
        assert_eq!(node.routes[0].path, "/api");
        assert_eq!(node.routes[0].method, "GET");
        assert_eq!(node.routes[1].method, "ANY"); // default
    }

    #[tokio::test]
    async fn test_init_empty_output_port() {
        let mut node = HttpServerNode::new();
        let result = node
            .init(&serde_json::json!({
                "routes": [{"path": "/", "output_port": ""}]
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_batch_mode() {
        let mut node = HttpServerNode::new();
        node.init(&serde_json::json!({
            "port": 4000,
            "routes": [{"path": "/", "output_port": "page"}]
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::json!({}), &ctx).await.unwrap();
        let result = &output.ports["default"];
        assert_eq!(result["type"], "http_server");
        assert_eq!(result["port"], 4000);
        assert_eq!(result["status"], "mock");
        assert_eq!(result["routes"][0]["path"], "/");
    }

    #[test]
    fn test_percent_decode_basic() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(percent_decode("foo+bar"), "foo bar");
        assert_eq!(percent_decode("100%25"), "100%");
        assert_eq!(percent_decode("no+encoding"), "no encoding");
    }

    #[test]
    fn test_percent_decode_special_chars() {
        assert_eq!(percent_decode("%3C%3E"), "<>");
        assert_eq!(percent_decode("a%26b"), "a&b");
    }

    #[test]
    fn test_path_param_matching() {
        // Test the path parameter pattern matching logic
        let route = RouteConfig {
            path: "/users/:id".to_string(),
            method: "GET".to_string(),
            output_port: "get_user".to_string(),
        };

        let route_parts: Vec<&str> = route.path.split('/').collect();
        let path_parts: Vec<&str> = "/users/42".split('/').collect();
        assert_eq!(route_parts.len(), path_parts.len());

        let mut params = serde_json::Map::new();
        let mut matches = true;
        for (rp, pp) in route_parts.iter().zip(path_parts.iter()) {
            if let Some(param_name) = rp.strip_prefix(':') {
                params.insert(param_name.to_string(), serde_json::json!(pp));
            } else if rp != pp {
                matches = false;
            }
        }
        assert!(matches);
        assert_eq!(params["id"], "42");
    }

    #[test]
    fn test_path_param_multiple() {
        let route_path = "/api/:version/users/:id";
        let request_path = "/api/v2/users/99";

        let route_parts: Vec<&str> = route_path.split('/').collect();
        let path_parts: Vec<&str> = request_path.split('/').collect();
        assert_eq!(route_parts.len(), path_parts.len());

        let mut params = serde_json::Map::new();
        for (rp, pp) in route_parts.iter().zip(path_parts.iter()) {
            if let Some(param_name) = rp.strip_prefix(':') {
                params.insert(param_name.to_string(), serde_json::json!(pp));
            }
        }
        assert_eq!(params["version"], "v2");
        assert_eq!(params["id"], "99");
    }

    #[test]
    fn test_route_config_deserialization() {
        let json = serde_json::json!({
            "path": "/test",
            "output_port": "out"
        });
        let route: RouteConfig = serde_json::from_value(json).unwrap();
        assert_eq!(route.path, "/test");
        assert_eq!(route.method, "ANY");
        assert_eq!(route.output_port, "out");
    }
}
