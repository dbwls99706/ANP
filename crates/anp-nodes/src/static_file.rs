use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Cached file entry with content, metadata, and ETag
struct CachedFile {
    content: String,
    content_type: &'static str,
    etag: String,
    cached_at: Instant,
}

/// Static File node — reads a static file and outputs it with proper content type.
///
/// Now with in-memory caching and ETag support for maximum performance.
/// Cached files are served without disk I/O. TTL-based invalidation ensures freshness.
///
/// Config example:
/// ```json
/// {
///   "path": "./static/style.css",
///   "cache_ttl_secs": 300
/// }
/// ```
///
/// Output: `{ "body": "<file contents>", "_content_type": "text/css", "_etag": "..." }`
pub struct StaticFileNode {
    path: Option<String>,
    base_dir: Option<String>,
    cache_ttl_secs: u64,
    cache: Arc<RwLock<HashMap<String, CachedFile>>>,
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
            cache_ttl_secs: 300, // 5 minutes default
            cache: Arc::new(RwLock::new(HashMap::new())),
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

/// Validate that the resolved path doesn't escape the base directory (path traversal prevention).
/// Also rejects symlinks that point outside the base directory.
fn validate_path(base: &Path, requested: &str) -> Result<PathBuf, NodeError> {
    // Reject path components that could escape
    if requested.contains("..") {
        return Err(NodeError::ExecutionFailed(
            "path traversal denied: '..' not allowed".into(),
        ));
    }

    let resolved = base.join(requested);

    // Check if the file is a symlink pointing outside base_dir
    if resolved.is_symlink() {
        let link_target = std::fs::read_link(&resolved)
            .map_err(|e| NodeError::ExecutionFailed(format!("cannot read symlink: {}", e)))?;
        // If symlink target is relative, resolve it relative to the symlink's parent
        let absolute_target = if link_target.is_relative() {
            resolved.parent().unwrap_or(base).join(&link_target)
        } else {
            link_target
        };
        let canonical_target = absolute_target.canonicalize().map_err(|e| {
            NodeError::ExecutionFailed(format!("cannot resolve symlink target: {}", e))
        })?;
        let canonical_base = base.canonicalize().map_err(|e| {
            NodeError::ExecutionFailed(format!("cannot resolve base directory: {}", e))
        })?;
        if !canonical_target.starts_with(&canonical_base) {
            return Err(NodeError::ExecutionFailed(
                "path traversal denied: symlink target is outside base directory".into(),
            ));
        }
    }

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

        if let Some(ttl) = config.get("cache_ttl_secs").and_then(|v| v.as_u64()) {
            self.cache_ttl_secs = ttl;
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
        let cache_key = resolved.to_string_lossy().to_string();

        // Check if client sent If-None-Match (ETag)
        let client_etag = input
            .get("_if_none_match")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(&cache_key) {
                let age = cached.cached_at.elapsed().as_secs();
                if age < self.cache_ttl_secs {
                    // ETag match → 304 Not Modified
                    if !client_etag.is_empty() && client_etag == cached.etag {
                        return Ok(NodeOutput::data(serde_json::json!({
                            "_status": 304,
                            "_content_type": cached.content_type,
                            "_etag": cached.etag,
                            "_cache": "hit",
                        })));
                    }
                    return Ok(NodeOutput::data(serde_json::json!({
                        "body": cached.content,
                        "_content_type": cached.content_type,
                        "_etag": cached.etag,
                        "_cache": "hit",
                        "_cache_control": format!("public, max-age={}", self.cache_ttl_secs),
                    })));
                }
            }
        }

        // Cache miss — read from disk
        let body = tokio::fs::read_to_string(&resolved).await.map_err(|e| {
            NodeError::ExecutionFailed(format!("failed to read '{}': {}", resolved.display(), e))
        })?;

        // Generate ETag from content hash (FNV-1a)
        let etag = {
            let mut h: u64 = 14695981039346656037;
            for &b in body.as_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(1099511628211);
            }
            format!("\"{:016x}\"", h)
        };

        // Store in cache
        {
            let mut cache = self.cache.write().await;
            cache.insert(
                cache_key,
                CachedFile {
                    content: body.clone(),
                    content_type,
                    etag: etag.clone(),
                    cached_at: Instant::now(),
                },
            );
        }

        // ETag match → 304
        if !client_etag.is_empty() && client_etag == etag {
            return Ok(NodeOutput::data(serde_json::json!({
                "_status": 304,
                "_content_type": content_type,
                "_etag": etag,
                "_cache": "miss",
            })));
        }

        Ok(NodeOutput::data(serde_json::json!({
            "body": body,
            "_content_type": content_type,
            "_etag": etag,
            "_cache": "miss",
            "_cache_control": format!("public, max-age={}", self.cache_ttl_secs),
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
