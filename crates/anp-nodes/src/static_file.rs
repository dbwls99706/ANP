use std::path::{Path, PathBuf};

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Static File node — reads a static file and outputs it with proper content type.
///
/// Designed for serving CSS, JS, images, and other static assets through `http_server`.
///
/// Config example:
/// ```json
/// {
///   "path": "./static/style.css"
/// }
/// ```
///
/// Can also receive the path dynamically via input: `{ "path": "style.css" }`
/// with a configured `base_dir` for security.
///
/// Output: `{ "body": "<file contents>", "_content_type": "text/css" }`
pub struct StaticFileNode {
    path: Option<String>,
    base_dir: Option<String>,
}

impl Default for StaticFileNode {
    fn default() -> Self {
        Self::new()
    }
}

impl StaticFileNode {
    pub fn new() -> Self {
        Self {
            path: None,
            base_dir: None,
        }
    }
}

/// Detect content type from file extension
fn content_type_for(path: &str) -> &'static str {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    match ext.to_lowercase().as_str() {
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "application/javascript",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "txt" => "text/plain",
        "xml" => "application/xml",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

/// Validate that the resolved path doesn't escape the base directory (path traversal prevention)
fn validate_path(base: &Path, requested: &str) -> Result<PathBuf, NodeError> {
    let resolved = base.join(requested);
    let canonical_base = base
        .canonicalize()
        .map_err(|e| NodeError::ExecutionFailed(format!("cannot resolve base directory: {}", e)))?;
    let canonical_path = resolved
        .canonicalize()
        .map_err(|e| NodeError::ExecutionFailed(format!("cannot resolve file path: {}", e)))?;

    if !canonical_path.starts_with(&canonical_base) {
        return Err(NodeError::ExecutionFailed(
            "path traversal denied: resolved path is outside base directory".into(),
        ));
    }

    Ok(canonical_path)
}

#[async_trait::async_trait]
impl NodeExecutor for StaticFileNode {
    fn node_type(&self) -> &str {
        "static_file"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(path) = config.get("path").and_then(|v| v.as_str()) {
            self.path = Some(path.to_string());
        }

        if let Some(base) = config.get("base_dir").and_then(|v| v.as_str()) {
            self.base_dir = Some(base.to_string());
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        // Determine file path: config.path > input.path
        let file_path = self
            .path
            .as_deref()
            .or_else(|| input.get("path").and_then(|v| v.as_str()))
            .ok_or_else(|| {
                NodeError::MissingInput("no file path provided in config or input".into())
            })?;

        // Resolve and validate path
        let resolved = if let Some(base) = &self.base_dir {
            validate_path(Path::new(base), file_path)?
        } else {
            let path = Path::new(file_path);
            // Basic traversal check even without base_dir
            if file_path.contains("..") {
                return Err(NodeError::ExecutionFailed(
                    "path traversal denied: '..' not allowed".into(),
                ));
            }
            path.to_path_buf()
        };

        let content_type = content_type_for(file_path);

        // Read file
        let body = tokio::fs::read_to_string(&resolved).await.map_err(|e| {
            NodeError::ExecutionFailed(format!("failed to read '{}': {}", resolved.display(), e))
        })?;

        Ok(NodeOutput::data(serde_json::json!({
            "body": body,
            "_content_type": content_type
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_type_detection() {
        assert_eq!(content_type_for("style.css"), "text/css");
        assert_eq!(content_type_for("app.js"), "application/javascript");
        assert_eq!(content_type_for("index.html"), "text/html");
        assert_eq!(content_type_for("logo.png"), "image/png");
        assert_eq!(content_type_for("photo.jpg"), "image/jpeg");
        assert_eq!(content_type_for("icon.svg"), "image/svg+xml");
        assert_eq!(content_type_for("data.json"), "application/json");
        assert_eq!(content_type_for("font.woff2"), "font/woff2");
        assert_eq!(content_type_for("unknown.xyz"), "application/octet-stream");
    }

    #[test]
    fn test_content_type_case_insensitive() {
        assert_eq!(content_type_for("STYLE.CSS"), "text/css");
        assert_eq!(content_type_for("app.JS"), "application/javascript");
    }

    #[tokio::test]
    async fn test_init_with_path() {
        let mut node = StaticFileNode::new();
        node.init(&serde_json::json!({"path": "index.html", "base_dir": "/tmp"}))
            .await
            .unwrap();
        assert_eq!(node.path.as_deref(), Some("index.html"));
        assert_eq!(node.base_dir.as_deref(), Some("/tmp"));
    }

    #[tokio::test]
    async fn test_path_traversal_rejected() {
        let mut node = StaticFileNode::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let result = node
            .execute(serde_json::json!({"path": "../../../etc/passwd"}), &ctx)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_missing_path() {
        let mut node = StaticFileNode::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let result = node.execute(serde_json::json!({}), &ctx).await;
        assert!(result.is_err());
    }
}
