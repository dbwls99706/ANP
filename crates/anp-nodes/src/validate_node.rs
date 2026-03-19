use std::collections::HashMap;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Validate node — validates data against a set of rules.
///
/// Config:
/// ```json
/// {
///   "rules": {
///     "name": { "type": "string", "required": true, "min_length": 1, "max_length": 255 },
///     "age": { "type": "number", "required": true, "min": 0, "max": 150 },
///     "role": { "type": "string", "enum": ["admin", "user", "guest"] },
///     "email": { "type": "string", "pattern": "^[^@]+@[^@]+$" }
///   }
/// }
/// ```
///
/// Multi-port output:
/// - `"valid"` port: data that passes all validation rules
/// - `"invalid"` port: `{ "errors": [...], "data": original_input }`
pub struct ValidateNode {
    rules: HashMap<String, ValidationRule>,
}

#[derive(Debug, Clone)]
struct ValidationRule {
    expected_type: Option<String>,
    required: bool,
    min: Option<f64>,
    max: Option<f64>,
    min_length: Option<usize>,
    max_length: Option<usize>,
    pattern: Option<String>,
    enum_values: Option<Vec<serde_json::Value>>,
}

impl Default for ValidateNode {
    fn default() -> Self {
        Self::new()
    }
}

impl ValidateNode {
    pub fn new() -> Self {
        Self {
            rules: HashMap::new(),
        }
    }
}

fn parse_rule(config: &serde_json::Value) -> Result<ValidationRule, NodeError> {
    let obj = config.as_object().ok_or_else(|| {
        NodeError::InvalidConfig("each rule must be an object".into())
    })?;

    Ok(ValidationRule {
        expected_type: obj.get("type").and_then(|v| v.as_str()).map(|s| s.to_string()),
        required: obj.get("required").and_then(|v| v.as_bool()).unwrap_or(false),
        min: obj.get("min").and_then(|v| v.as_f64()),
        max: obj.get("max").and_then(|v| v.as_f64()),
        min_length: obj.get("min_length").and_then(|v| v.as_u64()).map(|n| n as usize),
        max_length: obj.get("max_length").and_then(|v| v.as_u64()).map(|n| n as usize),
        pattern: obj.get("pattern").and_then(|v| v.as_str()).map(|s| s.to_string()),
        enum_values: obj.get("enum").and_then(|v| v.as_array()).cloned(),
    })
}

