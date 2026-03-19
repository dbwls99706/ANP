use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// RegexNode — pattern matching, extraction, and replacement on strings
pub struct RegexNode {
    pattern: String,
    operation: String,
    replacement: String,
}

impl Default for RegexNode {
    fn default() -> Self {
        Self::new()
    }
}

impl RegexNode {
    pub fn new() -> Self {
        Self {
            pattern: String::new(),
            operation: "match".to_string(),
            replacement: String::new(),
        }
    }
}

fn extract_text(input: &serde_json::Value) -> Option<String> {
    if let Some(s) = input.as_str() {
        Some(s.to_string())
    } else {
        input
            .get("text")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }
}

mod simple_regex {
    pub struct Pattern {
        raw: String,
    }

    impl Pattern {
        pub fn new(pattern: &str) -> Self {
            Self {
                raw: pattern.to_string(),
            }
        }

        pub fn find_all(&self, text: &str) -> Vec<(usize, usize)> {
            let mut results = Vec::new();
            let pat = &self.raw;

            if !has_special_chars(pat) {
                let mut start = 0;
                while let Some(pos) = text[start..].find(pat) {
                    let abs_pos = start + pos;
                    results.push((abs_pos, abs_pos + pat.len()));
                    start = abs_pos + pat.len();
                }
                return results;
            }

            let pat_chars: Vec<char> = pat.chars().collect();
            let text_chars: Vec<char> = text.chars().collect();
            let mut start = 0;
            while start <= text_chars.len() {
                if let Some(end) = try_match(&pat_chars, 0, &text_chars, start) {
                    let byte_start: usize = text_chars[..start].iter().collect::<String>().len();
                    let byte_end: usize = text_chars[..end].iter().collect::<String>().len();
                    if end > start {
                        results.push((byte_start, byte_end));
                        start = end;
                    } else {
                        start += 1;
                    }
                } else {
                    start += 1;
                }
            }

            results
        }

        pub fn _is_match(&self, text: &str) -> bool {
            !self.find_all(text).is_empty()
        }

