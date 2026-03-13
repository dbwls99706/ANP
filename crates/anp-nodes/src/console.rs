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
        // If input is an array of strings, print each on its own line
        if let serde_json::Value::Array(items) = &input {
            if items.iter().all(|v| v.is_string()) {
                for item in items {
                    let s = item.as_str().unwrap_or("");
                    if self.prefix.is_empty() {
                        println!("{}", s);
                    } else {
                        println!("{}{}", self.prefix, s);
                    }
                }
                return Ok(NodeOutput::data(input));
            }
        }

        let output_str = if input.is_null() {
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
