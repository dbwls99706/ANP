use anp_core::graph::editor::GraphOp;
use anp_core::graph::spec::AnpGraph;
use anp_core::graph::validate;
use anp_core::node::registry::NodeRegistry;
use tracing::{debug, info, warn};

use crate::catalog;
use crate::error::{PlannerError, PlannerResult};
use crate::llm::LlmClient;
use crate::parser;
use crate::prompt;

const MAX_FIX_ATTEMPTS: usize = 3;

/// ANP Graph Planner — generates graphs from natural language prompts.
pub struct AnpPlanner {
    client: Box<dyn LlmClient>,
    catalog_text: String,
    system_prompt: String,
}

impl AnpPlanner {
    /// Create a new planner with the given LLM client and node registry.
    pub fn new(client: Box<dyn LlmClient>, registry: &NodeRegistry) -> Self {
        let catalog_text = catalog::build_catalog(registry);
        let system_prompt = prompt::build_system_prompt(&catalog_text);
        Self {
            client,
            catalog_text,
            system_prompt,
        }
    }

    /// Generate an ANP graph from a natural language prompt.
    /// Includes validation and auto-fix loop (up to 3 attempts).
    pub async fn generate(&self, user_prompt: &str) -> PlannerResult<AnpGraph> {
        info!(provider = %self.client.provider(), "generating graph from prompt");
        debug!(prompt = %user_prompt, "user prompt");

        let mut attempt = 0;
        let mut current_user_msg = user_prompt.to_string();

        loop {
            // Call LLM
            let response = self
                .client
                .generate(&self.system_prompt, &current_user_msg)
                .await?;

            debug!(attempt = attempt, "received LLM response");

            // Parse JSON from response
            let json_str = match parser::extract_json(&response) {
                Ok(json) => json,
                Err(e) => {
                    if attempt < MAX_FIX_ATTEMPTS {
                        warn!(attempt = attempt, error = %e, "failed to extract JSON, retrying");
                        current_user_msg = format!(
                            "Your previous response did not contain valid JSON. \
                             Please respond with ONLY a valid ANP graph JSON object.\n\n\
                             Original request: {}",
                            user_prompt
                        );
                        attempt += 1;
                        continue;
                    }
                    return Err(e);
                }
            };

            // Parse into AnpGraph
            let graph = match parser::parse_graph(&json_str) {
                Ok(g) => g,
                Err(e) => {
                    if attempt < MAX_FIX_ATTEMPTS {
                        warn!(attempt = attempt, error = %e, "failed to parse graph, retrying");
                        current_user_msg = prompt::build_fix_prompt(
                            user_prompt,
                            &json_str,
                            &[format!("JSON parse error: {}", e)],
                        );
                        attempt += 1;
                        continue;
                    }
                    return Err(e);
                }
            };

            // Validate
            match validate::validate(&graph) {
                Ok(()) => {
                    info!(
                        name = %graph.metadata.name,
                        nodes = graph.nodes.len(),
                        edges = graph.edges.len(),
                        attempts = attempt + 1,
                        "graph generated successfully"
                    );
                    return Ok(graph);
                }
                Err(errors) => {
                    if attempt < MAX_FIX_ATTEMPTS {
                        let error_strs: Vec<String> =
                            errors.iter().map(|e| e.to_string()).collect();
                        warn!(
                            attempt = attempt,
                            errors = ?error_strs,
                            "graph validation failed, retrying"
                        );
                        current_user_msg =
                            prompt::build_fix_prompt(user_prompt, &json_str, &error_strs);
                        attempt += 1;
                        continue;
                    }
                    return Err(PlannerError::ValidationFailed {
                        attempts: attempt + 1,
                        errors,
                    });
                }
            }
        }
    }

