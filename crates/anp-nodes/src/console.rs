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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_init_no_prefix() {
        let mut node = ConsoleNode::new();
        node.init(&serde_json::json!({})).await.unwrap();
        assert_eq!(node.prefix, "");
    }

    #[tokio::test]
    async fn test_init_with_prefix() {
        let mut node = ConsoleNode::new();
        node.init(&serde_json::json!({"prefix": "[LOG] "}))
            .await
            .unwrap();
        assert_eq!(node.prefix, "[LOG] ");
    }

    #[tokio::test]
    async fn test_execute_string() {
        let mut node = ConsoleNode::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!("hello"), &ctx)
            .await
            .unwrap();
        // Console passes through input
        assert_eq!(output.ports["default"], "hello");
    }

    #[tokio::test]
    async fn test_execute_json_object() {
        let mut node = ConsoleNode::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let input = serde_json::json!({"key": "value"});
        let output = node.execute(input.clone(), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], input);
    }

    #[tokio::test]
    async fn test_execute_null() {
        let mut node = ConsoleNode::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::Value::Null, &ctx)
            .await
            .unwrap();
        assert!(output.ports["default"].is_null());
    }

    #[tokio::test]
    async fn test_execute_array_of_strings() {
        let mut node = ConsoleNode::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let input = serde_json::json!(["line1", "line2", "line3"]);
        let output = node.execute(input.clone(), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], input);
    }

    #[tokio::test]
    async fn test_node_type_and_kind() {
        let node = ConsoleNode::new();
        assert_eq!(node.node_type(), "console");
        assert_eq!(node.node_kind(), NodeKind::Sink);
    }
}
