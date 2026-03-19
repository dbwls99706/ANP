use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Template Engine node — advanced template engine with loops and conditionals.
///
/// Extends basic template rendering with control flow constructs:
/// - `{{variable}}` — variable substitution with dot-path resolution
/// - `{{#each items}}...{{/each}}` — loop over arrays
/// - `{{#if condition}}...{{else}}...{{/if}}` — conditionals
/// - `{{@index}}` — current loop index (0-based)
/// - `{{@first}}` / `{{@last}}` — boolean flags for first/last iteration
///
/// Config example:
/// ```json
/// {
///   "template": "<ul>{{#each items}}<li>{{name}}</li>{{/each}}</ul>"
/// }
/// ```
///
/// Input: JSON data for template context.
/// Output: `{ "rendered": "...", "_content_type": "text/html" }`
pub struct TemplateEngineNode {
    template: String,
}

impl Default for TemplateEngineNode {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateEngineNode {
    pub fn new() -> Self {
        Self {
            template: String::new(),
        }
    }
}

/// Resolve a dot-path like "user.name" on a JSON value.
fn resolve_path(value: &serde_json::Value, path: &str) -> serde_json::Value {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = value;

    for part in parts {
        match current {
            serde_json::Value::Object(map) => {
                if let Some(next) = map.get(part) {
                    current = next;
                } else {
                    return serde_json::Value::Null;
                }
            }
            serde_json::Value::Array(arr) => {
                if let Ok(idx) = part.parse::<usize>() {
                    if let Some(next) = arr.get(idx) {
                        current = next;
                    } else {
                        return serde_json::Value::Null;
                    }
                } else {
                    return serde_json::Value::Null;
                }
            }
            _ => return serde_json::Value::Null,
        }
    }

    current.clone()
}

/// Convert a JSON value to its string representation for template output.
fn value_to_string(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// Determine if a JSON value is "truthy" for conditional evaluation.
fn is_truthy(val: &serde_json::Value) -> bool {
    match val {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        serde_json::Value::String(s) => !s.is_empty(),
        serde_json::Value::Array(a) => !a.is_empty(),
        serde_json::Value::Object(_) => true,
    }
}

/// Find the position of a matching closing tag, handling nested blocks of the same type.
/// Returns the position relative to the start of `text`.
fn find_closing_tag(text: &str, open_prefix: &str, close_tag: &str) -> Option<usize> {
    let mut depth = 1;
    let mut pos = 0;

    while pos < text.len() {
        if text[pos..].starts_with(close_tag) {
            depth -= 1;
            if depth == 0 {
                return Some(pos);
            }
            pos += close_tag.len();
        } else if text[pos..].starts_with(open_prefix) {
            depth += 1;
            pos += open_prefix.len();
        } else {
            pos += 1;
        }
    }

    None
}

/// Find the top-level {{else}} in a block, returning (if_body, else_body).
/// Handles nested {{#if}} blocks correctly.
fn find_else_split(block: &str) -> (&str, &str) {
    let mut depth = 0;
    let mut pos = 0;

    while pos < block.len() {
        if block[pos..].starts_with("{{#if ") {
            depth += 1;
            pos += 6;
        } else if block[pos..].starts_with("{{/if}}") {
            depth -= 1;
            pos += 7;
        } else if depth == 0 && block[pos..].starts_with("{{else}}") {
            return (&block[..pos], &block[pos + 8..]);
        } else {
            pos += 1;
        }
    }

    (block, "")
}

/// Recursively render a template string against a JSON context.
fn render_template(template: &str, context: &serde_json::Value) -> String {
    let mut result = String::with_capacity(template.len());
    let chars: Vec<char> = template.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Check for {{#each ...}}
        if template[i..].starts_with("{{#each ") {
            if let Some(end_tag) = template[i..].find("}}") {
                let tag_content = &template[i + 8..i + end_tag];
                let var_name = tag_content.trim();

                let after_tag = i + end_tag + 2;
                // Find matching {{/each}} handling nesting
                if let Some(close_pos) =
                    find_closing_tag(&template[after_tag..], "{{#each ", "{{/each}}")
                {
                    let body = &template[after_tag..after_tag + close_pos];
                    let arr = resolve_path(context, var_name);

                    if let serde_json::Value::Array(items) = &arr {
                        let item_count = items.len();
                        for (idx, item) in items.iter().enumerate() {
                            // Create merged context: parent context + item fields + loop vars
                            let mut item_ctx = if let serde_json::Value::Object(parent) = context {
                                let mut merged = parent.clone();
                                // Overlay item fields if item is an object
                                if let serde_json::Value::Object(item_map) = item {
                                    for (k, v) in item_map {
                                        merged.insert(k.clone(), v.clone());
                                    }
                                }
                                // Store current item as "this"
                                merged.insert("this".to_string(), item.clone());
                                serde_json::Value::Object(merged)
                            } else {
                                let mut map = serde_json::Map::new();
                                map.insert("this".to_string(), item.clone());
                                serde_json::Value::Object(map)
                            };

                            // Add loop variables
                            if let serde_json::Value::Object(map) = &mut item_ctx {
                                map.insert("@index".to_string(), serde_json::json!(idx));
                                map.insert("@first".to_string(), serde_json::json!(idx == 0));
                                map.insert(
                                    "@last".to_string(),
                                    serde_json::json!(idx == item_count - 1),
                                );
                            }

                            result.push_str(&render_template(body, &item_ctx));
                        }
                    }

                    i = after_tag + close_pos + 9; // skip past {{/each}}
                    continue;
                }
            }
        }

        // Check for {{#if ...}}
        if template[i..].starts_with("{{#if ") {
            if let Some(end_tag) = template[i..].find("}}") {
                let condition = template[i + 6..i + end_tag].trim().to_string();
                let after_tag = i + end_tag + 2;

                // Find matching {{/if}} handling nesting
                if let Some(close_pos) =
                    find_closing_tag(&template[after_tag..], "{{#if ", "{{/if}}")
                {
                    let block = &template[after_tag..after_tag + close_pos];

                    // Split by {{else}} at the top level only
                    let (if_body, else_body) = find_else_split(block);

                    let val = resolve_path(context, &condition);
                    if is_truthy(&val) {
                        result.push_str(&render_template(if_body, context));
                    } else {
                        result.push_str(&render_template(else_body, context));
                    }

                    i = after_tag + close_pos + 7; // skip past {{/if}}
                    continue;
                }
            }
        }

        // Check for {{variable}} — skip block tags
        if i + 2 < len
            && chars[i] == '{'
            && chars[i + 1] == '{'
            && !template[i..].starts_with("{{#")
            && !template[i..].starts_with("{{/")
            && !template[i..].starts_with("{{else}}")
        {
            if let Some(end) = template[i + 2..].find("}}") {
                let var_name = template[i + 2..i + 2 + end].trim();
                if !var_name.is_empty() {
                    let val = resolve_path(context, var_name);
                    result.push_str(&value_to_string(&val));
                    i = i + 2 + end + 2;
                    continue;
                }
            }
        }

        result.push(chars[i]);
        i += 1;
    }

    result
}

#[async_trait::async_trait]
impl NodeExecutor for TemplateEngineNode {
    fn node_type(&self) -> &str {
        "template_engine"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        self.template = config
            .get("template")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                NodeError::InvalidConfig("'template' string is required in config".into())
            })?
            .to_string();

        if self.template.is_empty() {
            return Err(NodeError::InvalidConfig(
                "template must not be empty".into(),
            ));
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let rendered = render_template(&self.template, &input);

        Ok(NodeOutput::data(serde_json::json!({
            "rendered": rendered,
            "_content_type": "text/html"
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_variable() {
        let data = serde_json::json!({"name": "Alice"});
        let result = render_template("Hello, {{name}}!", &data);
        assert_eq!(result, "Hello, Alice!");
    }

    #[test]
    fn test_dot_path() {
        let data = serde_json::json!({"user": {"name": "Bob", "age": 30}});
        let result = render_template("{{user.name}} is {{user.age}}", &data);
        assert_eq!(result, "Bob is 30");
    }

    #[test]
    fn test_missing_variable() {
        let data = serde_json::json!({"name": "Alice"});
        let result = render_template("Hello, {{missing}}!", &data);
        assert_eq!(result, "Hello, !");
    }

    #[test]
    fn test_each_loop() {
        let data = serde_json::json!({
            "items": [
                {"name": "Apple"},
                {"name": "Banana"},
                {"name": "Cherry"}
            ]
        });
        let result = render_template("{{#each items}}{{name}},{{/each}}", &data);
        assert_eq!(result, "Apple,Banana,Cherry,");
    }

    #[test]
    fn test_each_with_index() {
        let data = serde_json::json!({
            "items": [{"name": "A"}, {"name": "B"}, {"name": "C"}]
        });
        let result = render_template("{{#each items}}{{@index}}:{{name}} {{/each}}", &data);
        assert_eq!(result, "0:A 1:B 2:C ");
    }

    #[test]
    fn test_each_first_last() {
        let data = serde_json::json!({
            "items": [{"v": "A"}, {"v": "B"}, {"v": "C"}]
        });
        let result = render_template("{{#each items}}{{@first}}-{{@last}},{{/each}}", &data);
        assert_eq!(result, "true-false,false-false,false-true,");
    }

    #[test]
    fn test_if_true() {
        let data = serde_json::json!({"show": true, "message": "visible"});
        let result = render_template("{{#if show}}{{message}}{{/if}}", &data);
        assert_eq!(result, "visible");
    }

    #[test]
    fn test_if_false() {
        let data = serde_json::json!({"show": false});
        let result = render_template("{{#if show}}visible{{/if}}", &data);
        assert_eq!(result, "");
    }

    #[test]
    fn test_if_else() {
        let data = serde_json::json!({"logged_in": false});
        let result = render_template(
            "{{#if logged_in}}Welcome{{else}}Please log in{{/if}}",
            &data,
        );
        assert_eq!(result, "Please log in");
    }

    #[test]
    fn test_if_else_true_branch() {
        let data = serde_json::json!({"logged_in": true});
        let result = render_template(
            "{{#if logged_in}}Welcome{{else}}Please log in{{/if}}",
            &data,
        );
        assert_eq!(result, "Welcome");
    }

    #[test]
    fn test_nested_if() {
        let data = serde_json::json!({"a": true, "b": true, "msg": "deep"});
        let result = render_template("{{#if a}}{{#if b}}{{msg}}{{/if}}{{/if}}", &data);
        assert_eq!(result, "deep");
    }

    #[test]
    fn test_truthy_values() {
        assert!(is_truthy(&serde_json::json!(true)));
        assert!(is_truthy(&serde_json::json!(1)));
        assert!(is_truthy(&serde_json::json!("hello")));
        assert!(is_truthy(&serde_json::json!([1])));
        assert!(is_truthy(&serde_json::json!({"a": 1})));

        assert!(!is_truthy(&serde_json::json!(false)));
        assert!(!is_truthy(&serde_json::json!(0)));
        assert!(!is_truthy(&serde_json::json!("")));
        assert!(!is_truthy(&serde_json::json!(null)));
        assert!(!is_truthy(&serde_json::json!([])));
    }

    #[test]
    fn test_each_with_if() {
        let data = serde_json::json!({
            "items": [
                {"name": "Alice", "active": true},
                {"name": "Bob", "active": false},
                {"name": "Carol", "active": true}
            ]
        });
        let result = render_template(
            "{{#each items}}{{#if active}}{{name}} {{/if}}{{/each}}",
            &data,
        );
        assert_eq!(result, "Alice Carol ");
    }

    #[test]
    fn test_no_template_tags() {
        let data = serde_json::json!({});
        let result = render_template("Just plain text", &data);
        assert_eq!(result, "Just plain text");
    }

    #[test]
    fn test_empty_array_each() {
        let data = serde_json::json!({"items": []});
        let result = render_template("before{{#each items}}item{{/each}}after", &data);
        assert_eq!(result, "beforeafter");
    }

    #[test]
    fn test_nested_each() {
        let data = serde_json::json!({
            "groups": [
                {"items": [{"v": "a"}, {"v": "b"}]},
                {"items": [{"v": "c"}]}
            ]
        });
        let result = render_template(
            "{{#each groups}}[{{#each items}}{{v}}{{/each}}]{{/each}}",
            &data,
        );
        assert_eq!(result, "[ab][c]");
    }

    #[test]
    fn test_if_with_nested_else() {
        let data = serde_json::json!({"a": false, "b": true, "msg": "inner"});
        let result = render_template(
            "{{#if a}}yes{{else}}{{#if b}}{{msg}}{{else}}no{{/if}}{{/if}}",
            &data,
        );
        assert_eq!(result, "inner");
    }

    #[tokio::test]
    async fn test_execute_output_format() {
        let mut node = TemplateEngineNode::new();
        node.init(&serde_json::json!({
            "template": "<h1>{{title}}</h1>"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(serde_json::json!({"title": "Test"}), &ctx)
            .await
            .unwrap();

        let result = &output.ports["default"];
        assert_eq!(result["rendered"], "<h1>Test</h1>");
        assert_eq!(result["_content_type"], "text/html");
    }

    #[tokio::test]
    async fn test_init_missing_template() {
        let mut node = TemplateEngineNode::new();
        assert!(node.init(&serde_json::json!({})).await.is_err());
    }

    #[tokio::test]
    async fn test_init_empty_template() {
        let mut node = TemplateEngineNode::new();
        assert!(node
            .init(&serde_json::json!({"template": ""}))
            .await
            .is_err());
    }

    #[test]
    fn test_html_list_template() {
        let data = serde_json::json!({
            "title": "Fruits",
            "items": [
                {"name": "Apple", "color": "red"},
                {"name": "Banana", "color": "yellow"}
            ]
        });
        let template =
            "<h1>{{title}}</h1><ul>{{#each items}}<li>{{name}} ({{color}})</li>{{/each}}</ul>";
        let result = render_template(template, &data);
        assert_eq!(
            result,
            "<h1>Fruits</h1><ul><li>Apple (red)</li><li>Banana (yellow)</li></ul>"
        );
    }

    #[test]
    fn test_parent_context_in_each() {
        let data = serde_json::json!({
            "prefix": "Item",
            "items": [{"id": 1}, {"id": 2}]
        });
        let result = render_template("{{#each items}}{{prefix}}-{{id}} {{/each}}", &data);
        assert_eq!(result, "Item-1 Item-2 ");
    }
}
