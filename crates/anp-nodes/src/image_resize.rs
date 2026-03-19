use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Image Resize node — image metadata detection and operation parameter generation.
///
/// Since we cannot add image processing crates, this node prepares image operation
/// parameters for use with external APIs or command-line tools. It can detect image
/// format from magic bytes and generate tool-compatible parameter sets.
///
/// Config example:
/// ```json
/// {
///   "operation": "metadata",
///   "width": 800,
///   "height": 600,
///   "maintain_aspect": true,
///   "format": "png"
/// }
/// ```
///
/// Input: JSON with image data or parameters.
/// Output: `{ "params": {...}, "api_compatible": { "imagemagick": "...", "sharp": {...} } }`
pub struct ImageResizeNode {
    operation: String,
    width: Option<u32>,
    height: Option<u32>,
    maintain_aspect: bool,
    format: String,
}

impl Default for ImageResizeNode {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageResizeNode {
    pub fn new() -> Self {
        Self {
            operation: "metadata".to_string(),
            width: None,
            height: None,
            maintain_aspect: true,
            format: "png".to_string(),
        }
    }
}

/// Detect image format from the first few bytes (magic bytes) of raw data,
/// or from common base64-encoded prefixes.
fn detect_format_from_data(data: &str) -> Option<&'static str> {
    // Check base64 prefix patterns (common encodings of magic bytes)
    if data.starts_with("iVBOR") {
        return Some("png"); // base64 of 0x89504E47
    }
    if data.starts_with("/9j/") {
        return Some("jpeg"); // base64 of 0xFFD8FF
    }
    if data.starts_with("R0lGOD") {
        return Some("gif"); // base64 of GIF8
    }
    if data.starts_with("UklGR") {
        return Some("webp"); // base64 of RIFF
    }
    if data.starts_with("Qk") {
        return Some("bmp"); // base64 of BM
    }

    // Check hex-encoded magic bytes
    let lower = data.to_lowercase();
    if lower.starts_with("89504e47") {
        return Some("png");
    }
    if lower.starts_with("ffd8ff") {
        return Some("jpeg");
    }
    if lower.starts_with("474946") {
        return Some("gif");
    }
    if lower.starts_with("52494646") && lower.len() >= 24 {
        // RIFF....WEBP
        if &lower[16..24] == "57454250" {
            return Some("webp");
        }
    }
    if lower.starts_with("424d") {
        return Some("bmp");
    }

    None
}