    /// Generate graph operations from a natural language edit instruction.
    /// Returns a list of GraphOps to apply to an existing graph.
    pub async fn generate_edit(
        &self,
        instruction: &str,
        current_graph: &AnpGraph,
    ) -> PlannerResult<Vec<GraphOp>> {
        info!(provider = %self.client.provider(), "generating graph edit operations");
        debug!(instruction = %instruction, "edit instruction");

        let graph_json = serde_json::to_string_pretty(current_graph)
            .map_err(|e| PlannerError::ParseError(e.to_string()))?;

        let edit_system = prompt::build_edit_system_prompt(&self.catalog_text);
        let edit_user = prompt::build_edit_user_prompt(instruction, &graph_json);

        let mut attempt = 0;
        let mut current_user_msg = edit_user;

        loop {
            let response = self
                .client
                .generate(&edit_system, &current_user_msg)
                .await?;

            debug!(attempt = attempt, "received edit LLM response");

            // Extract JSON array from response
            let json_str = match parser::extract_json(&response) {
                Ok(json) => json,
                Err(e) => {
                    if attempt < MAX_FIX_ATTEMPTS {
                        warn!(attempt = attempt, error = %e, "failed to extract JSON ops, retrying");
                        current_user_msg = format!(
                            "Your previous response did not contain valid JSON. \
                             Please respond with ONLY a valid JSON array of graph operations.\n\n\
                             Original instruction: {}",
                            instruction
                        );
                        attempt += 1;
                        continue;
                    }
                    return Err(e);
                }
            };

            // Parse as Vec<GraphOp>
            match serde_json::from_str::<Vec<GraphOp>>(&json_str) {
                Ok(ops) => {
                    info!(ops = ops.len(), attempts = attempt + 1, "edit ops generated successfully");
                    return Ok(ops);
                }
                Err(e) => {
                    if attempt < MAX_FIX_ATTEMPTS {
                        warn!(attempt = attempt, error = %e, "failed to parse ops, retrying");
                        current_user_msg = format!(
                            "Your previous response could not be parsed as graph operations: {}\n\n\
                             Please respond with ONLY a valid JSON array of operations.\n\n\
                             Original instruction: {}",
                            e, instruction
                        );
                        attempt += 1;
                        continue;
                    }
                    return Err(PlannerError::ParseError(format!(
                        "failed to parse graph operations: {}",
                        e
                    )));
                }
            }
        }
    }

    /// Get the node catalog text (useful for debugging).
    pub fn catalog(&self) -> &str {
        &self.catalog_text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::MockLlmClient;

    fn test_registry() -> NodeRegistry {
        let mut registry = NodeRegistry::new();
        anp_nodes::register_all(&mut registry);
        registry
    }

    #[tokio::test]
    async fn test_generate_with_mock() {
        let mock_response = r#"{
            "version": "0.1",
            "metadata": { "name": "Hello World", "description": "Read and print" },
            "nodes": [
                {
                    "id": "source",
                    "node_type": "file_read",
                    "kind": "source",
                    "config": { "path": "data.txt" },
                    "inputs": [],
                    "outputs": [{ "name": "default" }]
                },
                {
                    "id": "output",
                    "node_type": "console",
                    "kind": "sink",
                    "config": {},
                    "inputs": [{ "name": "default" }],
                    "outputs": []
                }
            ],
            "edges": [
                { "id": "e1", "from_node": "source", "to_node": "output" }
            ]
        }"#;

        let client = MockLlmClient::new(mock_response);
        let registry = test_registry();
        let planner = AnpPlanner::new(Box::new(client), &registry);

        let graph = planner.generate("hello world").await.unwrap();
        assert_eq!(graph.metadata.name, "Hello World");
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.edges.len(), 1);
    }

    #[tokio::test]
    async fn test_generate_with_markdown_wrapped_response() {
        let mock_response = r#"Here is the graph:

```json
{
    "version": "0.1",
    "metadata": { "name": "Test", "description": "" },
    "nodes": [
        {
            "id": "src",
            "node_type": "file_read",
            "kind": "source",
            "config": { "path": "test.txt" },
            "outputs": [{ "name": "default" }]
        },
        {
            "id": "out",
            "node_type": "console",
            "kind": "sink",
            "inputs": [{ "name": "default" }]
        }
    ],
    "edges": [
        { "id": "e1", "from_node": "src", "to_node": "out" }
    ]
}
```"#;

        let client = MockLlmClient::new(mock_response);
        let registry = test_registry();
        let planner = AnpPlanner::new(Box::new(client), &registry);

        let graph = planner.generate("read and print").await.unwrap();
        assert_eq!(graph.nodes.len(), 2);
    }

    #[tokio::test]
    async fn test_generate_no_json_fails() {
        let client = MockLlmClient::new("I cannot generate graphs.");
        let registry = test_registry();
        let planner = AnpPlanner::new(Box::new(client), &registry);

        let result = planner.generate("test").await;
        assert!(result.is_err());
    }
}
