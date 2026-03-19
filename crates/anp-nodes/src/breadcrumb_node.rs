use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// BreadcrumbNode — generates breadcrumb navigation HTML.
///
/// Config example:
/// ```json
/// {
///   "separator": " / ",
///   "items": [{ "label": "Home", "url": "/" }, { "label": "Products", "url": "/products" }],
///   "home_label": "Home",
///   "home_url": "/"
/// }
/// ```
///
/// Input can have `breadcrumbs` array or `path` string (splits by "/" to generate breadcrumbs).
/// Output: `{ "html": "<nav class='breadcrumbs'>...</nav>", "items": [...], "depth": N, "_content_type": "text/html" }`
pub struct BreadcrumbNode {
    separator: String,
    items: Vec<BreadcrumbItem>,
    home_label: String,
    home_url: String,
}

#[derive(Clone, Debug)]
struct BreadcrumbItem {
    label: String,
    url: String,
}

impl Default for BreadcrumbNode {
    fn default() -> Self {
        Self::new()
    }
}

impl BreadcrumbNode {
    pub fn new() -> Self {
        Self {
            separator: " / ".to_string(),
            items: Vec::new(),
            home_label: "Home".to_string(),
            home_url: "/".to_string(),
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

fn path_to_breadcrumbs(path: &str) -> Vec<BreadcrumbItem> {
    let segments: Vec<&str> = path
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    let mut items = Vec::new();
    let mut accumulated = String::new();

    for seg in &segments {
        accumulated.push('/');
        accumulated.push_str(seg);

        // Capitalize first letter for label
        let label = if seg.is_empty() {
            seg.to_string()
        } else {
            let mut chars = seg.chars();
            match chars.next() {
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    format!("{}{}", upper, chars.as_str())
                }
                None => String::new(),
            }
        };

        items.push(BreadcrumbItem {
            label,
            url: accumulated.clone(),
        });
    }

    items
}

#[async_trait::async_trait]
impl NodeExecutor for BreadcrumbNode {
    fn node_type(&self) -> &str {
        "breadcrumb"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(sep) = config.get("separator").and_then(|v| v.as_str()) {
            self.separator = sep.to_string();
        }

        if let Some(hl) = config.get("home_label").and_then(|v| v.as_str()) {
            self.home_label = hl.to_string();
        }

        if let Some(hu) = config.get("home_url").and_then(|v| v.as_str()) {
            self.home_url = hu.to_string();
        }

        if let Some(items) = config.get("items").and_then(|v| v.as_array()) {
            for item in items {
                let label = item
                    .get("label")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let url = item
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                self.items.push(BreadcrumbItem { label, url });
            }
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        // Start with home item
        let mut all_items = vec![BreadcrumbItem {
            label: self.home_label.clone(),
            url: self.home_url.clone(),
        }];

        // Add static items
        all_items.extend(self.items.iter().cloned());

        // Merge dynamic items from input
        if let Some(breadcrumbs) = input.get("breadcrumbs").and_then(|v| v.as_array()) {
            for bc in breadcrumbs {
                let label = bc
                    .get("label")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let url = bc
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                all_items.push(BreadcrumbItem { label, url });
            }
        } else if let Some(path) = input.get("path").and_then(|v| v.as_str()) {
            let path_items = path_to_breadcrumbs(path);
            all_items.extend(path_items);
        }

        let depth = all_items.len();
        let escaped_sep = html_escape(&self.separator);

        // Build HTML: all items except last are links, last is plain text
        let mut parts: Vec<String> = Vec::new();
        for (i, item) in all_items.iter().enumerate() {
            if i == all_items.len() - 1 {
                // Last item: plain text (current page)
                parts.push(format!(
                    "<span>{}</span>",
                    html_escape(&item.label)
                ));
            } else {
                parts.push(format!(
                    "<a href=\"{}\">{}</a>",
                    html_escape(&item.url),
                    html_escape(&item.label)
                ));
            }
        }

        let nav_html = format!(
            "<nav class=\"breadcrumbs\">{}</nav>",
            parts.join(&escaped_sep)
        );

        let items_json: Vec<serde_json::Value> = all_items
            .iter()
            .map(|item| {
                serde_json::json!({
                    "label": item.label,
                    "url": item.url
                })
            })
            .collect();

        Ok(NodeOutput::data(serde_json::json!({
            "html": nav_html,
            "items": items_json,
            "depth": depth,
            "_content_type": "text/html"
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_basic_breadcrumb() {
        let mut node = BreadcrumbNode::new();
        node.init(&json!({
            "items": [
                { "label": "Products", "url": "/products" }
            ]
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node.execute(json!({}), &ctx).await.unwrap();
        let result = &output.ports["default"];
        let html = result["html"].as_str().unwrap();
        assert!(html.contains("<nav class=\"breadcrumbs\">"));
        assert!(html.contains("<a href=\"/\">Home</a>"));
        assert!(html.contains("<span>Products</span>"));
        assert_eq!(result["depth"], 2);
        assert_eq!(result["_content_type"], "text/html");
    }

    #[tokio::test]
    async fn test_path_input() {
        let mut node = BreadcrumbNode::new();
        node.init(&json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({ "path": "/products/shoes" });
        let output = node.execute(input, &ctx).await.unwrap();
        let result = &output.ports["default"];
        let html = result["html"].as_str().unwrap();
        assert!(html.contains("Products"));
        assert!(html.contains("Shoes"));
        assert_eq!(result["depth"], 3); // Home + Products + Shoes
    }

    #[tokio::test]
    async fn test_dynamic_breadcrumbs() {
        let mut node = BreadcrumbNode::new();
        node.init(&json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({
            "breadcrumbs": [
                { "label": "Docs", "url": "/docs" },
                { "label": "API", "url": "/docs/api" }
            ]
        });
        let output = node.execute(input, &ctx).await.unwrap();
        let result = &output.ports["default"];
        assert_eq!(result["depth"], 3); // Home + Docs + API
        let html = result["html"].as_str().unwrap();
        assert!(html.contains("<a href=\"/docs\">Docs</a>"));
        assert!(html.contains("<span>API</span>")); // last item is span
    }

    #[tokio::test]
    async fn test_custom_separator() {
        let mut node = BreadcrumbNode::new();
        node.init(&json!({
            "separator": " > ",
            "items": [{ "label": "About", "url": "/about" }]
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node.execute(json!({}), &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(html.contains(" &gt; "));
    }

    #[tokio::test]
    async fn test_home_only() {
        let mut node = BreadcrumbNode::new();
        node.init(&json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let output = node.execute(json!({}), &ctx).await.unwrap();
        let result = &output.ports["default"];
        assert_eq!(result["depth"], 1);
        let html = result["html"].as_str().unwrap();
        // When home is the only item, it should be a span (current page)
        assert!(html.contains("<span>Home</span>"));
    }

    #[tokio::test]
    async fn test_custom_home_label() {
        let mut node = BreadcrumbNode::new();
        node.init(&json!({
            "home_label": "Start",
            "home_url": "/start",
            "items": [{ "label": "End", "url": "/end" }]
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let output = node.execute(json!({}), &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(html.contains("<a href=\"/start\">Start</a>"));
    }

    #[tokio::test]
    async fn test_xss_escape() {
        let mut node = BreadcrumbNode::new();
        node.init(&json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({
            "breadcrumbs": [
                { "label": "<script>alert(1)</script>", "url": "/test" }
            ]
        });
        let output = node.execute(input, &ctx).await.unwrap();
        let html = output.ports["default"]["html"].as_str().unwrap();
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }
}
