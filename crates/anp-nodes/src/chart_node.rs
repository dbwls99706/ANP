use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Chart node — generates SVG charts from data.
///
/// Supports bar, line, and pie chart types.
///
/// Config example:
/// ```json
/// {
///   "chart_type": "bar",
///   "title": "Sales",
///   "x_field": "month",
///   "y_field": "revenue",
///   "width": 400,
///   "height": 300
/// }
/// ```
///
/// Input: array of objects, or `{ "data": [...] }`
/// Output: `{ "svg": "<svg>...</svg>", "chart_type": "bar", "_content_type": "image/svg+xml" }`
pub struct ChartNode {
    chart_type: String,
    title: Option<String>,
    x_field: String,
    y_field: String,
    width: f64,
    height: f64,
}

const PIE_COLORS: &[&str] = &[
    "#4CAF50", "#2196F3", "#FF9800", "#F44336", "#9C27B0", "#00BCD4", "#FF5722", "#795548",
];

impl Default for ChartNode {
    fn default() -> Self {
        Self::new()
    }
}

impl ChartNode {
    pub fn new() -> Self {
        Self {
            chart_type: "bar".to_string(),
            title: None,
            x_field: "x".to_string(),
            y_field: "y".to_string(),
            width: 400.0,
            height: 300.0,
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

fn extract_data_points(
    data: &[serde_json::Value],
    x_field: &str,
    y_field: &str,
) -> Vec<(String, f64)> {
    data.iter()
        .filter_map(|item| {
            let x = match item.get(x_field) {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(v) => v.to_string(),
                None => return None,
            };
            let y = item
                .get(y_field)
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            Some((x, y))
        })
        .collect()
}

fn render_bar(points: &[(String, f64)], width: f64, height: f64, title: &Option<String>) -> String {
    let margin = 40.0;
    let title_offset = if title.is_some() { 30.0 } else { 0.0 };
    let chart_w = width - 2.0 * margin;
    let chart_h = height - 2.0 * margin - title_offset;

    let max_val = points
        .iter()
        .map(|(_, v)| *v)
        .fold(f64::NEG_INFINITY, f64::max)
        .max(1.0);

    let n = points.len().max(1) as f64;
    let bar_width = (chart_w / n) * 0.8;
    let gap = (chart_w / n) * 0.2;

    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\">",
        width, height
    );

    if let Some(t) = title {
        svg.push_str(&format!(
            "<text x=\"{}\" y=\"20\" text-anchor=\"middle\" font-size=\"14\" font-weight=\"bold\">{}</text>",
            width / 2.0,
            html_escape(t)
        ));
    }

    for (i, (label, val)) in points.iter().enumerate() {
        let x = margin + (i as f64) * (bar_width + gap);
        let bar_h = (val / max_val) * chart_h;
        let y = margin + title_offset + chart_h - bar_h;

        svg.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#4CAF50\"/>",
            x, y, bar_width, bar_h
        ));
        svg.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"10\">{}</text>",
            x + bar_width / 2.0,
            height - 10.0,
            html_escape(label)
        ));
    }

    svg.push_str("</svg>");
    svg
}

