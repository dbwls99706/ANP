use async_trait::async_trait;
use serde_json::{json, Value};

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Multipart form data builder and parser.
/// Builds multipart/form-data bodies for file upload via http_client,
/// and parses multipart responses.
pub struct MultipartNode {
    operation: String,
}

impl MultipartNode {
    pub fn new() -> Self {
        Self {
            operation: "build".to_string(),
        }
    }
}

impl Default for MultipartNode {
    fn default() -> Self {
        Self::new()
    }
}

fn generate_boundary() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("----ANPBoundary{:016x}", now)
}

fn build_multipart(parts: &[Value], boundary: &str) -> String {
    let mut body = String::new();
    for part in parts {
        body.push_str(&format!("--{}\r\n", boundary));

        let name = part.get("name").and_then(|v| v.as_str()).unwrap_or("field");
        let filename = part.get("filename").and_then(|v| v.as_str());
        let content_type = part.get("content_type").and_then(|v| v.as_str());

        if let Some(fname) = filename {
            body.push_str(&format!(
                "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n",
                name, fname
            ));
        } else {
            body.push_str(&format!(
                "Content-Disposition: form-data; name=\"{}\"\r\n",
                name
            ));
        }

        if let Some(ct) = content_type {
            body.push_str(&format!("Content-Type: {}\r\n", ct));
        } else if filename.is_some() {
            body.push_str("Content-Type: application/octet-stream\r\n");
        }

        body.push_str("\r\n");

        if let Some(value) = part.get("value").and_then(|v| v.as_str()) {
            body.push_str(value);
        } else if let Some(value) = part.get("value") {
            body.push_str(&value.to_string());
        }
        body.push_str("\r\n");
    }
    body.push_str(&format!("--{}--\r\n", boundary));
    body
}

fn parse_multipart(body: &str, boundary: &str) -> Vec<Value> {
    let mut parts = Vec::new();
    let delimiter = format!("--{}", boundary);
    let sections: Vec<&str> = body.split(&delimiter).collect();

    for section in sections {
        let section = section.trim();
        if section.is_empty() || section.starts_with("--") {
            continue;
        }
        // Remove trailing -- if it's the end delimiter
        let section = if section.ends_with("--") {
            section.strip_suffix("--").unwrap_or(section)
        } else {
            section
        };

        // Split headers from body
        if let Some(idx) = section.find("\r\n\r\n") {
            let headers = &section[..idx];
            let value = section[idx + 4..].trim_end_matches("\r\n");

            let mut part = json!({});
            part["value"] = json!(value);

            for line in headers.lines() {
                let line = line.trim();
                if line.to_lowercase().starts_with("content-disposition:") {
                    let disp = &line["content-disposition:".len()..].trim();
                    // Extract name
                    if let Some(name_start) = disp.find("name=\"") {
                        let rest = &disp[name_start + 6..];
                        if let Some(name_end) = rest.find('"') {
                            part["name"] = json!(&rest[..name_end]);
                        }
                    }
                    // Extract filename
                    if let Some(fn_start) = disp.find("filename=\"") {
                        let rest = &disp[fn_start + 10..];
                        if let Some(fn_end) = rest.find('"') {
                            part["filename"] = json!(&rest[..fn_end]);
                        }
                    }
                } else if line.to_lowercase().starts_with("content-type:") {
                    let ct = line["content-type:".len()..].trim();
                    part["content_type"] = json!(ct);
                }
            }

            parts.push(part);
        } else if let Some(idx) = section.find("\n\n") {
            // Handle \n\n separator (non-CRLF)
            let headers = &section[..idx];
            let value = section[idx + 2..].trim();

            let mut part = json!({"value": value});
            for line in headers.lines() {
                let line = line.trim();
                if line.to_lowercase().starts_with("content-disposition:") {
                    if let Some(name_start) = line.find("name=\"") {
                        let rest = &line[name_start + 6..];
                        if let Some(name_end) = rest.find('"') {
                            part["name"] = json!(&rest[..name_end]);
                        }
                    }
                }
            }
            parts.push(part);
        }
    }
    parts
}

fn build_url_encoded(fields: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(obj) = fields.as_object() {
        for (key, value) in obj {
            let v = value
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| value.to_string());
            parts.push(format!("{}={}", url_encode(key), url_encode(&v)));
        }
    }
    parts.join("&")
}

fn url_encode(s: &str) -> String {
    let mut encoded = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char);
            }
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{:02X}", b)),
        }
    }
    encoded
}

