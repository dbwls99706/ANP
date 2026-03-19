use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Email node — prepares email payloads for sending via HTTP API.
///
/// Config:
/// ```json
/// {
///   "smtp_host_env": "SMTP_HOST",
///   "smtp_port": 587,
///   "username_env": "SMTP_USER",
///   "password_env": "SMTP_PASS",
///   "from": "noreply@example.com"
/// }
/// ```
///
/// Kind: Sink — takes input with `to`, `subject`, and `body` fields and
/// formats it as a complete email object ready to be sent via `http_client`
/// to an email API (e.g., SendGrid).
///
/// Output includes both a generic email object and a SendGrid-compatible
/// API payload.
pub struct EmailNode {
    from: String,
}

impl Default for EmailNode {
    fn default() -> Self {
        Self::new()
    }
}

impl EmailNode {
    pub fn new() -> Self {
        Self {
            from: "noreply@example.com".to_string(),
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for EmailNode {
    fn node_type(&self) -> &str {
        "email"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Sink
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(from) = config.get("from").and_then(|v| v.as_str()) {
            self.from = from.to_string();
        }
        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let to = input
            .get("to")
            .and_then(|v| v.as_str())
            .ok_or_else(|| NodeError::MissingInput("missing 'to' field".into()))?;

        let subject = input
            .get("subject")
            .and_then(|v| v.as_str())
            .ok_or_else(|| NodeError::MissingInput("missing 'subject' field".into()))?;

        let body = input
            .get("body")
            .and_then(|v| v.as_str())
            .ok_or_else(|| NodeError::MissingInput("missing 'body' field".into()))?;

        // Determine content type (default to text/plain, use text/html if body looks like HTML)
        let content_type = if body.contains('<') && body.contains('>') {
            "text/html"
        } else {
            "text/plain"
        };

        // Generic email object
        let email = serde_json::json!({
            "to": to,
            "from": self.from,
            "subject": subject,
            "body": body,
            "headers": {
                "From": self.from,
                "To": to,
                "Subject": subject,
                "Content-Type": content_type,
                "MIME-Version": "1.0"
            }
        });

        // SendGrid-compatible API payload
        let api_payload = serde_json::json!({
            "personalizations": [
                {
                    "to": [{ "email": to }]
                }
            ],
            "from": {
                "email": self.from
            },
            "subject": subject,
            "content": [
                {
                    "type": content_type,
                    "value": body
                }
            ]
        });

        Ok(NodeOutput::data(serde_json::json!({
            "email": email,
            "api_payload": api_payload
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_email() {
        let mut node = EmailNode::new();
        node.init(&serde_json::json!({"from": "sender@test.com"}))
            .await
            .unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({
                    "to": "recipient@test.com",
                    "subject": "Hello",
                    "body": "This is a test email"
                }),
                &ctx,
            )
            .await
            .unwrap();

        let result = &output.ports["default"];
        assert_eq!(result["email"]["to"], "recipient@test.com");
        assert_eq!(result["email"]["from"], "sender@test.com");
        assert_eq!(result["email"]["subject"], "Hello");
        assert_eq!(result["email"]["body"], "This is a test email");
        assert_eq!(result["email"]["headers"]["Content-Type"], "text/plain");

        // Check SendGrid payload
        let api = &result["api_payload"];
        assert_eq!(api["personalizations"][0]["to"][0]["email"], "recipient@test.com");
        assert_eq!(api["from"]["email"], "sender@test.com");
        assert_eq!(api["subject"], "Hello");
        assert_eq!(api["content"][0]["type"], "text/plain");
        assert_eq!(api["content"][0]["value"], "This is a test email");
    }

    #[tokio::test]
    async fn test_html_email() {
        let mut node = EmailNode::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let output = node
            .execute(
                serde_json::json!({
                    "to": "user@example.com",
                    "subject": "HTML Test",
                    "body": "<h1>Hello</h1><p>World</p>"
                }),
                &ctx,
            )
            .await
            .unwrap();

        let result = &output.ports["default"];
        assert_eq!(result["email"]["headers"]["Content-Type"], "text/html");
        assert_eq!(result["api_payload"]["content"][0]["type"], "text/html");
    }

    #[tokio::test]
    async fn test_missing_to() {
        let mut node = EmailNode::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let result = node
            .execute(
                serde_json::json!({"subject": "test", "body": "test"}),
                &ctx,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_missing_subject() {
        let mut node = EmailNode::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let result = node
            .execute(
                serde_json::json!({"to": "user@test.com", "body": "test"}),
                &ctx,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_missing_body() {
        let mut node = EmailNode::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let result = node
            .execute(
                serde_json::json!({"to": "user@test.com", "subject": "test"}),
                &ctx,
            )
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_node_type_and_kind() {
        let node = EmailNode::new();
        assert_eq!(node.node_type(), "email");
        assert_eq!(node.node_kind(), NodeKind::Sink);
    }
}
