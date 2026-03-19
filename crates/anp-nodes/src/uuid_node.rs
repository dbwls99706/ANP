use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;
use uuid::Uuid;

/// UUID node — generates UUIDs.
///
/// Config:
/// ```json
/// { "version": "v4", "field": "id" }
/// ```
///
/// - If input is a non-null object: adds UUID at `field` key
/// - If input is null: produces `{ "<field>": "<uuid>" }`
pub struct UuidNode {
    version: String,
    field: String,
}

impl Default for UuidNode {
    fn default() -> Self {
        Self::new()
    }
}

impl UuidNode {
    pub fn new() -> Self {
        Self {
            version: "v4".to_string(),
            field: "id".to_string(),
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for UuidNode {
    fn node_type(&self) -> &str {
        "uuid"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(v) = config.get("version").and_then(|v| v.as_str()) {
            self.version = v.to_string();
        }
        if let Some(f) = config.get("field").and_then(|v| v.as_str()) {
            self.field = f.to_string();
        }
        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let uuid_str = Uuid::new_v4().to_string();

        if input.is_null() {
            // Source mode: generate a fresh object
            let result = serde_json::json!({ &self.field: uuid_str });
            Ok(NodeOutput::data(result))
        } else if let serde_json::Value::Object(mut map) = input {
            // Processor mode: add UUID to existing object
            map.insert(self.field.clone(), serde_json::Value::String(uuid_str));
            Ok(NodeOutput::data(serde_json::Value::Object(map)))
        } else {
            // For non-object inputs, wrap with UUID
            let result = serde_json::json!({
                &self.field: uuid_str,
                "value": input
            });
            Ok(NodeOutput::data(result))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_uuid_source_mode() {
        let mut node = UuidNode::new();
        node.init(&serde_json::json!({})).await.unwrap();
        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        let id = output.ports["default"]["id"].as_str().unwrap();
        assert_eq!(id.len(), 36); // UUID v4 format
    }

    #[tokio::test]
    async fn test_uuid_processor_mode() {
        let mut node = UuidNode::new();
        node.init(&serde_json::json!({"field": "request_id"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let input = serde_json::json!({"name": "Alice"});
        let output = node.execute(input, &ctx).await.unwrap();
        assert!(output.ports["default"]["request_id"].is_string());
        assert_eq!(output.ports["default"]["name"], "Alice");
    }

    #[tokio::test]
    async fn test_uuid_custom_field() {
        let mut node = UuidNode::new();
        node.init(&serde_json::json!({"field": "trace_id"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        assert!(output.ports["default"]["trace_id"].is_string());
    }

    #[tokio::test]
    async fn test_node_type_and_kind() {
        let node = UuidNode::new();
        assert_eq!(node.node_type(), "uuid");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
