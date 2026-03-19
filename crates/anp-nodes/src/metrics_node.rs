use std::collections::HashMap;
use std::sync::Arc;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;
use tokio::sync::RwLock;

/// MetricsNode — metrics collection with counters, gauges, and histograms.
///
/// Config example:
/// ```json
/// {
///   "operation": "increment",
///   "metric_name": "requests_total",
///   "metric_type": "counter"
/// }
/// ```
///
/// Operations:
/// - `increment`/`decrement`: add/subtract value for counters
/// - `set`: set gauge to specific value
/// - `get`: return current value of a metric
/// - `record`: add a data point to a histogram
/// - `summary`: return all metrics
pub struct MetricsNode {
    operation: String,
    metric_name: Option<String>,
    metric_type: String,
    metrics: Arc<RwLock<HashMap<String, MetricData>>>,
}

#[derive(Debug, Clone)]
enum MetricData {
    Counter(f64),
    Gauge(f64),
    Histogram(Vec<f64>),
}

impl MetricData {
    fn _type_str(&self) -> &str {
        match self {
            MetricData::Counter(_) => "counter",
            MetricData::Gauge(_) => "gauge",
            MetricData::Histogram(_) => "histogram",
        }
    }
}

impl Default for MetricsNode {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsNode {
    pub fn new() -> Self {
        Self {
            operation: "increment".to_string(),
            metric_name: None,
            metric_type: "counter".to_string(),
            metrics: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn compute_percentile(sorted: &[f64], p: f64) -> f64 {
        if sorted.is_empty() {
            return 0.0;
        }
        if sorted.len() == 1 {
            return sorted[0];
        }
        let idx = (p / 100.0) * (sorted.len() - 1) as f64;
        let lower = idx.floor() as usize;
        let upper = idx.ceil() as usize;
        if lower == upper {
            sorted[lower]
        } else {
            let frac = idx - lower as f64;
            sorted[lower] * (1.0 - frac) + sorted[upper] * frac
        }
    }
}

#[async_trait::async_trait]
impl NodeExecutor for MetricsNode {
    fn node_type(&self) -> &str {
        "metrics"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "increment" | "decrement" | "set" | "get" | "record" | "summary" => {
                    self.operation = op.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "invalid operation '{}': must be increment, decrement, set, get, record, or summary",
                        other
                    )));
                }
            }
        }

        if let Some(name) = config.get("metric_name").and_then(|v| v.as_str()) {
            self.metric_name = Some(name.to_string());
        }

        if let Some(mt) = config.get("metric_type").and_then(|v| v.as_str()) {
            match mt {
                "counter" | "gauge" | "histogram" => {
                    self.metric_type = mt.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "invalid metric_type '{}': must be counter, gauge, or histogram",
                        other
                    )));
                }
            }
        }

        // Validate that most operations require metric_name
        match self.operation.as_str() {
            "increment" | "decrement" | "set" | "get" | "record" => {
                if self.metric_name.is_none() {
                    return Err(NodeError::InvalidConfig(format!(
                        "operation '{}' requires 'metric_name'",
                        self.operation
                    )));
                }
            }
            _ => {}
        }

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        match self.operation.as_str() {
            "increment" => {
                let name = self.metric_name.as_ref().unwrap();
                let value = input
                    .get("value")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(1.0);

                let mut metrics = self.metrics.write().await;
                let entry = metrics
                    .entry(name.clone())
                    .or_insert_with(|| MetricData::Counter(0.0));

                match entry {
                    MetricData::Counter(ref mut v) => {
                        *v += value;
                        let current = *v;
                        Ok(NodeOutput::data(serde_json::json!({
                            "metric": name,
                            "type": "counter",
                            "value": current,
                            "operation": "increment",
                        })))
                    }
                    _ => Err(NodeError::ExecutionFailed(format!(
                        "metric '{}' is not a counter",
                        name
                    ))),
                }
            }
            "decrement" => {
                let name = self.metric_name.as_ref().unwrap();
                let value = input
                    .get("value")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(1.0);

                let mut metrics = self.metrics.write().await;
                let entry = metrics
                    .entry(name.clone())
                    .or_insert_with(|| MetricData::Counter(0.0));

                match entry {
                    MetricData::Counter(ref mut v) => {
                        *v -= value;
                        let current = *v;
                        Ok(NodeOutput::data(serde_json::json!({
                            "metric": name,
                            "type": "counter",
                            "value": current,
                            "operation": "decrement",
                        })))
                    }
                    _ => Err(NodeError::ExecutionFailed(format!(
                        "metric '{}' is not a counter",
                        name
                    ))),
                }
            }
            "set" => {
                let name = self.metric_name.as_ref().unwrap();
                let value = input
                    .get("value")
                    .and_then(|v| v.as_f64())
                    .ok_or_else(|| {
                        NodeError::ExecutionFailed("'value' field required for set".into())
                    })?;

                let mut metrics = self.metrics.write().await;
                metrics.insert(name.clone(), MetricData::Gauge(value));

                Ok(NodeOutput::data(serde_json::json!({
                    "metric": name,
                    "type": "gauge",
                    "value": value,
                    "operation": "set",
                })))
            }
            "get" => {
                let name = self.metric_name.as_ref().unwrap();
                let metrics = self.metrics.read().await;

                match metrics.get(name) {
                    Some(MetricData::Counter(v)) => {
                        Ok(NodeOutput::data(serde_json::json!({
                            "metric": name,
                            "type": "counter",
                            "value": *v,
                            "operation": "get",
                        })))
                    }
                    Some(MetricData::Gauge(v)) => {
                        Ok(NodeOutput::data(serde_json::json!({
                            "metric": name,
                            "type": "gauge",
                            "value": *v,
                            "operation": "get",
                        })))
                    }
                    Some(MetricData::Histogram(values)) => {
                        let mut sorted = values.clone();
                        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                        let count = sorted.len();
                        let sum: f64 = sorted.iter().sum();
                        let avg = if count > 0 { sum / count as f64 } else { 0.0 };
                        let min = sorted.first().copied().unwrap_or(0.0);
                        let max = sorted.last().copied().unwrap_or(0.0);
                        let p50 = Self::compute_percentile(&sorted, 50.0);
                        let p90 = Self::compute_percentile(&sorted, 90.0);
                        let p99 = Self::compute_percentile(&sorted, 99.0);

                        Ok(NodeOutput::data(serde_json::json!({
                            "metric": name,
                            "type": "histogram",
                            "count": count,
                            "sum": sum,
                            "avg": avg,
                            "min": min,
                            "max": max,
                            "p50": p50,
                            "p90": p90,
                            "p99": p99,
                            "operation": "get",
                        })))
                    }
                    None => {
                        Ok(NodeOutput::data(serde_json::json!({
                            "metric": name,
                            "type": null,
                            "value": null,
                            "operation": "get",
                            "found": false,
                        })))
                    }
                }
            }
            "record" => {
                let name = self.metric_name.as_ref().unwrap();
                let value = input
                    .get("value")
                    .and_then(|v| v.as_f64())
                    .ok_or_else(|| {
                        NodeError::ExecutionFailed("'value' field required for record".into())
                    })?;

                let mut metrics = self.metrics.write().await;
                let entry = metrics
                    .entry(name.clone())
                    .or_insert_with(|| MetricData::Histogram(Vec::new()));

                match entry {
                    MetricData::Histogram(ref mut values) => {
                        values.push(value);
                        let count = values.len();
                        Ok(NodeOutput::data(serde_json::json!({
                            "metric": name,
                            "type": "histogram",
                            "value": value,
                            "count": count,
                            "operation": "record",
                        })))
                    }
                    _ => Err(NodeError::ExecutionFailed(format!(
                        "metric '{}' is not a histogram",
                        name
                    ))),
                }
            }
            "summary" => {
                let metrics = self.metrics.read().await;
                let mut summary = serde_json::Map::new();

                for (name, data) in metrics.iter() {
                    let metric_info = match data {
                        MetricData::Counter(v) => {
                            serde_json::json!({
                                "type": "counter",
                                "value": *v,
                            })
                        }
                        MetricData::Gauge(v) => {
                            serde_json::json!({
                                "type": "gauge",
                                "value": *v,
                            })
                        }
                        MetricData::Histogram(values) => {
                            let mut sorted = values.clone();
                            sorted.sort_by(|a, b| {
                                a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                            });
                            let count = sorted.len();
                            let sum: f64 = sorted.iter().sum();
                            let avg = if count > 0 { sum / count as f64 } else { 0.0 };
                            let min = sorted.first().copied().unwrap_or(0.0);
                            let max = sorted.last().copied().unwrap_or(0.0);
                            let p50 = Self::compute_percentile(&sorted, 50.0);
                            let p90 = Self::compute_percentile(&sorted, 90.0);
                            let p99 = Self::compute_percentile(&sorted, 99.0);
                            serde_json::json!({
                                "type": "histogram",
                                "count": count,
                                "sum": sum,
                                "avg": avg,
                                "min": min,
                                "max": max,
                                "p50": p50,
                                "p90": p90,
                                "p99": p99,
                            })
                        }
                    };
                    summary.insert(name.clone(), metric_info);
                }

                Ok(NodeOutput::data(serde_json::json!({
                    "metrics": summary,
                })))
            }
            _ => Err(NodeError::ExecutionFailed(format!(
                "unknown operation: {}",
                self.operation
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ExecutionContext {
        ExecutionContext::default()
    }

    #[tokio::test]
    async fn test_increment_counter() {
        let mut node = MetricsNode::new();
        node.init(&serde_json::json!({
            "operation": "increment",
            "metric_name": "requests",
            "metric_type": "counter"
        }))
        .await
        .unwrap();

        let r1 = node
            .execute(serde_json::json!({}), &ctx())
            .await
            .unwrap();
        let out = r1.ports.get("default").unwrap();
        assert_eq!(out["value"], 1.0);
        assert_eq!(out["metric"], "requests");
        assert_eq!(out["type"], "counter");

        let r2 = node
            .execute(serde_json::json!({"value": 5}), &ctx())
            .await
            .unwrap();
        assert_eq!(r2.ports.get("default").unwrap()["value"], 6.0);
    }

    #[tokio::test]
    async fn test_decrement_counter() {
        let mut node = MetricsNode::new();
        node.init(&serde_json::json!({
            "operation": "increment",
            "metric_name": "active",
            "metric_type": "counter"
        }))
        .await
        .unwrap();

        // Increment to 10
        node.execute(serde_json::json!({"value": 10}), &ctx())
            .await
            .unwrap();

        // Now decrement
        let mut dec_node = MetricsNode::new();
        dec_node.metrics = node.metrics.clone();
        dec_node
            .init(&serde_json::json!({
                "operation": "decrement",
                "metric_name": "active",
                "metric_type": "counter"
            }))
            .await
            .unwrap();

        let r = dec_node
            .execute(serde_json::json!({"value": 3}), &ctx())
            .await
            .unwrap();
        assert_eq!(r.ports.get("default").unwrap()["value"], 7.0);
    }

    #[tokio::test]
    async fn test_set_gauge() {
        let mut node = MetricsNode::new();
        node.init(&serde_json::json!({
            "operation": "set",
            "metric_name": "temperature",
            "metric_type": "gauge"
        }))
        .await
        .unwrap();

        let r = node
            .execute(serde_json::json!({"value": 72.5}), &ctx())
            .await
            .unwrap();
        let out = r.ports.get("default").unwrap();
        assert_eq!(out["value"], 72.5);
        assert_eq!(out["type"], "gauge");
    }

    #[tokio::test]
    async fn test_record_histogram_and_get() {
        let mut node = MetricsNode::new();
        node.init(&serde_json::json!({
            "operation": "record",
            "metric_name": "latency",
            "metric_type": "histogram"
        }))
        .await
        .unwrap();

        for val in &[10.0, 20.0, 30.0, 40.0, 50.0] {
            node.execute(serde_json::json!({"value": val}), &ctx())
                .await
                .unwrap();
        }

        let mut get_node = MetricsNode::new();
        get_node.metrics = node.metrics.clone();
        get_node
            .init(&serde_json::json!({
                "operation": "get",
                "metric_name": "latency"
            }))
            .await
            .unwrap();

        let r = get_node
            .execute(serde_json::json!({}), &ctx())
            .await
            .unwrap();
        let out = r.ports.get("default").unwrap();
        assert_eq!(out["type"], "histogram");
        assert_eq!(out["count"], 5);
        assert_eq!(out["sum"], 150.0);
        assert_eq!(out["avg"], 30.0);
        assert_eq!(out["min"], 10.0);
        assert_eq!(out["max"], 50.0);
    }

    #[tokio::test]
    async fn test_summary() {
        let mut node = MetricsNode::new();
        node.init(&serde_json::json!({
            "operation": "increment",
            "metric_name": "counter_a",
            "metric_type": "counter"
        }))
        .await
        .unwrap();
        node.execute(serde_json::json!({"value": 5}), &ctx())
            .await
            .unwrap();

        let mut summary_node = MetricsNode::new();
        summary_node.metrics = node.metrics.clone();
        summary_node
            .init(&serde_json::json!({ "operation": "summary" }))
            .await
            .unwrap();

        let r = summary_node
            .execute(serde_json::json!({}), &ctx())
            .await
            .unwrap();
        let out = r.ports.get("default").unwrap();
        assert_eq!(out["metrics"]["counter_a"]["type"], "counter");
        assert_eq!(out["metrics"]["counter_a"]["value"], 5.0);
    }

    #[tokio::test]
    async fn test_get_nonexistent_metric() {
        let mut node = MetricsNode::new();
        node.init(&serde_json::json!({
            "operation": "get",
            "metric_name": "missing"
        }))
        .await
        .unwrap();

        let r = node
            .execute(serde_json::json!({}), &ctx())
            .await
            .unwrap();
        let out = r.ports.get("default").unwrap();
        assert_eq!(out["found"], false);
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let mut node = MetricsNode::new();
        let result = node
            .init(&serde_json::json!({ "operation": "invalid" }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_node_type_and_kind() {
        let node = MetricsNode::new();
        assert_eq!(node.node_type(), "metrics");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