#[async_trait]
impl NodeExecutor for MultipartNode {
    fn node_type(&self) -> &str {
        "multipart"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "build" | "parse" | "url_encode" => self.operation = op.to_string(),
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown operation '{}', expected 'build', 'parse', or 'url_encode'",
                        other
                    )));
                }
            }
        }
        Ok(())
    }

    async fn execute(&self, input: Value, _ctx: &ExecutionContext) -> NodeResult<NodeOutput> {
        let op = input
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.operation);

        match op {
            "build" => {
                let parts = input
                    .get("parts")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| NodeError::MissingInput("parts array required".into()))?;

                let boundary = input
                    .get("boundary")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(generate_boundary);

                let body = build_multipart(parts, &boundary);
                let content_type = format!("multipart/form-data; boundary={}", boundary);

                Ok(NodeOutput::data(json!({
                    "body": body,
                    "boundary": boundary,
                    "content_type": content_type,
                    "_headers": {
                        "Content-Type": content_type
                    },
                    "part_count": parts.len(),
                })))
            }

            "parse" => {
                let body = input
                    .get("body")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("body string required".into()))?;

                let boundary = input
                    .get("boundary")
                    .and_then(|v| v.as_str())
                    .or_else(|| {
                        input
                            .get("content_type")
                            .and_then(|v| v.as_str())
                            .and_then(|ct| ct.find("boundary=").map(|i| &ct[i + 9..]))
                    })
                    .ok_or_else(|| {
                        NodeError::MissingInput(
                            "boundary or content_type with boundary required".into(),
                        )
                    })?;

                let parts = parse_multipart(body, boundary);

                Ok(NodeOutput::data(json!({
                    "parts": parts,
                    "part_count": parts.len(),
                })))
            }

            "url_encode" => {
                let fields = input.get("fields").unwrap_or(&input);
                let encoded = build_url_encoded(fields);
                Ok(NodeOutput::data(json!({
                    "body": encoded,
                    "content_type": "application/x-www-form-urlencoded",
                })))
            }

            other => Err(NodeError::ExecutionFailed(format!(
                "unknown operation: {}",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ExecutionContext {
        ExecutionContext::new()
    }

    #[tokio::test]
    async fn test_build_multipart() {
        let mut node = MultipartNode::new();
        node.init(&json!({"operation": "build"})).await.unwrap();

        let output = node
            .execute(
                json!({
                    "parts": [
                        {"name": "username", "value": "john"},
                        {"name": "avatar", "filename": "photo.jpg", "value": "<binary>", "content_type": "image/jpeg"}
                    ],
                    "boundary": "TestBoundary123"
                }),
                &ctx(),
            )
            .await
            .unwrap();

        let data = &output.ports["default"];
        let body = data["body"].as_str().unwrap();
        assert!(body.contains("--TestBoundary123\r\n"));
        assert!(body.contains("name=\"username\""));
        assert!(body.contains("filename=\"photo.jpg\""));
        assert!(body.contains("Content-Type: image/jpeg"));
        assert!(body.contains("--TestBoundary123--"));
        assert_eq!(data["part_count"], 2);
    }

    #[tokio::test]
    async fn test_parse_multipart() {
        let mut node = MultipartNode::new();
        node.init(&json!({"operation": "parse"})).await.unwrap();

        let body = "--boundary\r\nContent-Disposition: form-data; name=\"field1\"\r\n\r\nvalue1\r\n--boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\nContent-Type: text/plain\r\n\r\nfile content\r\n--boundary--\r\n";

        let output = node
            .execute(json!({"body": body, "boundary": "boundary"}), &ctx())
            .await
            .unwrap();

        let data = &output.ports["default"];
        let parts = data["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["name"], "field1");
        assert_eq!(parts[0]["value"], "value1");
        assert_eq!(parts[1]["name"], "file");
        assert_eq!(parts[1]["filename"], "test.txt");
    }

    #[tokio::test]
    async fn test_url_encode() {
        let mut node = MultipartNode::new();
        node.init(&json!({"operation": "url_encode"}))
            .await
            .unwrap();

        let output = node
            .execute(
                json!({"fields": {"name": "John Doe", "email": "john@example.com"}}),
                &ctx(),
            )
            .await
            .unwrap();

        let data = &output.ports["default"];
        let body = data["body"].as_str().unwrap();
        assert!(body.contains("name=John+Doe"));
        assert!(body.contains("email=john%40example.com"));
    }
}
