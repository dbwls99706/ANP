use std::path::Path;

use crate::graph::spec::AnpGraph;

/// Errors that can occur when loading a graph
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("failed to read file: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to parse JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// Load an ANP graph from a JSON file
pub fn load_from_file(path: &Path) -> Result<AnpGraph, LoadError> {
    let content = std::fs::read_to_string(path)?;
    load_from_str(&content)
}

/// Load an ANP graph from a JSON string
pub fn load_from_str(json: &str) -> Result<AnpGraph, LoadError> {
    let graph: AnpGraph = serde_json::from_str(json)?;
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::spec::GraphVersion;

    #[test]
    fn test_load_from_str() {
        let json = r#"{
            "version": "0.1",
            "metadata": { "name": "test" },
            "nodes": [],
            "edges": []
        }"#;
        let graph = load_from_str(json).unwrap();
        assert_eq!(graph.version, GraphVersion::V0_1);
        assert_eq!(graph.metadata.name, "test");
    }
}
