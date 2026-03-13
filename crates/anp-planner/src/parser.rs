use crate::error::{PlannerError, PlannerResult};
use anp_core::graph::spec::AnpGraph;

/// Extract JSON from an LLM response that may contain markdown fences or surrounding text.
pub fn extract_json(text: &str) -> PlannerResult<String> {
    let trimmed = text.trim();

    // Case 1: Response is already pure JSON
    if trimmed.starts_with('{') {
        // Find the matching closing brace
        if let Some(json) = extract_balanced_braces(trimmed) {
            return Ok(json);
        }
    }

    // Case 2: JSON inside ```json ... ``` or ``` ... ```
    if let Some(start) = trimmed.find("```json") {
        let json_start = start + 7; // skip "```json"
        if let Some(end) = trimmed[json_start..].find("```") {
            let json_str = trimmed[json_start..json_start + end].trim();
            return Ok(json_str.to_string());
        }
    }

    if let Some(start) = trimmed.find("```") {
        let json_start = start + 3;
        if let Some(end) = trimmed[json_start..].find("```") {
            let json_str = trimmed[json_start..json_start + end].trim();
            if json_str.starts_with('{') {
                return Ok(json_str.to_string());
            }
        }
    }

    // Case 3: Find first { and extract balanced braces
    if let Some(pos) = trimmed.find('{') {
        if let Some(json) = extract_balanced_braces(&trimmed[pos..]) {
            return Ok(json);
        }
    }

    Err(PlannerError::NoJsonInResponse)
}

/// Extract a balanced JSON object starting from the first {
fn extract_balanced_braces(text: &str) -> Option<String> {
    let mut depth = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for (i, ch) in text.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }

        match ch {
            '\\' if in_string => escape_next = true,
            '"' => in_string = !in_string,
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[..=i].to_string());
                }
            }
            _ => {}
        }
    }

    None
}

/// Parse extracted JSON string into an AnpGraph.
pub fn parse_graph(json: &str) -> PlannerResult<AnpGraph> {
    serde_json::from_str(json).map_err(|e| PlannerError::ParseError(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_pure_json() {
        let input = r#"{"version": "0.1", "metadata": {"name": "test"}, "nodes": [], "edges": []}"#;
        let result = extract_json(input).unwrap();
        assert!(result.starts_with('{'));
        assert!(result.contains("version"));
    }

    #[test]
    fn test_extract_from_markdown() {
        let input = r#"Here is the graph:

```json
{"version": "0.1", "metadata": {"name": "test"}, "nodes": [], "edges": []}
```

Hope this helps!"#;
        let result = extract_json(input).unwrap();
        assert!(result.contains("version"));
    }

    #[test]
    fn test_extract_with_surrounding_text() {
        let input = r#"Sure! Here's the graph: {"version": "0.1", "metadata": {"name": "test"}, "nodes": [], "edges": []} Let me know if you need changes."#;
        let result = extract_json(input).unwrap();
        assert!(result.contains("version"));
    }

    #[test]
    fn test_no_json() {
        let input = "This response has no JSON at all.";
        assert!(extract_json(input).is_err());
    }

    #[test]
    fn test_parse_graph() {
        let json = r#"{"version": "0.1", "metadata": {"name": "test", "description": ""}, "nodes": [], "edges": []}"#;
        let graph = parse_graph(json).unwrap();
        assert_eq!(graph.metadata.name, "test");
    }
}
