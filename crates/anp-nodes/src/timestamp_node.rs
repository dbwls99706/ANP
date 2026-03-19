use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;
use std::time::SystemTime;

/// Timestamp node — date/time operations.
///
/// Config:
/// ```json
/// { "operation": "now", "format": "%Y-%m-%d %H:%M:%S", "unit": "seconds" }
/// ```
///
/// Operations:
/// - `now`: returns current timestamp info
/// - `unix`: returns just epoch seconds
/// - `from_unix`: converts epoch seconds (from input `unix` field) to timestamp string
/// - `add`: adds `amount` (from input or config) to a unix timestamp
/// - `subtract`: subtracts `amount` from a unix timestamp
/// - `diff`: computes difference between `start` and `end` unix timestamps from input
pub struct TimestampNode {
    operation: String,
    format: String,
    unit: String,
    amount: Option<i64>,
}

impl Default for TimestampNode {
    fn default() -> Self {
        Self::new()
    }
}

impl TimestampNode {
    pub fn new() -> Self {
        Self {
            operation: "now".to_string(),
            format: "%Y-%m-%d %H:%M:%S".to_string(),
            unit: "seconds".to_string(),
            amount: None,
        }
    }
}

/// Format a unix timestamp as a simple ISO-like string (no chrono dependency).
fn format_timestamp(epoch_secs: u64) -> String {
    // Calculate date/time components from epoch seconds
    let mut remaining = epoch_secs as i64;

    let secs_per_minute = 60i64;
    let _secs_per_hour = 3600i64;
    let _secs_per_day = 86400i64;

    let second = remaining % secs_per_minute;
    remaining /= secs_per_minute;
    let minute = remaining % 60;
    remaining /= 60;
    let hour = remaining % 24;
    let mut days = remaining / 24;

    // Calculate year and day-of-year from days since epoch (1970-01-01)
    let mut year = 1970i64;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    // Calculate month and day
    let leap = is_leap_year(year);
    let month_days: [i64; 12] = if leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if days < md {
            month = i + 1;
            break;
        }
        days -= md;
    }
    if month == 0 {
        month = 12;
    }
    let day = days + 1;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hour, minute, second
    )
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn current_epoch() -> (u64, u128) {
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    (duration.as_secs(), duration.as_millis())
}

