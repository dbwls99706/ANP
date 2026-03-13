use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Console node — prints data to stdout
pub struct ConsoleNode {
    prefix: String,
}

impl Default for ConsoleNode {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsoleNode {
    pub fn new() -> Self {
        Self {
            prefix: String::new(),
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for ConsoleNode {
    fn node_type(&self) -> &str {
        "console"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Sink
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(prefix) = config.get("prefix").and_then(|v| v.as_str()) {
            self.prefix = prefix.to_string();
        }

        // If config has a static "message", that's the data to print
        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let output_str = if input.is_null() {
            // No input — check if there's a static message in the node
            "".to_string()
        } else if let Some(s) = input.as_str() {
            s.to_string()
        } else {
            serde_json::to_string_pretty(&input)
                .map_err(|e| NodeError::ExecutionFailed(e.to_string()))?
        };

        if self.prefix.is_empty() {
            println!("{}", output_str);
        } else {
            println!("{}{}", self.prefix, output_str);
        }

        Ok(NodeOutput::data(input))
    }
}
