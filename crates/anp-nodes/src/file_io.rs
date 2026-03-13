use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// FileRead node — reads a file and outputs its content
pub struct FileReadNode {
    path: String,
}

impl Default for FileReadNode {
    fn default() -> Self {
        Self::new()
    }
}

impl FileReadNode {
    pub fn new() -> Self {
        Self {
            path: String::new(),
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for FileReadNode {
    fn node_type(&self) -> &str {
        "file_read"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Source
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        self.path = config
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| NodeError::InvalidConfig("missing 'path' in config".into()))?
            .to_string();
        Ok(())
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let content = tokio::fs::read_to_string(&self.path)
            .await
            .map_err(|e| NodeError::ExecutionFailed(format!("failed to read '{}': {}", self.path, e)))?;

        let len = content.len();
        Ok(NodeOutput::data(serde_json::Value::String(content))
            .with_log(format!("read {} bytes from {}", len, self.path)))
    }
}

/// FileWrite node — writes data to a file
pub struct FileWriteNode {
    path: String,
}

impl Default for FileWriteNode {
    fn default() -> Self {
        Self::new()
    }
}

impl FileWriteNode {
    pub fn new() -> Self {
        Self {
            path: String::new(),
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for FileWriteNode {
    fn node_type(&self) -> &str {
        "file_write"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Sink
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        self.path = config
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| NodeError::InvalidConfig("missing 'path' in config".into()))?
            .to_string();
        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let content = match &input {
            serde_json::Value::String(s) => s.clone(),
            other => serde_json::to_string_pretty(other)
                .map_err(|e| NodeError::ExecutionFailed(e.to_string()))?,
        };

        tokio::fs::write(&self.path, &content)
            .await
            .map_err(|e| NodeError::ExecutionFailed(format!("failed to write '{}': {}", self.path, e)))?;

        Ok(NodeOutput::data(serde_json::json!({
            "path": self.path,
            "bytes_written": content.len()
        }))
        .with_log(format!("wrote {} bytes to {}", content.len(), self.path)))
    }
}