#[async_trait::async_trait]
impl NodeExecutor for ImageResizeNode {
    fn node_type(&self) -> &str {
        "image_resize"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "metadata" | "resize_params" | "crop_params" | "format_convert_params" => {
                    self.operation = op.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown image operation: '{}'. Use: metadata, resize_params, crop_params, format_convert_params",
                        other
                    )));
                }
            }
        }

        if let Some(w) = config.get("width").and_then(|v| v.as_u64()) {
            if w == 0 || w > 65535 {
                return Err(NodeError::InvalidConfig(
                    "width must be between 1 and 65535".into(),
                ));
            }
            self.width = Some(w as u32);
        }

        if let Some(h) = config.get("height").and_then(|v| v.as_u64()) {
            if h == 0 || h > 65535 {
                return Err(NodeError::InvalidConfig(
                    "height must be between 1 and 65535".into(),
                ));
            }
            self.height = Some(h as u32);
        }

        if let Some(maintain) = config.get("maintain_aspect").and_then(|v| v.as_bool()) {
            self.maintain_aspect = maintain;
        }

        if let Some(fmt) = config.get("format").and_then(|v| v.as_str()) {
            match fmt {
                "png" | "jpeg" | "webp" | "gif" | "bmp" => {
                    self.format = fmt.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unsupported image format: '{}'. Use: png, jpeg, webp, gif, bmp",
                        other
                    )));
                }
            }
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let op = input
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.operation);

        match op {
            "metadata" => {
                // Try to detect format from base64/hex data
                let data_str = input
                    .get("data")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                // Strip data URI prefix if present
                let raw_data = if let Some(pos) = data_str.find(',') {
                    &data_str[pos + 1..]
                } else {
                    data_str
                };

                let detected_format = if !raw_data.is_empty() {
                    detect_format_from_data(raw_data)
                } else {
                    None
                };

                // Also accept explicit format hint
                let format_hint = input
                    .get("format")
                    .and_then(|v| v.as_str());

                let mut result = serde_json::json!({
                    "operation": "metadata",
                    "detected_format": detected_format,
                    "format_hint": format_hint.or(detected_format),
                    "data_length": raw_data.len(),
                    "is_base64": !raw_data.is_empty() && raw_data.chars().all(|c| c.is_alphanumeric() || c == '+' || c == '/' || c == '=')
                });

                // Include any dimensions from input
                if let Some(w) = input.get("width").and_then(|v| v.as_u64()) {
                    result["width"] = serde_json::json!(w);
                }
                if let Some(h) = input.get("height").and_then(|v| v.as_u64()) {
                    result["height"] = serde_json::json!(h);
                }

                Ok(NodeOutput::data(result))
            }

            "resize_params" => {
                let width = input
                    .get("width")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .or(self.width)
                    .ok_or_else(|| {
                        NodeError::MissingInput("'width' required for resize_params".into())
                    })?;

                let height = input
                    .get("height")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .or(self.height)
                    .ok_or_else(|| {
                        NodeError::MissingInput("'height' required for resize_params".into())
                    })?;

                let maintain_aspect = input
                    .get("maintain_aspect")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(self.maintain_aspect);

                let format = input
                    .get("format")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&self.format);

                let resize_flag = if maintain_aspect { "" } else { "!" };
                let imagemagick_cmd = format!("-resize {}x{}{}", width, height, resize_flag);

                Ok(NodeOutput::data(serde_json::json!({
                    "operation": "resize_params",
                    "params": {
                        "width": width,
                        "height": height,
                        "maintain_aspect": maintain_aspect,
                        "format": format
                    },
                    "api_compatible": {
                        "imagemagick": imagemagick_cmd,
                        "sharp": {
                            "width": width,
                            "height": height,
                            "fit": if maintain_aspect { "inside" } else { "fill" },
                            "withoutEnlargement": true
                        }
                    }
                })))
            }

            "crop_params" => {
                let width = input
                    .get("width")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .or(self.width)
                    .ok_or_else(|| {
                        NodeError::MissingInput("'width' required for crop_params".into())
                    })?;

                let height = input
                    .get("height")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .or(self.height)
                    .ok_or_else(|| {
                        NodeError::MissingInput("'height' required for crop_params".into())
                    })?;

                let x = input
                    .get("x")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                let y = input
                    .get("y")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;

                let gravity = input
                    .get("gravity")
                    .and_then(|v| v.as_str())
                    .unwrap_or("center");

                let imagemagick_cmd = format!("-crop {}x{}+{}+{}", width, height, x, y);

                Ok(NodeOutput::data(serde_json::json!({
                    "operation": "crop_params",
                    "params": {
                        "width": width,
                        "height": height,
                        "x": x,
                        "y": y,
                        "gravity": gravity
                    },
                    "api_compatible": {
                        "imagemagick": imagemagick_cmd,
                        "sharp": {
                            "left": x,
                            "top": y,
                            "width": width,
                            "height": height
                        }
                    }
                })))
            }

            "format_convert_params" => {
                let target_format = input
                    .get("format")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&self.format);

                let quality = input
                    .get("quality")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(85) as u32;

                let source_format = input
                    .get("source_format")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");

                let mime_type = match target_format {
                    "png" => "image/png",
                    "jpeg" | "jpg" => "image/jpeg",
                    "webp" => "image/webp",
                    "gif" => "image/gif",
                    "bmp" => "image/bmp",
                    _ => "application/octet-stream",
                };

                let imagemagick_cmd = format!(
                    "-quality {} {}:",
                    quality,
                    target_format.to_uppercase()
                );

                Ok(NodeOutput::data(serde_json::json!({
                    "operation": "format_convert_params",
                    "params": {
                        "source_format": source_format,
                        "target_format": target_format,
                        "quality": quality,
                        "mime_type": mime_type
                    },
                    "api_compatible": {
                        "imagemagick": imagemagick_cmd,
                        "sharp": {
                            "format": target_format,
                            "quality": quality
                        }
                    }
                })))
            }

            other => Err(NodeError::ExecutionFailed(format!(
                "unknown image operation: '{}'. Use: metadata, resize_params, crop_params, format_convert_params",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_png_base64() {
        assert_eq!(detect_format_from_data("iVBORw0KGgoAAAA"), Some("png"));
    }

    #[test]
    fn test_detect_jpeg_base64() {
        assert_eq!(detect_format_from_data("/9j/4AAQSkZJ"), Some("jpeg"));
    }

    #[test]
    fn test_detect_gif_base64() {
        assert_eq!(detect_format_from_data("R0lGODlhAQAB"), Some("gif"));
    }

    #[test]
    fn test_detect_png_hex() {
        assert_eq!(detect_format_from_data("89504e470d0a1a0a"), Some("png"));
    }

    #[test]
    fn test_detect_jpeg_hex() {
        assert_eq!(
            detect_format_from_data("ffd8ffe000104a464946"),
            Some("jpeg")
        );
    }

    #[test]
    fn test_detect_unknown() {
        assert_eq!(detect_format_from_data("hello world"), None);
    }

    #[test]
    fn test_detect_empty() {
        assert_eq!(detect_format_from_data(""), None);
    }

    #[tokio::test]
    async fn test_resize_params() {
        let mut node = ImageResizeNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({
            "operation": "resize_params",
            "width": 800,
            "height": 600
        }))
        .await
        .unwrap();

        let result = node.execute(serde_json::json!({}), &ctx).await.unwrap();

        let data = &result.ports["default"];
        assert_eq!(data["params"]["width"], 800);
        assert_eq!(data["params"]["height"], 600);
        assert_eq!(data["params"]["maintain_aspect"], true);
        assert!(data["api_compatible"]["imagemagick"].as_str().is_some());
        assert!(data["api_compatible"]["sharp"].is_object());
    }

    #[tokio::test]
    async fn test_resize_params_no_aspect() {
        let mut node = ImageResizeNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({
            "operation": "resize_params",
            "width": 800,
            "height": 600,
            "maintain_aspect": false
        }))
        .await
        .unwrap();

        let result = node.execute(serde_json::json!({}), &ctx).await.unwrap();

        let data = &result.ports["default"];
        assert_eq!(data["params"]["maintain_aspect"], false);
        let im_cmd = data["api_compatible"]["imagemagick"].as_str().unwrap();
        assert!(im_cmd.contains("!"));
    }

    #[tokio::test]
    async fn test_crop_params() {
        let mut node = ImageResizeNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({"operation": "crop_params"}))
            .await
            .unwrap();

        let result = node
            .execute(
                serde_json::json!({"width": 100, "height": 100, "x": 10, "y": 20}),
                &ctx,
            )
            .await
            .unwrap();

        let data = &result.ports["default"];
        assert_eq!(data["params"]["width"], 100);
        assert_eq!(data["params"]["x"], 10);
        assert_eq!(data["params"]["y"], 20);
        assert_eq!(data["params"]["gravity"], "center");
    }

    #[tokio::test]
    async fn test_format_convert_params() {
        let mut node = ImageResizeNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({"operation": "format_convert_params"}))
            .await
            .unwrap();

        let result = node
            .execute(serde_json::json!({"format": "webp", "quality": 90}), &ctx)
            .await
            .unwrap();

        let data = &result.ports["default"];
        assert_eq!(data["params"]["target_format"], "webp");
        assert_eq!(data["params"]["quality"], 90);
        assert_eq!(data["params"]["mime_type"], "image/webp");
    }

    #[tokio::test]
    async fn test_metadata_with_data() {
        let mut node = ImageResizeNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({"operation": "metadata"}))
            .await
            .unwrap();

        let result = node
            .execute(
                serde_json::json!({"data": "iVBORw0KGgoAAAANSUhEUg=="}),
                &ctx,
            )
            .await
            .unwrap();

        let data = &result.ports["default"];
        assert_eq!(data["detected_format"], "png");
    }

    #[tokio::test]
    async fn test_metadata_no_data() {
        let mut node = ImageResizeNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({"operation": "metadata"}))
            .await
            .unwrap();

        let result = node.execute(serde_json::json!({}), &ctx).await.unwrap();

        let data = &result.ports["default"];
        assert_eq!(data["detected_format"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn test_resize_missing_dimensions() {
        let mut node = ImageResizeNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({"operation": "resize_params"}))
            .await
            .unwrap();

        let result = node.execute(serde_json::json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let mut node = ImageResizeNode::new();
        let result = node
            .init(&serde_json::json!({"operation": "invalid"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_invalid_format() {
        let mut node = ImageResizeNode::new();
        let result = node.init(&serde_json::json!({"format": "tiff"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_metadata_with_data_uri() {
        let mut node = ImageResizeNode::new();
        let ctx = ExecutionContext::new();
        node.init(&serde_json::json!({"operation": "metadata"}))
            .await
            .unwrap();

        let result = node
            .execute(
                serde_json::json!({"data": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUg=="}),
                &ctx,
            )
            .await
            .unwrap();

        let data = &result.ports["default"];
        assert_eq!(data["detected_format"], "png");
    }
}
