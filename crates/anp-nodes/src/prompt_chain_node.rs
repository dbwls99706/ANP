use std::collections::HashMap;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// PromptChain node — chains multiple template prompts with variable substitution.
///
/// Config:
/// ```json
/// {
///   "steps": [
///     { "template": "Hello {{name}}, welcome to {{place}}!", "output_key": "greeting" },
///     { "template": "{{greeting}} How are you?", "output_key": "full" }
///   ],
///   "initial_context": { "name": "Alice" }
/// }
/// ```
pub struct PromptChainNode {
    steps: Vec<ChainStep>,
    initial_context: HashMap<String, String>,
}

struct ChainStep {
    template: String,
    output_key: String,
}

impl Default for PromptChainNode {
    fn default() -> Self {
        Self::new()
    }
}

impl PromptChainNode {
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            initial_context: HashMap::new(),
        }
    }

    fn render_template(template: &str, context: &HashMap<String, String>) -> String {
        let mut result = template.to_string();
        // Replace all {{key}} patterns
        for (key, value) in context {
            let pattern = format!("{{{{{}}}}}", key);
            result = result.replace(&pattern, value);
        }
        result
    }

    fn flatten_json_to_context(
        value: &serde_json::Value,
        prefix: &str,
        context: &mut HashMap<String, String>,
    ) {
        match value {
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    let key = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{}.{}", prefix, k)
                    };
                    // Store the direct key as well for simple access
                    if let Some(s) = v.as_str() {
                        context.insert(k.clone(), s.to_string());
                        if !prefix.is_empty() {
                            context.insert(key.clone(), s.to_string());
                        }
                    } else {
                        let s = v.to_string();
                        context.insert(k.clone(), s.clone());
                        if !prefix.is_empty() {
                            context.insert(key.clone(), s);
                        }
                    }
                    if v.is_object() {
                        Self::flatten_json_to_context(v, &key, context);
                    }
                }
            }
            serde_json::Value::String(s) => {
                if !prefix.is_empty() {
                    context.insert(prefix.to_string(), s.clone());
                }
            }
            other => {
                if !prefix.is_empty() {
                    context.insert(prefix.to_string(), other.to_string());
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for PromptChainNode {
    fn node_type(&self) -> &str {
        "prompt_chain"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        let steps_arr = config
            .get("steps")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                NodeError::InvalidConfig("steps config is required and must be an array".into())
            })?;

        self.steps.clear();
        for (i, step) in steps_arr.iter().enumerate() {
            let template = step
                .get("template")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    NodeError::InvalidConfig(format!("step {} missing 'template' string", i))
                })?
                .to_string();
            let output_key = step
                .get("output_key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    NodeError::InvalidConfig(format!("step {} missing 'output_key' string", i))
                })?
                .to_string();
            self.steps.push(ChainStep {
                template,
                output_key,
            });
        }

        if let Some(ctx_obj) = config.get("initial_context").and_then(|v| v.as_object()) {
            for (k, v) in ctx_obj {
                if let Some(s) = v.as_str() {
                    self.initial_context.insert(k.clone(), s.to_string());
                } else {
                    self.initial_context.insert(k.clone(), v.to_string());
                }
            }
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let mut context: HashMap<String, String> = self.initial_context.clone();

        // Merge input into context
        Self::flatten_json_to_context(&input, "", &mut context);

        let mut results = serde_json::Map::new();
        let mut final_output = String::new();

        for step in &self.steps {
            let rendered = Self::render_template(&step.template, &context);
            context.insert(step.output_key.clone(), rendered.clone());
            results.insert(
                step.output_key.clone(),
                serde_json::Value::String(rendered.clone()),
            );
            final_output = rendered;
        }

        Ok(NodeOutput::data(serde_json::json!({
            "results": results,
            "step_count": self.steps.len(),
            "final_output": final_output,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_chain() {
        let mut node = PromptChainNode::new();
        node.init(&serde_json::json!({
            "steps": [
                {"template": "Hello {{name}}!", "output_key": "greeting"},
                {"template": "{{greeting}} Welcome to {{place}}.", "output_key": "full"}
            ]
        }))
        .await
        .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"name": "Alice", "place": "Wonderland"}), &ctx)
            .await
            .unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["results"]["greeting"], "Hello Alice!");
        assert_eq!(
            data["results"]["full"],
            "Hello Alice! Welcome to Wonderland."
        );
        assert_eq!(data["step_count"], 2);
        assert_eq!(data["final_output"], "Hello Alice! Welcome to Wonderland.");
    }

    #[tokio::test]
    async fn test_initial_context() {
        let mut node = PromptChainNode::new();
        node.init(&serde_json::json!({
            "steps": [
                {"template": "{{role}}: {{message}}", "output_key": "formatted"}
            ],
            "initial_context": {"role": "System"}
        }))
        .await
        .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"message": "Hello world"}), &ctx)
            .await
            .unwrap();
        assert_eq!(
            output.ports["default"]["results"]["formatted"],
            "System: Hello world"
        );
    }

    #[tokio::test]
    async fn test_single_step() {
        let mut node = PromptChainNode::new();
        node.init(&serde_json::json!({
            "steps": [
                {"template": "Result: {{value}}", "output_key": "out"}
            ]
        }))
        .await
        .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"value": "42"}), &ctx)
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["final_output"], "Result: 42");
        assert_eq!(output.ports["default"]["step_count"], 1);
    }

    #[tokio::test]
    async fn test_missing_steps() {
        let mut node = PromptChainNode::new();
        let result = node.init(&serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_missing_template_in_step() {
        let mut node = PromptChainNode::new();
        let result = node
            .init(&serde_json::json!({
                "steps": [{"output_key": "out"}]
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_unresolved_variables_pass_through() {
        let mut node = PromptChainNode::new();
        node.init(&serde_json::json!({
            "steps": [
                {"template": "Hello {{unknown}}!", "output_key": "out"}
            ]
        }))
        .await
        .unwrap();
        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({}), &ctx)
            .await
            .unwrap();
        // Unresolved variables remain as-is
        assert_eq!(output.ports["default"]["final_output"], "Hello {{unknown}}!");
    }

    #[tokio::test]
    async fn test_node_type_and_kind() {
        let node = PromptChainNode::new();
        assert_eq!(node.node_type(), "prompt_chain");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
