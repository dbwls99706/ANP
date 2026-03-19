use anp_core::node::registry::NodeRegistry;

/// Build an LLM-friendly text catalog from the node registry.
/// This becomes part of the system prompt so the LLM knows what nodes are available.
pub fn build_catalog(registry: &NodeRegistry) -> String {
    let mut out = String::from("## Available Nodes\n\n");

    let mut types: Vec<&str> = registry.node_types();
    types.sort();

    for node_type in types {
        if let Some(schema) = registry.get_schema(node_type) {
            out.push_str(&format!("### {} ({})\n", schema.node_type, schema.kind));
            out.push_str(&format!("{}\n", schema.description));

            // Inputs
            if !schema.inputs.is_empty() {
                let ports: Vec<&str> = schema.inputs.iter().map(|p| p.name.as_str()).collect();
                out.push_str(&format!("Inputs: [{}]\n", ports.join(", ")));
            } else {
                out.push_str("Inputs: none\n");
            }

            // Outputs
            if !schema.outputs.is_empty() {
                let ports: Vec<&str> = schema.outputs.iter().map(|p| p.name.as_str()).collect();
                out.push_str(&format!("Outputs: [{}]\n", ports.join(", ")));
            } else {
                out.push_str("Outputs: none\n");
            }

            // Config schema
            out.push_str(&format!(
                "Config: {}\n",
                serde_json::to_string_pretty(&schema.config_schema).unwrap_or_default()
            ));

            out.push('\n');
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_catalog() {
        let mut registry = NodeRegistry::new();
        anp_nodes::register_all(&mut registry);
        let catalog = build_catalog(&registry);
        // Core nodes
        assert!(catalog.contains("http_server"));
        assert!(catalog.contains("database"));
        assert!(catalog.contains("console"));
        assert!(catalog.contains("service"));
        // New v0.6 nodes
        assert!(catalog.contains("embedding"));
        assert!(catalog.contains("sentiment"));
        assert!(catalog.contains("chart"));
        assert!(catalog.contains("form"));
        assert!(catalog.contains("batch"));
        assert!(catalog.contains("kv_store"));
        assert!(catalog.contains("circuit_breaker"));
        assert!(catalog.contains("xml"));
    }

    #[test]
    fn test_catalog_has_92_nodes() {
        let mut registry = NodeRegistry::new();
        anp_nodes::register_all(&mut registry);
        let types = registry.node_types();
        assert!(
            types.len() >= 92,
            "Expected at least 92 node types, got {}",
            types.len()
        );
    }
}
