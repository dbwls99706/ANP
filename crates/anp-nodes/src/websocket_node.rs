use std::sync::Arc;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult, PipelineTrigger};
use anp_core::runtime::context::ExecutionContext;

/// WebSocket node — WebSocket server endpoint.
///
/// Config:
/// ```json
/// {
///   "port": 8080,
///   "path": "/ws"
/// }
/// ```
///
/// Kind: Service — spawns an HTTP server with the configured path.
///
/// NOTE: Full WebSocket upgrade support requires the `ws` feature on axum
/// (which pulls in `tokio-tungstenite` and `sha1`). Since that feature
/// is not currently enabled in the workspace, this node provides a basic
/// HTTP endpoint that triggers the downstream pipeline on each request
/// and returns the pipeline result. Enable `axum/ws` in Cargo.toml for
/// full WebSocket upgrade support with persistent connections.
pub struct WebSocketNode {
    port: u16,
    path: String,
}

impl Default for WebSocketNode {
    fn default() -> Self {
        Self::new()
    }
}

impl WebSocketNode {
    pub fn new() -> Self {
        Self {
            port: 8080,
            path: "/ws".to_string(),
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for WebSocketNode {
    fn node_type(&self) -> &str {
        "websocket"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Service
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(port) = config.get("port").and_then(|v| v.as_u64()) {
            if port > u16::MAX as u64 {
                return Err(NodeError::InvalidConfig(format!(
                    "port {} exceeds maximum value {}",
                    port,
                    u16::MAX
                )));
            }
            self.port = port as u16;
        }
        if let Some(path) = config.get("path").and_then(|v| v.as_str()) {
            self.path = path.to_string();
        }
        Ok(())
    }

    async fn start_service(
        &self,
        node_id: &str,
        trigger: Arc<dyn PipelineTrigger>,
    ) -> NodeResult<()> {
        use axum::routing::get;
        use axum::Router;

        let port = self.port;
        let node_id = node_id.to_string();
        let trigger_clone = trigger.clone();
        let node_id_clone = node_id.clone();

        let app = Router::new().route(
            &self.path.clone(),
            get(move || {
                let trigger = trigger_clone.clone();
                let nid = node_id_clone.clone();
                async move {
                    // Trigger downstream pipeline with connection metadata
                    let data = serde_json::json!({
                        "event": "connection",
                        "timestamp_ms": std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64,
                        "message": "WebSocket endpoint active"
                    });
                    match trigger.trigger(&nid, "default", data).await {
                        Ok(result) => axum::Json(result),
                        Err(e) => axum::Json(serde_json::json!({
                            "error": e.to_string()
                        })),
                    }
                }
            }),
        );

        let addr = format!("0.0.0.0:{}", port);
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| NodeError::Other(format!("failed to bind to {}: {}", addr, e)))?;

        tracing::info!(
            port = port,
            "WebSocket server listening (HTTP fallback — enable axum 'ws' feature for full WS support)"
        );

        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                tracing::error!(error = %e, "WebSocket server error");
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
        Ok(NodeOutput::data(serde_json::json!({
            "type": "websocket",
            "port": self.port,
            "path": self.path,
            "status": "mock",
            "_note": "Full WebSocket support requires axum 'ws' feature"
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_init_defaults() {
        let mut node = WebSocketNode::new();
        node.init(&serde_json::json!({})).await.unwrap();
        assert_eq!(node.port, 8080);
        assert_eq!(node.path, "/ws");
    }

    #[tokio::test]
    async fn test_init_custom() {
        let mut node = WebSocketNode::new();
        node.init(&serde_json::json!({
            "port": 9090,
            "path": "/socket"
        }))
        .await
        .unwrap();
        assert_eq!(node.port, 9090);
        assert_eq!(node.path, "/socket");
    }

    #[tokio::test]
    async fn test_init_port_too_large() {
        let mut node = WebSocketNode::new();
        let result = node.init(&serde_json::json!({"port": 99999})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_batch_mode() {
        let mut node = WebSocketNode::new();
        node.init(&serde_json::json!({
            "port": 8081,
            "path": "/ws"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::json!({}), &ctx).await.unwrap();
        let result = &output.ports["default"];
        assert_eq!(result["type"], "websocket");
        assert_eq!(result["port"], 8081);
        assert_eq!(result["path"], "/ws");
        assert_eq!(result["status"], "mock");
    }

    #[test]
    fn test_node_type_and_kind() {
        let node = WebSocketNode::new();
        assert_eq!(node.node_type(), "websocket");
        assert_eq!(node.node_kind(), NodeKind::Service);
    }
}
