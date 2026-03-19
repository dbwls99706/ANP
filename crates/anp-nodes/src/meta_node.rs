use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// MetaNode — generates HTML meta tags and SEO metadata.
///
/// Config example:
/// ```json
/// {
///   "defaults": {
///     "description": "Default site description",
///     "robots": "index, follow"
///   }
/// }
/// ```
///
/// Input: `{ "title": "Page Title", "description": "...", "og_title": "...", ... }`
/// Output: `{ "meta_html": "<meta ...>\n...", "tags_count": N }`
pub struct MetaNode {
    defaults: serde_json::Value,
}

impl Default for MetaNode {
    fn default() -> Self {
        Self::new()
    }
}

impl MetaNode {
    pub fn new() -> Self {
        Self {
            defaults: serde_json::Value::Object(serde_json::Map::new()),
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

fn get_field<'a>(
    input: &'a serde_json::Value,
    defaults: &'a serde_json::Value,
    key: &str,
) -> Option<String> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .or_else(|| defaults.get(key).and_then(|v| v.as_str()))
        .map(|s| s.to_string())
}

#[async_trait::async_trait]
impl NodeExecutor for MetaNode {
    fn node_type(&self) -> &str {
        "meta"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(defaults) = config.get("defaults") {
            self.defaults = defaults.clone();
        }
        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let mut tags: Vec<String> = Vec::new();

        // Standard meta tags
        if let Some(title) = get_field(&input, &self.defaults, "title") {
            tags.push(format!("<title>{}</title>", html_escape(&title)));
        }

        if let Some(description) = get_field(&input, &self.defaults, "description") {
            tags.push(format!(
                "<meta name=\"description\" content=\"{}\">",
                html_escape(&description)
            ));
        }

        if let Some(keywords) = get_field(&input, &self.defaults, "keywords") {
            tags.push(format!(
                "<meta name=\"keywords\" content=\"{}\">",
                html_escape(&keywords)
            ));
        }

        if let Some(author) = get_field(&input, &self.defaults, "author") {
            tags.push(format!(
                "<meta name=\"author\" content=\"{}\">",
                html_escape(&author)
            ));
        }

        if let Some(robots) = get_field(&input, &self.defaults, "robots") {
            tags.push(format!(
                "<meta name=\"robots\" content=\"{}\">",
                html_escape(&robots)
            ));
        }

        // Open Graph tags
        if let Some(og_title) = get_field(&input, &self.defaults, "og_title") {
            tags.push(format!(
                "<meta property=\"og:title\" content=\"{}\">",
                html_escape(&og_title)
            ));
        }

        if let Some(og_description) = get_field(&input, &self.defaults, "og_description") {
            tags.push(format!(
                "<meta property=\"og:description\" content=\"{}\">",
                html_escape(&og_description)
            ));
        }

        if let Some(og_image) = get_field(&input, &self.defaults, "og_image") {
            tags.push(format!(
                "<meta property=\"og:image\" content=\"{}\">",
                html_escape(&og_image)
            ));
        }

        // Canonical link
        if let Some(canonical) = get_field(&input, &self.defaults, "canonical") {
            tags.push(format!(
                "<link rel=\"canonical\" href=\"{}\">",
                html_escape(&canonical)
            ));
        }

        let tags_count = tags.len();
        let meta_html = tags.join("\n");

        Ok(NodeOutput::data(serde_json::json!({
            "meta_html": meta_html,
            "tags_count": tags_count,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_basic_meta() {
        let mut node = MetaNode::new();
        node.init(&json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({
            "title": "My Page",
            "description": "A great page"
        });
        let output = node.execute(input, &ctx).await.unwrap();
        let result = &output.ports["default"];
        let meta = result["meta_html"].as_str().unwrap();
        assert!(meta.contains("<title>My Page</title>"));
        assert!(meta.contains("name=\"description\" content=\"A great page\""));
        assert_eq!(result["tags_count"], 2);
    }

    #[tokio::test]
    async fn test_open_graph() {
        let mut node = MetaNode::new();
        node.init(&json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({
            "og_title": "OG Title",
            "og_description": "OG Desc",
            "og_image": "https://example.com/img.png"
        });
        let output = node.execute(input, &ctx).await.unwrap();
        let meta = output.ports["default"]["meta_html"].as_str().unwrap();
        assert!(meta.contains("property=\"og:title\" content=\"OG Title\""));
        assert!(meta.contains("property=\"og:description\" content=\"OG Desc\""));
        assert!(meta.contains("property=\"og:image\" content=\"https://example.com/img.png\""));
    }

    #[tokio::test]
    async fn test_canonical_and_robots() {
        let mut node = MetaNode::new();
        node.init(&json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({
            "canonical": "https://example.com/page",
            "robots": "noindex, nofollow"
        });
        let output = node.execute(input, &ctx).await.unwrap();
        let meta = output.ports["default"]["meta_html"].as_str().unwrap();
        assert!(meta.contains("rel=\"canonical\" href=\"https://example.com/page\""));
        assert!(meta.contains("name=\"robots\" content=\"noindex, nofollow\""));
    }

    #[tokio::test]
    async fn test_defaults_fallback() {
        let mut node = MetaNode::new();
        node.init(&json!({
            "defaults": {
                "description": "Default description",
                "robots": "index, follow"
            }
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({ "title": "Custom Title" });
        let output = node.execute(input, &ctx).await.unwrap();
        let meta = output.ports["default"]["meta_html"].as_str().unwrap();
        assert!(meta.contains("Custom Title"));
        assert!(meta.contains("Default description"));
        assert!(meta.contains("index, follow"));
    }

    #[tokio::test]
    async fn test_input_overrides_defaults() {
        let mut node = MetaNode::new();
        node.init(&json!({
            "defaults": {
                "description": "Default desc"
            }
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({ "description": "Custom desc" });
        let output = node.execute(input, &ctx).await.unwrap();
        let meta = output.ports["default"]["meta_html"].as_str().unwrap();
        assert!(meta.contains("Custom desc"));
        assert!(!meta.contains("Default desc"));
    }

    #[tokio::test]
    async fn test_xss_escape() {
        let mut node = MetaNode::new();
        node.init(&json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({ "title": "<script>alert('xss')</script>" });
        let output = node.execute(input, &ctx).await.unwrap();
        let meta = output.ports["default"]["meta_html"].as_str().unwrap();
        assert!(!meta.contains("<script>"));
        assert!(meta.contains("&lt;script&gt;"));
    }

    #[tokio::test]
    async fn test_empty_input() {
        let mut node = MetaNode::new();
        node.init(&json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let output = node.execute(json!({}), &ctx).await.unwrap();
        let result = &output.ports["default"];
        assert_eq!(result["tags_count"], 0);
        assert_eq!(result["meta_html"], "");
    }
}