#[async_trait::async_trait]
impl NodeExecutor for TimestampNode {
    fn node_type(&self) -> &str {
        "timestamp"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            self.operation = op.to_string();
        }
        if let Some(fmt) = config.get("format").and_then(|v| v.as_str()) {
            self.format = fmt.to_string();
        }
        if let Some(u) = config.get("unit").and_then(|v| v.as_str()) {
            self.unit = u.to_string();
        }
        if let Some(amt) = config.get("amount").and_then(|v| v.as_i64()) {
            self.amount = Some(amt);
        }
        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        match self.operation.as_str() {
            "now" => {
                let (secs, millis) = current_epoch();
                let ts = format_timestamp(secs);
                let result = serde_json::json!({
                    "timestamp": ts,
                    "unix": secs,
                    "unix_millis": millis as u64
                });
                Ok(NodeOutput::data(result))
            }

            "unix" => {
                let (secs, _) = current_epoch();
                Ok(NodeOutput::data(serde_json::json!(secs)))
            }

            "from_unix" => {
                let epoch = input
                    .get("unix")
                    .or_else(|| input.get("timestamp"))
                    .and_then(|v| v.as_u64())
                    .or_else(|| input.as_u64())
                    .ok_or_else(|| {
                        NodeError::ExecutionFailed(
                            "from_unix requires a 'unix' field or numeric input".into(),
                        )
                    })?;
                let ts = format_timestamp(epoch);
                let result = serde_json::json!({
                    "timestamp": ts,
                    "unix": epoch
                });
                Ok(NodeOutput::data(result))
            }

            "add" => {
                let base = input.get("unix").and_then(|v| v.as_i64()).ok_or_else(|| {
                    NodeError::ExecutionFailed("add requires 'unix' field in input".into())
                })?;
                let amount = self
                    .amount
                    .or_else(|| input.get("amount").and_then(|v| v.as_i64()))
                    .unwrap_or(0);

                let new_unix = base + amount;
                let ts = format_timestamp(new_unix as u64);
                let result = serde_json::json!({
                    "timestamp": ts,
                    "unix": new_unix,
                    "previous_unix": base
                });
                Ok(NodeOutput::data(result))
            }

            "subtract" => {
                let base = input.get("unix").and_then(|v| v.as_i64()).ok_or_else(|| {
                    NodeError::ExecutionFailed("subtract requires 'unix' field in input".into())
                })?;
                let amount = self
                    .amount
                    .or_else(|| input.get("amount").and_then(|v| v.as_i64()))
                    .unwrap_or(0);

                let new_unix = base - amount;
                let ts = format_timestamp(new_unix as u64);
                let result = serde_json::json!({
                    "timestamp": ts,
                    "unix": new_unix,
                    "previous_unix": base
                });
                Ok(NodeOutput::data(result))
            }

            "diff" => {
                let start = input.get("start").and_then(|v| v.as_i64()).ok_or_else(|| {
                    NodeError::ExecutionFailed("diff requires 'start' field in input".into())
                })?;
                let end = input.get("end").and_then(|v| v.as_i64()).ok_or_else(|| {
                    NodeError::ExecutionFailed("diff requires 'end' field in input".into())
                })?;

                let diff = end - start;
                let result = serde_json::json!({
                    "difference": diff,
                    "abs_difference": diff.unsigned_abs(),
                    "start": start,
                    "end": end
                });
                Ok(NodeOutput::data(result))
            }

            other => Err(NodeError::InvalidConfig(format!(
                "unknown timestamp operation: '{}'. Use: now, unix, from_unix, add, subtract, diff",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_now() {
        let mut node = TimestampNode::new();
        node.init(&serde_json::json!({"operation": "now"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        assert!(output.ports["default"]["timestamp"].is_string());
        assert!(output.ports["default"]["unix"].is_number());
        assert!(output.ports["default"]["unix_millis"].is_number());
    }

    #[tokio::test]
    async fn test_unix() {
        let mut node = TimestampNode::new();
        node.init(&serde_json::json!({"operation": "unix"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let output = node.execute(serde_json::Value::Null, &ctx).await.unwrap();
        assert!(output.ports["default"].is_number());
    }

    #[tokio::test]
    async fn test_from_unix() {
        let mut node = TimestampNode::new();
        node.init(&serde_json::json!({"operation": "from_unix"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let input = serde_json::json!({"unix": 0});
        let output = node.execute(input, &ctx).await.unwrap();
        assert_eq!(
            output.ports["default"]["timestamp"].as_str().unwrap(),
            "1970-01-01T00:00:00Z"
        );
    }

    #[tokio::test]
    async fn test_add() {
        let mut node = TimestampNode::new();
        node.init(&serde_json::json!({"operation": "add", "amount": 3600}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let input = serde_json::json!({"unix": 0});
        let output = node.execute(input, &ctx).await.unwrap();
        assert_eq!(output.ports["default"]["unix"], 3600);
        assert_eq!(
            output.ports["default"]["timestamp"].as_str().unwrap(),
            "1970-01-01T01:00:00Z"
        );
    }

    #[tokio::test]
    async fn test_diff() {
        let mut node = TimestampNode::new();
        node.init(&serde_json::json!({"operation": "diff"}))
            .await
            .unwrap();
        let ctx = ExecutionContext::new();
        let input = serde_json::json!({"start": 1000, "end": 5000});
        let output = node.execute(input, &ctx).await.unwrap();
        assert_eq!(output.ports["default"]["difference"], 4000);
    }

    #[test]
    fn test_format_timestamp_epoch() {
        assert_eq!(format_timestamp(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn test_format_timestamp_known_date() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        assert_eq!(format_timestamp(1704067200), "2024-01-01T00:00:00Z");
    }

    #[tokio::test]
    async fn test_node_type_and_kind() {
        let node = TimestampNode::new();
        assert_eq!(node.node_type(), "timestamp");
        assert_eq!(node.node_kind(), NodeKind::Processor);
    }
}