fn render_line(
    points: &[(String, f64)],
    width: f64,
    height: f64,
    title: &Option<String>,
) -> String {
    let margin = 40.0;
    let title_offset = if title.is_some() { 30.0 } else { 0.0 };
    let chart_w = width - 2.0 * margin;
    let chart_h = height - 2.0 * margin - title_offset;

    let max_val = points
        .iter()
        .map(|(_, v)| *v)
        .fold(f64::NEG_INFINITY, f64::max)
        .max(1.0);

    let n = points.len().max(1);

    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\">",
        width, height
    );

    if let Some(t) = title {
        svg.push_str(&format!(
            "<text x=\"{}\" y=\"20\" text-anchor=\"middle\" font-size=\"14\" font-weight=\"bold\">{}</text>",
            width / 2.0,
            html_escape(t)
        ));
    }

    let coords: Vec<(f64, f64)> = points
        .iter()
        .enumerate()
        .map(|(i, (_, val))| {
            let x = margin + if n > 1 {
                (i as f64) / ((n - 1) as f64) * chart_w
            } else {
                chart_w / 2.0
            };
            let y = margin + title_offset + chart_h - (val / max_val) * chart_h;
            (x, y)
        })
        .collect();

    if !coords.is_empty() {
        let poly_points: Vec<String> = coords
            .iter()
            .map(|(x, y)| format!("{:.1},{:.1}", x, y))
            .collect();
        svg.push_str(&format!(
            "<polyline points=\"{}\" fill=\"none\" stroke=\"#2196F3\" stroke-width=\"2\"/>",
            poly_points.join(" ")
        ));

        for (x, y) in &coords {
            svg.push_str(&format!(
                "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"3\" fill=\"#2196F3\"/>",
                x, y
            ));
        }
    }

    for (i, (label, _)) in points.iter().enumerate() {
        let x = if n > 1 {
            margin + (i as f64) / ((n - 1) as f64) * chart_w
        } else {
            margin + chart_w / 2.0
        };
        svg.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{}\" text-anchor=\"middle\" font-size=\"10\">{}</text>",
            x,
            height - 10.0,
            html_escape(label)
        ));
    }

    svg.push_str("</svg>");
    svg
}

fn render_pie(
    points: &[(String, f64)],
    width: f64,
    height: f64,
    title: &Option<String>,
) -> String {
    let title_offset = if title.is_some() { 30.0 } else { 0.0 };
    let cx = width / 2.0;
    let cy = (height + title_offset) / 2.0;
    let radius = (width.min(height - title_offset) / 2.0 - 20.0).max(10.0);

    let total: f64 = points.iter().map(|(_, v)| v.abs()).sum();

    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\">",
        width, height
    );

    if let Some(t) = title {
        svg.push_str(&format!(
            "<text x=\"{}\" y=\"20\" text-anchor=\"middle\" font-size=\"14\" font-weight=\"bold\">{}</text>",
            width / 2.0,
            html_escape(t)
        ));
    }

    if total == 0.0 {
        svg.push_str(&format!(
            "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"#ccc\"/>",
            cx, cy, radius
        ));
        svg.push_str("</svg>");
        return svg;
    }

    let mut angle = -std::f64::consts::FRAC_PI_2;

    for (i, (_, val)) in points.iter().enumerate() {
        let sweep = (val.abs() / total) * 2.0 * std::f64::consts::PI;
        let color = PIE_COLORS[i % PIE_COLORS.len()];

        let x1 = cx + radius * angle.cos();
        let y1 = cy + radius * angle.sin();
        let end_angle = angle + sweep;
        let x2 = cx + radius * end_angle.cos();
        let y2 = cy + radius * end_angle.sin();
        let large_arc = if sweep > std::f64::consts::PI { 1 } else { 0 };

        svg.push_str(&format!(
            "<path d=\"M {cx} {cy} L {x1:.1} {y1:.1} A {r} {r} 0 {la} 1 {x2:.1} {y2:.1} Z\" fill=\"{color}\"/>",
            cx = cx,
            cy = cy,
            x1 = x1,
            y1 = y1,
            r = radius,
            la = large_arc,
            x2 = x2,
            y2 = y2,
            color = color,
        ));

        angle = end_angle;
    }

    svg.push_str("</svg>");
    svg
}

