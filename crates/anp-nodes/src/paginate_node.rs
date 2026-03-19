use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// PaginateNode — pagination for arrays.
///
/// Config example:
/// ```json
/// {
///   "page_size": 10,
///   "page_field": "page"
/// }
/// ```
///
/// Input: array or `{ "data": [...], "page": 2 }`
/// Output: `{ "items": [...], "page": 1, "page_size": 10, "total_items": 100, "total_pages": 10, "has_next": true, "has_prev": false, "next_page": 2, "prev_page": null }`
pub struct PaginateNode {
    page_size: usize,
    page_field: String,
}

impl Default for PaginateNode {
    fn default() -> Self {
        Self::new()
    }
}

impl PaginateNode {
    pub fn new() -> Self {
        Self {
            page_size: 10,
            page_field: "page".to_string(),
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for PaginateNode {
    fn node_type(&self) -> &str {
        "paginate"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(ps) = config.get("page_size").and_then(|v| v.as_u64()) {
            if ps == 0 {
                return Err(NodeError::InvalidConfig(
                    "page_size must be greater than 0".into(),
                ));
            }
            self.page_size = ps as usize;
        }

        if let Some(pf) = config.get("page_field").and_then(|v| v.as_str()) {
            self.page_field = pf.to_string();
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        // Extract data array and page number
        let (data, page) = if let Some(arr) = input.as_array() {
            (arr.clone(), 1usize)
        } else if let Some(obj) = input.as_object() {
            let arr = obj
                .get("data")
                .and_then(|v| v.as_array())
                .or_else(|| obj.get("items").and_then(|v| v.as_array()))
                .ok_or_else(|| {
                    NodeError::ExecutionFailed(
                        "input must be an array or object with 'data' or 'items' array".into(),
                    )
                })?
                .clone();

            let page = obj
                .get(&self.page_field)
                .and_then(|v| v.as_u64())
                .map(|p| (p as usize).max(1))
                .unwrap_or(1);

            (arr, page)
        } else {
            return Err(NodeError::ExecutionFailed(
                "input must be an array or object with 'data' array".into(),
            ));
        };

        let total_items = data.len();
        let total_pages = if total_items == 0 {
            1
        } else {
            (total_items + self.page_size - 1) / self.page_size
        };

        // Clamp page to valid range
        let page = page.min(total_pages).max(1);

        let start = (page - 1) * self.page_size;
        let end = (start + self.page_size).min(total_items);
        let items: Vec<serde_json::Value> = data[start..end].to_vec();

        let has_prev = page > 1;
        let has_next = page < total_pages;

        let prev_page: serde_json::Value = if has_prev {
            serde_json::json!(page - 1)
        } else {
            serde_json::Value::Null
        };

        let next_page: serde_json::Value = if has_next {
            serde_json::json!(page + 1)
        } else {
            serde_json::Value::Null
        };

        Ok(NodeOutput::data(serde_json::json!({
            "items": items,
            "page": page,
            "page_size": self.page_size,
            "total_items": total_items,
            "total_pages": total_pages,
            "has_next": has_next,
            "has_prev": has_prev,
            "next_page": next_page,
            "prev_page": prev_page,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_basic_pagination() {
        let mut node = PaginateNode::new();
        node.init(&json!({ "page_size": 2 })).await.unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({
            "data": [1, 2, 3, 4, 5],
            "page": 1
        });
        let output = node.execute(input, &ctx).await.unwrap();
        let result = &output.ports["default"];
        assert_eq!(result["items"], json!([1, 2]));
        assert_eq!(result["page"], 1);
        assert_eq!(result["page_size"], 2);
        assert_eq!(result["total_items"], 5);
        assert_eq!(result["total_pages"], 3);
        assert_eq!(result["has_next"], true);
        assert_eq!(result["has_prev"], false);
        assert_eq!(result["next_page"], 2);
        assert_eq!(result["prev_page"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn test_second_page() {
        let mut node = PaginateNode::new();
        node.init(&json!({ "page_size": 2 })).await.unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({
            "data": [1, 2, 3, 4, 5],
            "page": 2
        });
        let output = node.execute(input, &ctx).await.unwrap();
        let result = &output.ports["default"];
        assert_eq!(result["items"], json!([3, 4]));
        assert_eq!(result["has_next"], true);
        assert_eq!(result["has_prev"], true);
        assert_eq!(result["prev_page"], 1);
        assert_eq!(result["next_page"], 3);
    }

    #[tokio::test]
    async fn test_last_page() {
        let mut node = PaginateNode::new();
        node.init(&json!({ "page_size": 2 })).await.unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({
            "data": [1, 2, 3, 4, 5],
            "page": 3
        });
        let output = node.execute(input, &ctx).await.unwrap();
        let result = &output.ports["default"];
        assert_eq!(result["items"], json!([5]));
        assert_eq!(result["has_next"], false);
        assert_eq!(result["has_prev"], true);
        assert_eq!(result["next_page"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn test_array_input_defaults_to_page_1() {
        let mut node = PaginateNode::new();
        node.init(&json!({ "page_size": 3 })).await.unwrap();

        let ctx = ExecutionContext::new();
        let input = json!([10, 20, 30, 40]);
        let output = node.execute(input, &ctx).await.unwrap();
        let result = &output.ports["default"];
        assert_eq!(result["items"], json!([10, 20, 30]));
        assert_eq!(result["page"], 1);
        assert_eq!(result["total_pages"], 2);
    }

    #[tokio::test]
    async fn test_page_beyond_range_clamped() {
        let mut node = PaginateNode::new();
        node.init(&json!({ "page_size": 5 })).await.unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({
            "data": [1, 2, 3],
            "page": 99
        });
        let output = node.execute(input, &ctx).await.unwrap();
        let result = &output.ports["default"];
        // Should clamp to last page (page 1)
        assert_eq!(result["page"], 1);
        assert_eq!(result["items"], json!([1, 2, 3]));
    }

    #[tokio::test]
    async fn test_empty_data() {
        let mut node = PaginateNode::new();
        node.init(&json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let input = json!({ "data": [] });
        let output = node.execute(input, &ctx).await.unwrap();
        let result = &output.ports["default"];
        assert_eq!(result["items"], json!([]));
        assert_eq!(result["total_items"], 0);
        assert_eq!(result["total_pages"], 1);
        assert_eq!(result["has_next"], false);
        assert_eq!(result["has_prev"], false);
    }

    #[tokio::test]
    async fn test_invalid_page_size() {
        let mut node = PaginateNode::new();
        let result = node.init(&json!({ "page_size": 0 })).await;
        assert!(result.is_err());
    }
}
