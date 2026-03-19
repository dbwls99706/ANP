use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Health check / readiness probe node for production deployments.
/// Returns system health status for load balancers and orchestrators (K8s, ECS).
/// Tracks uptime, request count, error rate, and custom health checks.
pub struct HealthCheckNode {
    operation: String,
    state: Arc<RwLock<HealthState>>,
}

struct HealthState {
    started_at: Instant,
    total_requests: u64,
    total_errors: u64,
    is_ready: bool,
    is_alive: bool,
    checks: Vec<HealthCheck>,
    version: String,
}

struct HealthCheck {
    name: String,
    status: bool,
    message: String,
    last_checked: Instant,
}

impl HealthCheckNode {
    pub fn new() -> Self {
        Self {
            operation: "health".to_string(),
            state: Arc::new(RwLock::new(HealthState {
                started_at: Instant::now(),
                total_requests: 0,
                total_errors: 0,
                is_ready: true,
                is_alive: true,
                checks: Vec::new(),
                version: "0.7.0".to_string(),
            })),
        }
    }
}

impl Default for HealthCheckNode {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NodeExecutor for HealthCheckNode {
    fn node_type(&self) -> &str {
        "health_check"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "health" | "ready" | "live" | "metrics" | "set_ready" | "set_alive"
                | "record_request" | "record_error" | "register_check" | "update_check" => {
                    self.operation = op.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown operation: {}",
                        other
                    )));
                }
            }
        }
        if let Some(version) = config.get("version").and_then(|v| v.as_str()) {
            let mut state = self.state.write().await;
            state.version = version.to_string();
        }
        Ok(())
    }

    async fn execute(&self, input: Value, _ctx: &ExecutionContext) -> NodeResult<NodeOutput> {
        let op = input
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.operation);

        match op {
            "health" => {
                let state = self.state.read().await;
                let uptime_secs = state.started_at.elapsed().as_secs();
                let error_rate = if state.total_requests > 0 {
                    (state.total_errors as f64 / state.total_requests as f64) * 100.0
                } else {
                    0.0
                };

                let checks: Vec<Value> = state
                    .checks
                    .iter()
                    .map(|c| {
                        json!({
                            "name": c.name,
                            "status": if c.status { "pass" } else { "fail" },
                            "message": c.message,
                            "age_secs": c.last_checked.elapsed().as_secs(),
                        })
                    })
                    .collect();

                let all_checks_pass = state.checks.iter().all(|c| c.status);
                let healthy = state.is_alive && state.is_ready && all_checks_pass;

                Ok(NodeOutput::data(json!({
                    "status": if healthy { "healthy" } else { "unhealthy" },
                    "uptime_secs": uptime_secs,
                    "version": state.version,
                    "ready": state.is_ready,
                    "alive": state.is_alive,
                    "total_requests": state.total_requests,
                    "total_errors": state.total_errors,
                    "error_rate": format!("{:.2}%", error_rate),
                    "checks": checks,
                })))
            }

            "ready" => {
                let state = self.state.read().await;
                let all_checks_pass = state.checks.iter().all(|c| c.status);
                let ready = state.is_ready && all_checks_pass;
                if ready {
                    Ok(NodeOutput::data(json!({"status": "ready"})))
                } else {
                    Ok(NodeOutput::data(json!({
                        "status": "not_ready",
                        "_status": 503,
                    })))
                }
            }

            "live" => {
                let state = self.state.read().await;
                if state.is_alive {
                    Ok(NodeOutput::data(json!({"status": "alive"})))
                } else {
                    Ok(NodeOutput::data(json!({
                        "status": "dead",
                        "_status": 503,
                    })))
                }
            }

            "metrics" => {
                let state = self.state.read().await;
                let uptime = state.started_at.elapsed().as_secs();
                let error_rate = if state.total_requests > 0 {
                    state.total_errors as f64 / state.total_requests as f64
                } else {
                    0.0
                };

                // Prometheus-compatible format
                let prometheus = format!(
                    "# HELP anp_uptime_seconds Total uptime in seconds\n\
                     # TYPE anp_uptime_seconds gauge\n\
                     anp_uptime_seconds {}\n\
                     # HELP anp_requests_total Total requests processed\n\
                     # TYPE anp_requests_total counter\n\
                     anp_requests_total {}\n\
                     # HELP anp_errors_total Total errors\n\
                     # TYPE anp_errors_total counter\n\
                     anp_errors_total {}\n\
                     # HELP anp_error_rate Current error rate\n\
                     # TYPE anp_error_rate gauge\n\
                     anp_error_rate {:.4}\n",
                    uptime, state.total_requests, state.total_errors, error_rate,
                );

                Ok(NodeOutput::data(json!({
                    "body": prometheus,
                    "_content_type": "text/plain; version=0.0.4",
                    "uptime_secs": uptime,
                    "total_requests": state.total_requests,
                    "total_errors": state.total_errors,
                    "error_rate": error_rate,
                })))
            }

            "set_ready" => {
                let ready = input.get("ready").and_then(|v| v.as_bool()).unwrap_or(true);
                let mut state = self.state.write().await;
                state.is_ready = ready;
                Ok(NodeOutput::data(json!({"ready": ready})))
            }

            "set_alive" => {
                let alive = input.get("alive").and_then(|v| v.as_bool()).unwrap_or(true);
                let mut state = self.state.write().await;
                state.is_alive = alive;
                Ok(NodeOutput::data(json!({"alive": alive})))
            }

            "record_request" => {
                let mut state = self.state.write().await;
                state.total_requests += 1;
                Ok(NodeOutput::data(json!({
                    "total_requests": state.total_requests
                })))
            }

            "record_error" => {
                let mut state = self.state.write().await;
                state.total_errors += 1;
                state.total_requests += 1;
                Ok(NodeOutput::data(json!({
                    "total_errors": state.total_errors
                })))
            }

            "register_check" => {
                let name = input
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("name required".into()))?;
                let status = input.get("status").and_then(|v| v.as_bool()).unwrap_or(true);
                let message = input
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("ok")
                    .to_string();

                let mut state = self.state.write().await;
                state.checks.push(HealthCheck {
                    name: name.to_string(),
                    status,
                    message,
                    last_checked: Instant::now(),
                });
                Ok(NodeOutput::data(json!({"registered": name})))
            }

            "update_check" => {
                let name = input
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("name required".into()))?;
                let status = input.get("status").and_then(|v| v.as_bool()).unwrap_or(true);
                let message = input
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("ok")
                    .to_string();

                let mut state = self.state.write().await;
                if let Some(check) = state.checks.iter_mut().find(|c| c.name == name) {
                    check.status = status;
                    check.message = message;
                    check.last_checked = Instant::now();
                    Ok(NodeOutput::data(json!({"updated": name, "status": status})))
                } else {
                    Err(NodeError::ExecutionFailed(format!(
                        "check '{}' not found",
                        name
                    )))
                }
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
    async fn test_health() {
        let mut node = HealthCheckNode::new();
        node.init(&json!({"version": "1.0.0"})).await.unwrap();

        let output = node
            .execute(json!({"operation": "health"}), &ctx())
            .await
            .unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["status"], "healthy");
        assert_eq!(data["version"], "1.0.0");
        assert_eq!(data["ready"], true);
        assert_eq!(data["alive"], true);
    }

    #[tokio::test]
    async fn test_ready_live() {
        let mut node = HealthCheckNode::new();
        node.init(&json!({})).await.unwrap();

        let output = node
            .execute(json!({"operation": "ready"}), &ctx())
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["status"], "ready");

        let output = node
            .execute(json!({"operation": "live"}), &ctx())
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["status"], "alive");
    }

    #[tokio::test]
    async fn test_set_not_ready() {
        let mut node = HealthCheckNode::new();
        node.init(&json!({})).await.unwrap();

        node.execute(json!({"operation": "set_ready", "ready": false}), &ctx())
            .await
            .unwrap();

        let output = node
            .execute(json!({"operation": "ready"}), &ctx())
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["status"], "not_ready");
    }

    #[tokio::test]
    async fn test_record_requests_errors() {
        let mut node = HealthCheckNode::new();
        node.init(&json!({})).await.unwrap();

        for _ in 0..10 {
            node.execute(json!({"operation": "record_request"}), &ctx())
                .await
                .unwrap();
        }
        node.execute(json!({"operation": "record_error"}), &ctx())
            .await
            .unwrap();

        let output = node
            .execute(json!({"operation": "health"}), &ctx())
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["total_requests"], 11);
        assert_eq!(output.ports["default"]["total_errors"], 1);
    }

    #[tokio::test]
    async fn test_health_checks() {
        let mut node = HealthCheckNode::new();
        node.init(&json!({})).await.unwrap();

        node.execute(
            json!({"operation": "register_check", "name": "db", "status": true}),
            &ctx(),
        )
        .await
        .unwrap();

        let output = node
            .execute(json!({"operation": "health"}), &ctx())
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["status"], "healthy");

        // Fail the check
        node.execute(
            json!({"operation": "update_check", "name": "db", "status": false, "message": "connection lost"}),
            &ctx(),
        )
        .await
        .unwrap();

        let output = node
            .execute(json!({"operation": "health"}), &ctx())
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["status"], "unhealthy");
    }

    #[tokio::test]
    async fn test_prometheus_metrics() {
        let mut node = HealthCheckNode::new();
        node.init(&json!({})).await.unwrap();

        node.execute(json!({"operation": "record_request"}), &ctx())
            .await
            .unwrap();

        let output = node
            .execute(json!({"operation": "metrics"}), &ctx())
            .await
            .unwrap();
        let body = output.ports["default"]["body"].as_str().unwrap();
        assert!(body.contains("anp_requests_total 1"));
        assert_eq!(
            output.ports["default"]["_content_type"],
            "text/plain; version=0.0.4"
        );
    }
}