fn check_type(value: &serde_json::Value, expected: &str) -> bool {
    match expected {
        "string" => value.is_string(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        _ => true,
    }
}

fn validate_field(
    field_name: &str,
    value: Option<&serde_json::Value>,
    rule: &ValidationRule,
) -> Vec<String> {
    let mut errors = Vec::new();

    match value {
        None | Some(serde_json::Value::Null) => {
            if rule.required {
                errors.push(format!("'{}' is required", field_name));
            }
            // If absent and not required, skip other checks
            return errors;
        }
        Some(val) => {
            // Type check
            if let Some(ref expected_type) = rule.expected_type {
                if !check_type(val, expected_type) {
                    errors.push(format!(
                        "'{}' expected type '{}' but got '{}'",
                        field_name,
                        expected_type,
                        json_type_name(val)
                    ));
                }
            }

            // Numeric range checks
            if let Some(num) = val.as_f64() {
                if let Some(min) = rule.min {
                    if num < min {
                        errors.push(format!(
                            "'{}' value {} is less than minimum {}",
                            field_name, num, min
                        ));
                    }
                }
                if let Some(max) = rule.max {
                    if num > max {
                        errors.push(format!(
                            "'{}' value {} is greater than maximum {}",
                            field_name, num, max
                        ));
                    }
                }
            }

            // String length checks
            if let Some(s) = val.as_str() {
                if let Some(min_len) = rule.min_length {
                    if s.len() < min_len {
                        errors.push(format!(
                            "'{}' length {} is less than minimum {}",
                            field_name,
                            s.len(),
                            min_len
                        ));
                    }
                }
                if let Some(max_len) = rule.max_length {
                    if s.len() > max_len {
                        errors.push(format!(
                            "'{}' length {} exceeds maximum {}",
                            field_name,
                            s.len(),
                            max_len
                        ));
                    }
                }
            }

            // Array length checks
            if let Some(arr) = val.as_array() {
                if let Some(min_len) = rule.min_length {
                    if arr.len() < min_len {
                        errors.push(format!(
                            "'{}' array length {} is less than minimum {}",
                            field_name,
                            arr.len(),
                            min_len
                        ));
                    }
                }
                if let Some(max_len) = rule.max_length {
                    if arr.len() > max_len {
                        errors.push(format!(
                            "'{}' array length {} exceeds maximum {}",
                            field_name,
                            arr.len(),
                            max_len
                        ));
                    }
                }
            }

            // Pattern check (simple substring or basic regex-like matching)
            if let Some(ref pattern) = rule.pattern {
                if let Some(s) = val.as_str() {
                    if !simple_pattern_match(pattern, s) {
                        errors.push(format!(
                            "'{}' does not match pattern '{}'",
                            field_name, pattern
                        ));
                    }
                }
            }

            // Enum check
            if let Some(ref allowed) = rule.enum_values {
                if !allowed.contains(val) {
                    errors.push(format!(
                        "'{}' value is not one of the allowed values",
                        field_name
                    ));
                }
            }
        }
    }

    errors
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Simple pattern matching. Supports basic patterns:
/// - Exact match if no special characters
/// - `^` anchor at start, `$` anchor at end
/// - `.` matches any character, `*` matches zero or more of preceding (simplified)
/// For full regex, we do a simple character-by-character approach.
/// Actually, let's use a straightforward contains-based approach with anchors.
fn simple_pattern_match(pattern: &str, text: &str) -> bool {
    // Handle anchor-based patterns
    let (must_start, pat) = if let Some(p) = pattern.strip_prefix('^') {
        (true, p)
    } else {
        (false, pattern)
    };
    let (must_end, pat) = if let Some(p) = pat.strip_suffix('$') {
        (true, p)
    } else {
        (false, pat)
    };

    // For patterns with special regex chars beyond anchors, do char-by-char matching
    if pat.contains('[') || pat.contains('+') || pat.contains('*') || pat.contains('?') || pat.contains('.') {
        // Use a simple NFA-style matcher for basic regex
        return basic_regex_match(pat, text, must_start, must_end);
    }

    // Simple substring/exact matching with anchors
    if must_start && must_end {
        text == pat
    } else if must_start {
        text.starts_with(pat)
    } else if must_end {
        text.ends_with(pat)
    } else {
        text.contains(pat)
    }
}

/// Basic regex matching supporting: `.` (any char), `[...]` char classes,
/// `+` (one or more), `*` (zero or more), `?` (zero or one).
/// This is intentionally simplified — not a full regex engine.
fn basic_regex_match(pattern: &str, text: &str, must_start: bool, must_end: bool) -> bool {
    if must_start {
        match_at(pattern.as_bytes(), text.as_bytes(), 0, 0, must_end)
    } else {
        // Try matching at every position
        for start in 0..=text.len() {
            if match_at(pattern.as_bytes(), text.as_bytes(), 0, start, must_end) {
                return true;
            }
        }
        false
    }
}

fn match_at(pat: &[u8], text: &[u8], pi: usize, ti: usize, must_end: bool) -> bool {
    if pi >= pat.len() {
        return if must_end { ti >= text.len() } else { true };
    }

    // Parse current pattern element
    let (matches_char, next_pi) = parse_pattern_element(pat, pi);

    // Check for quantifier
    if next_pi < pat.len() && (pat[next_pi] == b'+' || pat[next_pi] == b'*' || pat[next_pi] == b'?') {
        let quantifier = pat[next_pi];
        let after_quant = next_pi + 1;

        match quantifier {
            b'*' => {
                // Zero or more: try zero first, then consume
                if match_at(pat, text, after_quant, ti, must_end) {
                    return true;
                }
                let mut pos = ti;
                while pos < text.len() && matches_char(text[pos]) {
                    pos += 1;
                    if match_at(pat, text, after_quant, pos, must_end) {
                        return true;
                    }
                }
                false
            }
            b'+' => {
                // One or more
                let mut pos = ti;
                while pos < text.len() && matches_char(text[pos]) {
                    pos += 1;
                    if match_at(pat, text, after_quant, pos, must_end) {
                        return true;
                    }
                }
                false
            }
            b'?' => {
                // Zero or one
                if match_at(pat, text, after_quant, ti, must_end) {
                    return true;
                }
                if ti < text.len() && matches_char(text[ti]) {
                    return match_at(pat, text, after_quant, ti + 1, must_end);
                }
                false
            }
            _ => unreachable!(),
        }
    } else {
        // No quantifier — must match exactly one
        if ti < text.len() && matches_char(text[ti]) {
            match_at(pat, text, next_pi, ti + 1, must_end)
        } else {
            false
        }
    }
}

/// Parse one pattern element at position `pi`, returning a closure that tests
/// if a byte matches, and the index after this element.
fn parse_pattern_element(pat: &[u8], pi: usize) -> (Box<dyn Fn(u8) -> bool>, usize) {
    if pat[pi] == b'.' {
        (Box::new(|_| true), pi + 1)
    } else if pat[pi] == b'[' {
        // Character class
        let mut end = pi + 1;
        let negate = end < pat.len() && pat[end] == b'^';
        if negate {
            end += 1;
        }
        let mut chars = Vec::new();
        while end < pat.len() && pat[end] != b']' {
            if end + 2 < pat.len() && pat[end + 1] == b'-' {
                let from = pat[end];
                let to = pat[end + 2];
                for c in from..=to {
                    chars.push(c);
                }
                end += 3;
            } else {
                chars.push(pat[end]);
                end += 1;
            }
        }
        let next = if end < pat.len() { end + 1 } else { end };
        if negate {
            (Box::new(move |c| !chars.contains(&c)), next)
        } else {
            (Box::new(move |c| chars.contains(&c)), next)
        }
    } else if pat[pi] == b'\\' && pi + 1 < pat.len() {
        let escaped = pat[pi + 1];
        (Box::new(move |c| c == escaped), pi + 2)
    } else {
        let expected = pat[pi];
        (Box::new(move |c| c == expected), pi + 1)
    }
}

#[async_trait::async_trait]
impl NodeExecutor for ValidateNode {
    fn node_type(&self) -> &str {
        "validate"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        let rules_obj = config
            .get("rules")
            .and_then(|v| v.as_object())
            .ok_or_else(|| NodeError::InvalidConfig("missing 'rules' object in config".into()))?;

        for (field_name, rule_config) in rules_obj {
            let rule = parse_rule(rule_config)?;
            self.rules.insert(field_name.clone(), rule);
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let mut all_errors = Vec::new();

        for (field_name, rule) in &self.rules {
            let field_value = input.get(field_name);
            let field_errors = validate_field(field_name, field_value, rule);
            all_errors.extend(field_errors);
        }

        if all_errors.is_empty() {
            // Valid — route to "valid" port
            let mut ports = HashMap::new();
            ports.insert("valid".to_string(), input);
            Ok(NodeOutput::multi(ports))
        } else {
            // Invalid — route to "invalid" port
            let error_output = serde_json::json!({
                "errors": all_errors,
                "data": input
            });
            let mut ports = HashMap::new();
            ports.insert("invalid".to_string(), error_output);
            Ok(NodeOutput::multi(ports))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_valid_data() {
        let mut node = ValidateNode::new();
        node.init(&serde_json::json!({
            "rules": {
                "name": { "type": "string", "required": true, "min_length": 1 },
                "age": { "type": "number", "min": 0, "max": 150 }
            }
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let input = serde_json::json!({"name": "Alice", "age": 30});
        let output = node.execute(input.clone(), &ctx).await.unwrap();
        assert!(output.ports.contains_key("valid"));
        assert!(!output.ports.contains_key("invalid"));
        assert_eq!(output.ports["valid"], input);
    }

    #[tokio::test]
    async fn test_invalid_data_required() {
        let mut node = ValidateNode::new();
        node.init(&serde_json::json!({
            "rules": {
                "name": { "type": "string", "required": true }
            }
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let input = serde_json::json!({"age": 30});
        let output = node.execute(input, &ctx).await.unwrap();
        assert!(output.ports.contains_key("invalid"));
        assert!(!output.ports.contains_key("valid"));
        let errors = output.ports["invalid"]["errors"].as_array().unwrap();
        assert!(!errors.is_empty());
    }

    #[tokio::test]
    async fn test_invalid_data_type_mismatch() {
        let mut node = ValidateNode::new();
        node.init(&serde_json::json!({
            "rules": {
                "age": { "type": "number" }
            }
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let input = serde_json::json!({"age": "not_a_number"});
        let output = node.execute(input, &ctx).await.unwrap();
        assert!(output.ports.contains_key("invalid"));
    }

    #[tokio::test]
    async fn test_invalid_data_range() {
        let mut node = ValidateNode::new();
        node.init(&serde_json::json!({
            "rules": {
                "score": { "type": "number", "min": 0, "max": 100 }
            }
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let input = serde_json::json!({"score": 150});
        let output = node.execute(input, &ctx).await.unwrap();
        assert!(output.ports.contains_key("invalid"));
    }

    #[tokio::test]
    async fn test_enum_validation() {
        let mut node = ValidateNode::new();
        node.init(&serde_json::json!({
            "rules": {
                "role": { "type": "string", "enum": ["admin", "user"] }
            }
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();

        // Valid enum value
        let input = serde_json::json!({"role": "admin"});
        let output = node.execute(input, &ctx).await.unwrap();
        assert!(output.ports.contains_key("valid"));

        // Invalid enum value
        let input = serde_json::json!({"role": "superuser"});
        let output = node.execute(input, &ctx).await.unwrap();
        assert!(output.ports.contains_key("invalid"));
    }

    #[tokio::test]
    async fn test_pattern_validation() {
        let mut node = ValidateNode::new();
        node.init(&serde_json::json!({
            "rules": {
                "email": { "type": "string", "pattern": "^[^@]+@[^@]+$" }
            }
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();

        let input = serde_json::json!({"email": "user@example.com"});
        let output = node.execute(input, &ctx).await.unwrap();
        assert!(output.ports.contains_key("valid"));

        let input = serde_json::json!({"email": "not-an-email"});
        let output = node.execute(input, &ctx).await.unwrap();
        assert!(output.ports.contains_key("invalid"));
    }

    #[tokio::test]
    async fn test_optional_field_absent() {
        let mut node = ValidateNode::new();
        node.init(&serde_json::json!({
            "rules": {
                "nickname": { "type": "string" }
            }
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let input = serde_json::json!({"name": "Alice"});
        let output = node.execute(input, &ctx).await.unwrap();
        // Optional field not present → valid
        assert!(output.ports.contains_key("valid"));
    }

    #[tokio::test]
    async fn test_node_type_and_kind() {
        let node = ValidateNode::new();
        assert_eq!(node.node_type(), "validate");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