#[async_trait::async_trait]
impl NodeExecutor for ChartNode {
    fn node_type(&self) -> &str {
        "chart"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        self.chart_type = config
            .get("chart_type")
            .and_then(|v| v.as_str())
            .unwrap_or("bar")
            .to_string();

        if !["bar", "line", "pie"].contains(&self.chart_type.as_str()) {
            return Err(NodeError::InvalidConfig(format!(
                "unsupported chart_type '{}'. Use: bar, line, pie",
                self.chart_type
            )));
        }

        self.title = config
            .get("title")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        self.x_field = config
            .get("x_field")
            .and_then(|v| v.as_str())
            .unwrap_or("x")
            .to_string();

        self.y_field = config
            .get("y_field")
            .and_then(|v| v.as_str())
            .unwrap_or("y")
            .to_string();

        self.width = config
            .get("width")
            .and_then(|v| v.as_f64())
            .unwrap_or(400.0);

        self.height = config
            .get("height")
            .and_then(|v| v.as_f64())
            .unwrap_or(300.0);

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

        if data.is_empty() {
            return Err(NodeError::ExecutionFailed("input data array is empty".into()));
        }

        let points = extract_data_points(&data, &self.x_field, &self.y_field);

        let svg = match self.chart_type.as_str() {
            "bar" => render_bar(&points, self.width, self.height, &self.title),
            "line" => render_line(&points, self.width, self.height, &self.title),
            "pie" => render_pie(&points, self.width, self.height, &self.title),
            _ => unreachable!(),
        };

        Ok(NodeOutput::data(serde_json::json!({
            "svg": svg,
            "chart_type": self.chart_type,
            "_content_type": "image/svg+xml"
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_bar_chart() {
        let mut node = ChartNode::new();
        node.init(&serde_json::json!({
            "chart_type": "bar",
            "x_field": "name",
            "y_field": "value"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let input = serde_json::json!([
            { "name": "A", "value": 10 },
            { "name": "B", "value": 20 },
            { "name": "C", "value": 15 }
        ]);
        let output = node.execute(input, &ctx).await.unwrap();
        let result = &output.ports["default"];
        let svg = result["svg"].as_str().unwrap();
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("<rect"));
        assert_eq!(result["chart_type"], "bar");
        assert_eq!(result["_content_type"], "image/svg+xml");
    }

    #[tokio::test]
    async fn test_line_chart() {
        let mut node = ChartNode::new();
        node.init(&serde_json::json!({
            "chart_type": "line",
            "x_field": "x",
            "y_field": "y"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let input = serde_json::json!([
            { "x": "Jan", "y": 100 },
            { "x": "Feb", "y": 200 }
        ]);
        let output = node.execute(input, &ctx).await.unwrap();
        let svg = output.ports["default"]["svg"].as_str().unwrap();
        assert!(svg.contains("<polyline"));
        assert!(svg.contains("<circle"));
    }

    #[tokio::test]
    async fn test_pie_chart() {
        let mut node = ChartNode::new();
        node.init(&serde_json::json!({
            "chart_type": "pie",
            "x_field": "label",
            "y_field": "val"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let input = serde_json::json!([
            { "label": "Red", "val": 30 },
            { "label": "Blue", "val": 70 }
        ]);
        let output = node.execute(input, &ctx).await.unwrap();
        let svg = output.ports["default"]["svg"].as_str().unwrap();
        assert!(svg.contains("<path"));
        assert!(svg.contains("#4CAF50"));
    }

    #[tokio::test]
    async fn test_invalid_chart_type() {
        let mut node = ChartNode::new();
        let result = node
            .init(&serde_json::json!({ "chart_type": "donut" }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_data_wrapper() {
        let mut node = ChartNode::new();
        node.init(&serde_json::json!({
            "x_field": "k",
            "y_field": "v"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let input = serde_json::json!({
            "data": [{ "k": "X", "v": 5 }]
        });
        let output = node.execute(input, &ctx).await.unwrap();
        assert!(output.ports["default"]["svg"].as_str().unwrap().contains("<svg"));
    }

    #[tokio::test]
    async fn test_empty_data() {
        let mut node = ChartNode::new();
        node.init(&serde_json::json!({})).await.unwrap();

        let ctx = ExecutionContext::new();
        let result = node.execute(serde_json::json!([]), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_title() {
        let mut node = ChartNode::new();
        node.init(&serde_json::json!({
            "chart_type": "bar",
            "title": "My Chart",
            "x_field": "x",
            "y_field": "y"
        }))
        .await
        .unwrap();

        let ctx = ExecutionContext::new();
        let input = serde_json::json!([{ "x": "A", "y": 10 }]);
        let output = node.execute(input, &ctx).await.unwrap();
        let svg = output.ports["default"]["svg"].as_str().unwrap();
        assert!(svg.contains("My Chart"));
    }
}