        pub fn find_all_strings<'a>(&self, text: &'a str) -> Vec<&'a str> {
            self.find_all(text)
                .into_iter()
                .map(|(s, e)| &text[s..e])
                .collect()
        }

        pub fn replace_all(&self, text: &str, replacement: &str) -> String {
            let matches = self.find_all(text);
            if matches.is_empty() {
                return text.to_string();
            }
            let mut result = String::new();
            let mut last_end = 0;
            for (start, end) in matches {
                result.push_str(&text[last_end..start]);
                result.push_str(replacement);
                last_end = end;
            }
            result.push_str(&text[last_end..]);
            result
        }

        pub fn split<'a>(&self, text: &'a str) -> Vec<&'a str> {
            let matches = self.find_all(text);
            if matches.is_empty() {
                return vec![text];
            }
            let mut parts = Vec::new();
            let mut last_end = 0;
            for (start, end) in matches {
                parts.push(&text[last_end..start]);
                last_end = end;
            }
            parts.push(&text[last_end..]);
            parts
        }
    }

    fn has_special_chars(pattern: &str) -> bool {
        let special = ['.', '*', '+', '?', '[', ']', '\\', '^', '$'];
        pattern.chars().any(|c| special.contains(&c))
    }

    fn try_match(pat: &[char], pi: usize, text: &[char], ti: usize) -> Option<usize> {
        match_from(pat, pi, text, ti)
    }

    fn match_from(pat: &[char], mut pi: usize, text: &[char], mut ti: usize) -> Option<usize> {
        while pi < pat.len() {
            if pat[pi] == '^' {
                if ti != 0 {
                    return None;
                }
                pi += 1;
                continue;
            }
            if pat[pi] == '$' {
                if ti == text.len() {
                    pi += 1;
                    continue;
                }
                return None;
            }

            let (elem_len, matcher) = get_element(pat, pi);

            if pi + elem_len < pat.len() && pat[pi + elem_len] == '*' {
                let mut count = 0;
                while ti + count < text.len() && (matcher)(text[ti + count]) {
                    count += 1;
                }
                loop {
                    if let Some(end) = match_from(pat, pi + elem_len + 1, text, ti + count) {
                        return Some(end);
                    }
                    if count == 0 {
                        break;
                    }
                    count -= 1;
                }
                return None;
            }

            if pi + elem_len < pat.len() && pat[pi + elem_len] == '+' {
                if ti >= text.len() || !(matcher)(text[ti]) {
                    return None;
                }
                let mut count = 1;
                while ti + count < text.len() && (matcher)(text[ti + count]) {
                    count += 1;
                }
                while count >= 1 {
                    if let Some(end) = match_from(pat, pi + elem_len + 1, text, ti + count) {
                        return Some(end);
                    }
                    count -= 1;
                }
                return None;
            }

            if pi + elem_len < pat.len() && pat[pi + elem_len] == '?' {
                if ti < text.len() && (matcher)(text[ti]) {
                    if let Some(end) = match_from(pat, pi + elem_len + 1, text, ti + 1) {
                        return Some(end);
                    }
                }
                return match_from(pat, pi + elem_len + 1, text, ti);
            }

            if ti >= text.len() {
                return None;
            }
            if !(matcher)(text[ti]) {
                return None;
            }
            pi += elem_len;
            ti += 1;
        }

        Some(ti)
    }

    fn get_element(pat: &[char], pi: usize) -> (usize, Box<dyn Fn(char) -> bool>) {
        if pat[pi] == '\\' && pi + 1 < pat.len() {
            let next = pat[pi + 1];
            match next {
                'd' => (2, Box::new(|c: char| c.is_ascii_digit())),
                'w' => (2, Box::new(|c: char| c.is_alphanumeric() || c == '_')),
                's' => (2, Box::new(|c: char| c.is_whitespace())),
                'D' => (2, Box::new(|c: char| !c.is_ascii_digit())),
                'W' => (2, Box::new(|c: char| !c.is_alphanumeric() && c != '_')),
                'S' => (2, Box::new(|c: char| !c.is_whitespace())),
                _ => {
                    let ch = next;
                    (2, Box::new(move |c: char| c == ch))
                }
            }
        } else if pat[pi] == '[' {
            let mut end = pi + 1;
            let negated = end < pat.len() && pat[end] == '^';
            if negated {
                end += 1;
            }
            while end < pat.len() && pat[end] != ']' {
                end += 1;
            }
            let class_start = pi + 1 + if negated { 1 } else { 0 };
            let class_chars: Vec<char> = pat[class_start..end].to_vec();
            let len = end - pi + 1;

            let mut char_set: Vec<char> = Vec::new();
            let mut i = 0;
            while i < class_chars.len() {
                if i + 2 < class_chars.len() && class_chars[i + 1] == '-' {
                    let from = class_chars[i];
                    let to = class_chars[i + 2];
                    for c in from..=to {
                        char_set.push(c);
                    }
                    i += 3;
                } else {
                    char_set.push(class_chars[i]);
                    i += 1;
                }
            }

            (
                len,
                Box::new(move |c: char| {
                    let found = char_set.contains(&c);
                    if negated {
                        !found
                    } else {
                        found
                    }
                }),
            )
        } else if pat[pi] == '.' {
            (1, Box::new(|c: char| c != '\n'))
        } else {
            let ch = pat[pi];
            (1, Box::new(move |c: char| c == ch))
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for RegexNode {
    fn node_type(&self) -> &str {
        "regex"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        self.pattern = config
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| NodeError::InvalidConfig("missing 'pattern' in config".into()))?
            .to_string();

        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "match" | "extract" | "replace" | "split" => {
                    self.operation = op.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown operation '{}'. Use: match, extract, replace, split",
                        other
                    )));
                }
            }
        }

        if let Some(r) = config.get("replacement").and_then(|v| v.as_str()) {
            self.replacement = r.to_string();
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let text = extract_text(&input).ok_or_else(|| {
            NodeError::ExecutionFailed("input must be a string or { \"text\": \"...\" }".into())
        })?;

        let pat = simple_regex::Pattern::new(&self.pattern);

        let result = match self.operation.as_str() {
            "match" => {
                let matches = pat.find_all_strings(&text);
                let matched = !matches.is_empty();
                let captures: Vec<serde_json::Value> = matches
                    .into_iter()
                    .map(|m| serde_json::Value::String(m.to_string()))
                    .collect();
                serde_json::json!({
                    "matched": matched,
                    "captures": captures
                })
            }
            "extract" => {
                let matches: Vec<serde_json::Value> = pat
                    .find_all_strings(&text)
                    .into_iter()
                    .map(|m| serde_json::Value::String(m.to_string()))
                    .collect();
                serde_json::Value::Array(matches)
            }
            "replace" => {
                let replaced = pat.replace_all(&text, &self.replacement);
                serde_json::Value::String(replaced)
            }
            "split" => {
                let parts: Vec<serde_json::Value> = pat
                    .split(&text)
                    .into_iter()
                    .map(|s| serde_json::Value::String(s.to_string()))
                    .collect();
                serde_json::Value::Array(parts)
            }
            _ => {
                return Err(NodeError::ExecutionFailed(format!(
                    "unknown operation: {}",
                    self.operation
                )));
            }
        };

        Ok(NodeOutput::data(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anp_core::runtime::context::ExecutionContext;
    use serde_json::json;

    async fn make_node(config: serde_json::Value) -> RegexNode {
        let mut node = RegexNode::new();
        node.init(&config).await.unwrap();
        node
    }

    #[tokio::test]
    async fn test_match_literal_found() {
        let node = make_node(json!({"pattern": "hello", "operation": "match"})).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!("say hello world"), &ctx).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["matched"], true);
        assert_eq!(data["captures"][0], "hello");
    }

    #[tokio::test]
    async fn test_match_not_found() {
        let node = make_node(json!({"pattern": "xyz", "operation": "match"})).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!("hello world"), &ctx).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["matched"], false);
        assert_eq!(data["captures"], json!([]));
    }

    #[tokio::test]
    async fn test_extract_digits() {
        let node = make_node(json!({"pattern": "\\d+", "operation": "extract"})).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!("abc 123 def 456"), &ctx).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data, &json!(["123", "456"]));
    }

    #[tokio::test]
    async fn test_replace_literal() {
        let node =
            make_node(json!({"pattern": "world", "operation": "replace", "replacement": "rust"}))
                .await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!("hello world"), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], "hello rust");
    }

    #[tokio::test]
    async fn test_replace_pattern() {
        let node =
            make_node(json!({"pattern": "\\d+", "operation": "replace", "replacement": "N"})).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!("a1b2c3"), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], "aNbNcN");
    }

    #[tokio::test]
    async fn test_split_literal() {
        let node = make_node(json!({"pattern": ",", "operation": "split"})).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!("a,b,c"), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], json!(["a", "b", "c"]));
    }

    #[tokio::test]
    async fn test_split_whitespace() {
        let node = make_node(json!({"pattern": "\\s+", "operation": "split"})).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!("one  two   three"), &ctx).await.unwrap();
        assert_eq!(output.ports["default"], json!(["one", "two", "three"]));
    }

    #[tokio::test]
    async fn test_match_with_text_object() {
        let node = make_node(json!({"pattern": "foo", "operation": "match"})).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!({"text": "foobar"}), &ctx).await.unwrap();
        assert_eq!(output.ports["default"]["matched"], true);
    }

    #[tokio::test]
    async fn test_extract_dot_wildcard() {
        let node = make_node(json!({"pattern": "h.llo", "operation": "extract"})).await;
        let ctx = ExecutionContext::new();
        let output = node.execute(json!("hello hallo"), &ctx).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data.as_array().unwrap().len(), 2);
        assert_eq!(data[0], "hello");
        assert_eq!(data[1], "hallo");
    }
}
