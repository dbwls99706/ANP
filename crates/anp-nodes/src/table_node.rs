use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// TableNode — generates HTML tables from arrays of objects.
///
/// Config example:
/// ```json
/// {
///   "columns": [{ "field": "name", "header": "Name", "align": "left" }],
///   "css_class": "data-table",
///   "striped": true,
///   "sortable": false,
///   "caption": "User List",
///   "empty_text": "No data available"
/// }
/// ```
///
/// Input: array of objects, or `{ "data": [...] }`
/// Output: `{ "html": "<table>...</table>", "row_count": N, "column_count": M, "_content_type": "text/html" }`
pub struct TableNode {
    columns: Vec<ColumnDef>,
    css_class: Option<String>,
    striped: bool,
    sortable: bool,
    caption: Option<String>,
    empty_text: String,
}

#[derive(Clone, Debug)]
struct ColumnDef {
    field: String,
    header: String,
    align: String,
}

impl Default for TableNode {
    fn default() -> Self {
        Self::new()
    }
}

impl TableNode {
    pub fn new() -> Self {
        Self {
            columns: Vec::new(),
            css_class: None,
            striped: true,
            sortable: false,
            caption: None,
            empty_text: "No data available".to_string(),
        }
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

#[async_trait::async_trait]
impl NodeExecutor for TableNode {
    fn node_type(&self) -> &str {
        "table"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        self.css_class = config
            .get("css_class")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        self.striped = config
            .get("striped")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        self.sortable = config
            .get("sortable")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        self.caption = config
            .get("caption")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        self.empty_text = config
            .get("empty_text")
            .and_then(|v| v.as_str())
            .unwrap_or("No data available")
            .to_string();

        if let Some(cols) = config.get("columns").and_then(|v| v.as_array()) {
            for c in cols {
                let field = c
                    .get("field")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if field.is_empty() {
                    return Err(NodeError::InvalidConfig(
                        "each column must have a non-empty 'field'".into(),
                    ));
                }
                let header = c
                    .get("header")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&field)
                    .to_string();
                let align = c
                    .get("align")
                    .and_then(|v| v.as_str())
                    .unwrap_or("left")
                    .to_string();
                self.columns.push(ColumnDef {
                    field,
                    header,
                    align,
                });
            }
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let data = if let Some(arr) = input.as_array() {
            arr.clone()
        } else if let Some(arr) = input.get("data").and_then(|v| v.as_array()) {
            arr.clone()
        } else {
            return Err(NodeError::ExecutionFailed(
                "input must be an array or object with 'data' array".into(),
            ));
        };

        // Determine columns: use configured or auto-detect from first row
        let columns = if !self.columns.is_empty() {
            self.columns.clone()
        } else if let Some(first) = data.first().and_then(|v| v.as_object()) {
            first
                .keys()
                .map(|k| ColumnDef {
                    field: k.clone(),
                    header: k.clone(),
                    align: "left".to_string(),
                })
                .collect()
        } else {
            Vec::new()
        };

        let column_count = columns.len();

        // Build table HTML
        let class_attr = match &self.css_class {
            Some(cls) => format!(" class=\"{}\"", html_escape(cls)),
            None => String::new(),
        };

        let mut html = format!("<table{}>", class_attr);

        if let Some(caption) = &self.caption {
            html.push_str(&format!("<caption>{}</caption>", html_escape(caption)));
        }

        // Header
        html.push_str("<thead><tr>");
        for col in &columns {
            let sort_attr = if self.sortable {
                format!(" data-sortable=\"true\" data-field=\"{}\"", html_escape(&col.field))
            } else {
                String::new()
            };
            html.push_str(&format!(
                "<th style=\"text-align: {}\"{}>{}</th>",
                html_escape(&col.align),
                sort_attr,
                html_escape(&col.header)
            ));
        }
        html.push_str("</tr></thead>");

        // Body
        html.push_str("<tbody>");
        if data.is_empty() {
            html.push_str(&format!(
                "<tr><td colspan=\"{}\">{}</td></tr>",
                column_count.max(1),
                html_escape(&self.empty_text)
            ));
        } else {
            for (row_idx, row) in data.iter().enumerate() {
                let row_style = if self.striped && row_idx % 2 == 1 {
                    " style=\"background-color: #f2f2f2\""
                } else {
                    ""
                };
                html.push_str(&format!("<tr{}>", row_style));
                for col in &columns {
                    let val = row
                        .get(&col.field)
                        .map(value_to_string)
                        .unwrap_or_default();
                    html.push_str(&format!(
                        "<td style=\"text-align: {}\">{}</td>",
                        html_escape(&col.align),
                        html_escape(&val)
                    ));
                }
                html.push_str("</tr>");
            }
        }
        html.push_str("</tbody></table>");

        let row_count = data.len();

        Ok(NodeOutput::data(serde_json::json!({
            "html": html,
            "row_count": row_count,
            "column_count": column_count,
            "_content_type": "text/html"
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_basic_table() {
        let mut node = TableNode::new();
        node.init(&json!({
            "columns": [
                { "field": "name", "header": "Name" },
                { "field": "age", "header": "Age", "align": "right" }
            ]
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let input = json!([
            { "name": "Alice", "age": 30 },
            { "name": "Bob", "age": 25 }
        ]);
        let output = node.execute(input, &ctx).await.unwrap();
        let result = &output.ports["default"];
        let html = result["html"].as_str().unwrap();
        assert!(html.contains("<table>"));
        assert!(html.contains("<th"));
        assert!(html.contains("Name"));
        assert!(html.contains("Alice"));
        assert_eq!(result["row_count"], 2);
        assert_eq!(result["column_count"], 2);
        assert_eq!(result["_content_type"], "text/html");
    }

    #[tokio::test]
    async fn test_auto_detect_columns() {
        let mut node = TableNode::new();
        node.init(&json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let input = json!([{ "x": 1, "y": 2 }]);
        let output = node.execute(input, &ctx).await.unwrap();
        let result = &output.ports["default"];
        assert_eq!(result["column_count"], 2);
        assert_eq!(result["row_count"], 1);
    }

    #[tokio::test]
    async fn test_empty_data() {
        let mut node = TableNode::new();
        node.init(&json!({
            "columns": [{ "field": "name", "header": "Name" }],
            "empty_text": "Nothing here"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node.execute(json!([]), &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(html.contains("Nothing here"));
    }

    #[tokio::test]
    async fn test_striped_rows() {
        let mut node = TableNode::new();
        node.init(&json!({
            "columns": [{ "field": "v", "header": "V" }],
            "striped": true
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let input = json!([{ "v": "a" }, { "v": "b" }, { "v": "c" }]);
        let output = node.execute(input, &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(html.contains("background-color: #f2f2f2"));
    }

    #[tokio::test]
    async fn test_xss_escape() {
        let mut node = TableNode::new();
        node.init(&json!({
            "columns": [{ "field": "name", "header": "Name" }]
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let input = json!([{ "name": "<script>alert('xss')</script>" }]);
        let output = node.execute(input, &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[tokio::test]
    async fn test_sortable_data_attrs() {
        let mut node = TableNode::new();
        node.init(&json!({
            "columns": [{ "field": "id", "header": "ID" }],
            "sortable": true
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let input = json!([{ "id": 1 }]);
        let output = node.execute(input, &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(html.contains("data-sortable=\"true\""));
        assert!(html.contains("data-field=\"id\""));
    }

    #[tokio::test]
    async fn test_caption() {
        let mut node = TableNode::new();
        node.init(&json!({
            "columns": [{ "field": "x", "header": "X" }],
            "caption": "My Table"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let input = json!([{ "x": 1 }]);
        let output = node.execute(input, &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(html.contains("<caption>My Table</caption>"));
    }

    #[tokio::test]
    async fn test_data_wrapper() {
        let mut node = TableNode::new();
        node.init(&json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({ "data": [{ "a": 1 }] });
        let output = node.execute(input, &ctx).await.unwrap();
        assert_eq!(output.ports["default"]["row_count"], 1);
    }
}
