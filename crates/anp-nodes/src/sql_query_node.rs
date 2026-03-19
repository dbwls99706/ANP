use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// SQL query engine for JSON data.
/// Provides SELECT, INSERT, UPDATE, DELETE over in-memory JSON tables.
/// Supports WHERE, ORDER BY, LIMIT, OFFSET, GROUP BY, JOINs, and aggregates.
pub struct SqlQueryNode {
    operation: String,
    tables: Arc<RwLock<HashMap<String, Vec<Value>>>>,
}

impl SqlQueryNode {
    pub fn new() -> Self {
        Self {
            operation: "query".to_string(),
            tables: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for SqlQueryNode {
    fn default() -> Self {
        Self::new()
    }
}

fn get_field(obj: &Value, path: &str) -> Value {
    let path = path.trim();
    // Handle table.field notation
    let field = if let Some(dot_pos) = path.find('.') {
        &path[dot_pos + 1..]
    } else {
        path
    };
    if let Some(v) = obj.get(field) {
        v.clone()
    } else {
        Value::Null
    }
}

fn compare_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (a, b) {
        (Value::Number(na), Value::Number(nb)) => {
            let fa = na.as_f64().unwrap_or(0.0);
            let fb = nb.as_f64().unwrap_or(0.0);
            fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal)
        }
        (Value::String(sa), Value::String(sb)) => sa.cmp(sb),
        (Value::Bool(ba), Value::Bool(bb)) => ba.cmp(bb),
        (Value::Null, Value::Null) => std::cmp::Ordering::Equal,
        (Value::Null, _) => std::cmp::Ordering::Less,
        (_, Value::Null) => std::cmp::Ordering::Greater,
        _ => {
            let sa = a.to_string();
            let sb = b.to_string();
            sa.cmp(&sb)
        }
    }
}

fn evaluate_condition(row: &Value, condition: &Value) -> bool {
    if let Some(and_conditions) = condition.get("AND").and_then(|v| v.as_array()) {
        return and_conditions.iter().all(|c| evaluate_condition(row, c));
    }
    if let Some(or_conditions) = condition.get("OR").and_then(|v| v.as_array()) {
        return or_conditions.iter().any(|c| evaluate_condition(row, c));
    }

    let field = condition
        .get("field")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let op = condition
        .get("op")
        .and_then(|v| v.as_str())
        .unwrap_or("eq");
    let value = condition.get("value").cloned().unwrap_or(Value::Null);

    let field_value = get_field(row, field);

    match op {
        "eq" | "=" => field_value == value,
        "neq" | "!=" | "<>" => field_value != value,
        "gt" | ">" => compare_values(&field_value, &value) == std::cmp::Ordering::Greater,
        "gte" | ">=" => {
            let cmp = compare_values(&field_value, &value);
            cmp == std::cmp::Ordering::Greater || cmp == std::cmp::Ordering::Equal
        }
        "lt" | "<" => compare_values(&field_value, &value) == std::cmp::Ordering::Less,
        "lte" | "<=" => {
            let cmp = compare_values(&field_value, &value);
            cmp == std::cmp::Ordering::Less || cmp == std::cmp::Ordering::Equal
        }
        "like" | "contains" => {
            let sv = field_value.as_str().unwrap_or("");
            let pattern = value.as_str().unwrap_or("");
            sv.to_lowercase().contains(&pattern.to_lowercase())
        }
        "starts_with" => {
            let sv = field_value.as_str().unwrap_or("");
            let pattern = value.as_str().unwrap_or("");
            sv.to_lowercase().starts_with(&pattern.to_lowercase())
        }
        "ends_with" => {
            let sv = field_value.as_str().unwrap_or("");
            let pattern = value.as_str().unwrap_or("");
            sv.to_lowercase().ends_with(&pattern.to_lowercase())
        }
        "in" => {
            if let Some(arr) = value.as_array() {
                arr.contains(&field_value)
            } else {
                false
            }
        }
        "not_in" => {
            if let Some(arr) = value.as_array() {
                !arr.contains(&field_value)
            } else {
                true
            }
        }
        "is_null" => field_value.is_null(),
        "is_not_null" => !field_value.is_null(),
        "between" => {
            if let Some(arr) = value.as_array() {
                if arr.len() == 2 {
                    compare_values(&field_value, &arr[0]) != std::cmp::Ordering::Less
                        && compare_values(&field_value, &arr[1]) != std::cmp::Ordering::Greater
                } else {
                    false
                }
            } else {
                false
            }
        }
        _ => false,
    }
}

fn apply_select(row: &Value, fields: &[Value]) -> Value {
    if fields.is_empty() || (fields.len() == 1 && fields[0].as_str() == Some("*")) {
        return row.clone();
    }

    let mut result = json!({});
    for field_spec in fields {
        if let Some(field_str) = field_spec.as_str() {
            let (source, alias) = if let Some(as_pos) = field_str.to_lowercase().find(" as ") {
                let source = field_str[..as_pos].trim();
                let alias = field_str[as_pos + 4..].trim();
                (source, alias)
            } else {
                let f = field_str.trim();
                // Strip table prefix for alias
                let alias = if let Some(dot_pos) = f.find('.') {
                    &f[dot_pos + 1..]
                } else {
                    f
                };
                (f, alias)
            };
            result[alias] = get_field(row, source);
        } else if let Some(obj) = field_spec.as_object() {
            if let (Some(expr), Some(alias)) = (
                obj.get("expr").and_then(|v| v.as_str()),
                obj.get("as").and_then(|v| v.as_str()),
            ) {
                result[alias] = get_field(row, expr);
            }
        }
    }
    result
}

fn aggregate_values(values: &[Value], func: &str) -> Value {
    match func.to_lowercase().as_str() {
        "count" => json!(values.len()),
        "sum" => {
            let sum: f64 = values
                .iter()
                .filter_map(|v| v.as_f64())
                .sum();
            json!(sum)
        }
        "avg" => {
            let nums: Vec<f64> = values.iter().filter_map(|v| v.as_f64()).collect();
            if nums.is_empty() {
                Value::Null
            } else {
                json!(nums.iter().sum::<f64>() / nums.len() as f64)
            }
        }
        "min" => values
            .iter()
            .filter(|v| !v.is_null())
            .min_by(|a, b| compare_values(a, b))
            .cloned()
            .unwrap_or(Value::Null),
        "max" => values
            .iter()
            .filter(|v| !v.is_null())
            .max_by(|a, b| compare_values(a, b))
            .cloned()
            .unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

#[async_trait]
impl NodeExecutor for SqlQueryNode {
    fn node_type(&self) -> &str {
        "sql_query"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "query" | "select" | "insert" | "update" | "delete" | "create_table"
                | "drop_table" | "describe" => {
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
        // Pre-populate tables
        if let Some(tables) = config.get("tables").and_then(|v| v.as_object()) {
            let mut store = self.tables.write().await;
            for (name, data) in tables {
                if let Some(arr) = data.as_array() {
                    store.insert(name.clone(), arr.clone());
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
            "query" | "select" => {
                let table_name = input
                    .get("from")
                    .or_else(|| input.get("table"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("from/table required".into()))?;

                let tables = self.tables.read().await;
                let rows = tables
                    .get(table_name)
                    .cloned()
                    .or_else(|| {
                        input.get("data").and_then(|v| v.as_array()).cloned()
                    })
                    .unwrap_or_default();

                // WHERE
                let mut result: Vec<Value> = if let Some(where_clause) = input.get("where") {
                    rows.into_iter()
                        .filter(|row| evaluate_condition(row, where_clause))
                        .collect()
                } else {
                    rows
                };

                // JOIN
                if let Some(join) = input.get("join") {
                    let join_table = join
                        .get("table")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let on_left = join.get("on_left").and_then(|v| v.as_str()).unwrap_or("");
                    let on_right = join.get("on_right").and_then(|v| v.as_str()).unwrap_or("");
                    let join_type = join.get("type").and_then(|v| v.as_str()).unwrap_or("inner");

                    let right_rows = tables.get(join_table).cloned().unwrap_or_default();

                    let mut joined = Vec::new();
                    for left in &result {
                        let left_val = get_field(left, on_left);
                        let mut matched = false;
                        for right in &right_rows {
                            let right_val = get_field(right, on_right);
                            if left_val == right_val {
                                let mut merged = left.clone();
                                if let (Some(l), Some(r)) = (merged.as_object_mut(), right.as_object()) {
                                    for (k, v) in r {
                                        l.entry(k.clone()).or_insert_with(|| v.clone());
                                    }
                                }
                                joined.push(merged);
                                matched = true;
                            }
                        }
                        if !matched && join_type == "left" {
                            joined.push(left.clone());
                        }
                    }
                    result = joined;
                }

                // GROUP BY
                if let Some(group_by) = input.get("group_by").and_then(|v| v.as_str()) {
                    let mut groups: HashMap<String, Vec<Value>> = HashMap::new();
                    for row in &result {
                        let key = get_field(row, group_by).to_string();
                        groups.entry(key).or_default().push(row.clone());
                    }

                    let aggregates = input.get("aggregates").and_then(|v| v.as_array());
                    let mut grouped_result = Vec::new();
                    for (key_str, group_rows) in &groups {
                        let mut row = json!({});
                        // Parse key back
                        if let Ok(v) = serde_json::from_str::<Value>(key_str) {
                            row[group_by] = v;
                        } else {
                            row[group_by] = json!(key_str);
                        }
                        row["_count"] = json!(group_rows.len());

                        if let Some(aggs) = aggregates {
                            for agg in aggs {
                                let func = agg.get("func").and_then(|v| v.as_str()).unwrap_or("count");
                                let field = agg.get("field").and_then(|v| v.as_str()).unwrap_or("*");
                                let default_alias = format!("{}_{}", func, field);
                                let alias = agg
                                    .get("as")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or(&default_alias);

                                let values: Vec<Value> = group_rows
                                    .iter()
                                    .map(|r| get_field(r, field))
                                    .collect();
                                row[alias] = aggregate_values(&values, func);
                            }
                        }
                        grouped_result.push(row);
                    }
                    result = grouped_result;
                }

                // ORDER BY
                if let Some(order_by) = input.get("order_by").and_then(|v| v.as_str()) {
                    let desc = input
                        .get("order")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_lowercase() == "desc")
                        .unwrap_or(false);

                    result.sort_by(|a, b| {
                        let va = get_field(a, order_by);
                        let vb = get_field(b, order_by);
                        let cmp = compare_values(&va, &vb);
                        if desc {
                            cmp.reverse()
                        } else {
                            cmp
                        }
                    });
                }

                let total = result.len();

                // OFFSET
                if let Some(offset) = input.get("offset").and_then(|v| v.as_u64()) {
                    result = result.into_iter().skip(offset as usize).collect();
                }

                // LIMIT
                if let Some(limit) = input.get("limit").and_then(|v| v.as_u64()) {
                    result = result.into_iter().take(limit as usize).collect();
                }

                // SELECT fields
                if let Some(select) = input.get("select").and_then(|v| v.as_array()) {
                    result = result.iter().map(|row| apply_select(row, select)).collect();
                }

                let count = result.len();
                Ok(NodeOutput::data(json!({
                    "rows": result,
                    "count": count,
                    "total": total,
                })))
            }

            "insert" => {
                let table_name = input
                    .get("into")
                    .or_else(|| input.get("table"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("into/table required".into()))?;

                let mut tables = self.tables.write().await;
                let table = tables.entry(table_name.to_string()).or_default();

                let inserted = if let Some(rows) = input.get("values").and_then(|v| v.as_array()) {
                    let count = rows.len();
                    table.extend(rows.clone());
                    count
                } else if let Some(row) = input.get("value") {
                    table.push(row.clone());
                    1
                } else {
                    return Err(NodeError::MissingInput("values or value required".into()));
                };

                Ok(NodeOutput::data(json!({
                    "inserted": inserted,
                    "table": table_name,
                    "total_rows": table.len(),
                })))
            }

            "update" => {
                let table_name = input
                    .get("table")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("table required".into()))?;
                let set = input
                    .get("set")
                    .ok_or_else(|| NodeError::MissingInput("set required".into()))?;
                let where_clause = input.get("where");

                let mut tables = self.tables.write().await;
                let table = tables.entry(table_name.to_string()).or_default();

                let mut updated = 0;
                for row in table.iter_mut() {
                    let matches = where_clause
                        .map(|w| evaluate_condition(row, w))
                        .unwrap_or(true);
                    if matches {
                        if let (Some(row_obj), Some(set_obj)) =
                            (row.as_object_mut(), set.as_object())
                        {
                            for (k, v) in set_obj {
                                row_obj.insert(k.clone(), v.clone());
                            }
                            updated += 1;
                        }
                    }
                }

                Ok(NodeOutput::data(json!({
                    "updated": updated,
                    "table": table_name,
                })))
            }

            "delete" => {
                let table_name = input
                    .get("from")
                    .or_else(|| input.get("table"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("from/table required".into()))?;
                let where_clause = input.get("where");

                let mut tables = self.tables.write().await;
                let table = tables.entry(table_name.to_string()).or_default();

                let before = table.len();
                if let Some(w) = where_clause {
                    table.retain(|row| !evaluate_condition(row, w));
                } else {
                    table.clear();
                }
                let deleted = before - table.len();

                Ok(NodeOutput::data(json!({
                    "deleted": deleted,
                    "table": table_name,
                    "remaining": table.len(),
                })))
            }

            "create_table" => {
                let table_name = input
                    .get("table")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("table name required".into()))?;

                let mut tables = self.tables.write().await;
                let exists = tables.contains_key(table_name);
                if !exists {
                    tables.insert(table_name.to_string(), Vec::new());
                }

                Ok(NodeOutput::data(json!({
                    "table": table_name,
                    "created": !exists,
                    "already_existed": exists,
                })))
            }

            "drop_table" => {
                let table_name = input
                    .get("table")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| NodeError::MissingInput("table name required".into()))?;

                let mut tables = self.tables.write().await;
                let existed = tables.remove(table_name).is_some();

                Ok(NodeOutput::data(json!({
                    "table": table_name,
                    "dropped": existed,
                })))
            }

            "describe" => {
                let tables = self.tables.read().await;

                if let Some(table_name) = input.get("table").and_then(|v| v.as_str()) {
                    if let Some(rows) = tables.get(table_name) {
                        let mut columns: Vec<String> = Vec::new();
                        if let Some(first) = rows.first() {
                            if let Some(obj) = first.as_object() {
                                columns = obj.keys().cloned().collect();
                            }
                        }
                        Ok(NodeOutput::data(json!({
                            "table": table_name,
                            "row_count": rows.len(),
                            "columns": columns,
                        })))
                    } else {
                        Err(NodeError::ExecutionFailed(format!(
                            "table '{}' not found",
                            table_name
                        )))
                    }
                } else {
                    let table_list: Vec<Value> = tables
                        .iter()
                        .map(|(name, rows)| {
                            json!({
                                "table": name,
                                "row_count": rows.len(),
                            })
                        })
                        .collect();
                    Ok(NodeOutput::data(json!({
                        "tables": table_list,
                        "table_count": table_list.len(),
                    })))
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
    async fn test_create_insert_select() {
        let mut node = SqlQueryNode::new();
        node.init(&json!({})).await.unwrap();

        // Create table
        node.execute(
            json!({"operation": "create_table", "table": "users"}),
            &ctx(),
        )
        .await
        .unwrap();

        // Insert
        node.execute(
            json!({
                "operation": "insert",
                "into": "users",
                "values": [
                    {"id": 1, "name": "Alice", "age": 30},
                    {"id": 2, "name": "Bob", "age": 25},
                    {"id": 3, "name": "Carol", "age": 35}
                ]
            }),
            &ctx(),
        )
        .await
        .unwrap();

        // Select with WHERE
        let output = node
            .execute(
                json!({
                    "operation": "select",
                    "from": "users",
                    "where": {"field": "age", "op": "gte", "value": 30},
                    "order_by": "name"
                }),
                &ctx(),
            )
            .await
            .unwrap();

        let data = &output.ports["default"];
        assert_eq!(data["count"], 2);
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows[0]["name"], "Alice");
        assert_eq!(rows[1]["name"], "Carol");
    }

    #[tokio::test]
    async fn test_update_delete() {
        let mut node = SqlQueryNode::new();
        node.init(&json!({
            "tables": {
                "items": [
                    {"id": 1, "name": "A", "price": 10},
                    {"id": 2, "name": "B", "price": 20}
                ]
            }
        }))
        .await
        .unwrap();

        // Update
        let output = node
            .execute(
                json!({
                    "operation": "update",
                    "table": "items",
                    "set": {"price": 15},
                    "where": {"field": "id", "op": "eq", "value": 1}
                }),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["updated"], 1);

        // Delete
        let output = node
            .execute(
                json!({
                    "operation": "delete",
                    "from": "items",
                    "where": {"field": "id", "op": "eq", "value": 2}
                }),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["deleted"], 1);
    }

    #[tokio::test]
    async fn test_group_by() {
        let mut node = SqlQueryNode::new();
        node.init(&json!({
            "tables": {
                "sales": [
                    {"region": "US", "amount": 100},
                    {"region": "EU", "amount": 200},
                    {"region": "US", "amount": 150},
                    {"region": "EU", "amount": 50}
                ]
            }
        }))
        .await
        .unwrap();

        let output = node
            .execute(
                json!({
                    "operation": "select",
                    "from": "sales",
                    "group_by": "region",
                    "aggregates": [
                        {"func": "sum", "field": "amount", "as": "total"},
                        {"func": "count", "field": "*", "as": "count"}
                    ]
                }),
                &ctx(),
            )
            .await
            .unwrap();

        let rows = output.ports["default"]["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[tokio::test]
    async fn test_join() {
        let mut node = SqlQueryNode::new();
        node.init(&json!({
            "tables": {
                "orders": [
                    {"id": 1, "user_id": 1, "product": "Widget"},
                    {"id": 2, "user_id": 2, "product": "Gadget"}
                ],
                "users": [
                    {"id": 1, "name": "Alice"},
                    {"id": 2, "name": "Bob"}
                ]
            }
        }))
        .await
        .unwrap();

        let output = node
            .execute(
                json!({
                    "operation": "select",
                    "from": "orders",
                    "join": {
                        "table": "users",
                        "on_left": "user_id",
                        "on_right": "id",
                        "type": "inner"
                    },
                    "select": ["product", "name"]
                }),
                &ctx(),
            )
            .await
            .unwrap();

        let rows = output.ports["default"]["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["name"], "Alice");
        assert_eq!(rows[0]["product"], "Widget");
    }

    #[tokio::test]
    async fn test_limit_offset() {
        let mut node = SqlQueryNode::new();
        node.init(&json!({
            "tables": {
                "items": [
                    {"id": 1}, {"id": 2}, {"id": 3}, {"id": 4}, {"id": 5}
                ]
            }
        }))
        .await
        .unwrap();

        let output = node
            .execute(
                json!({
                    "operation": "select",
                    "from": "items",
                    "order_by": "id",
                    "offset": 1,
                    "limit": 2
                }),
                &ctx(),
            )
            .await
            .unwrap();

        let rows = output.ports["default"]["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["id"], 2);
        assert_eq!(rows[1]["id"], 3);
    }

    #[tokio::test]
    async fn test_describe() {
        let mut node = SqlQueryNode::new();
        node.init(&json!({
            "tables": {
                "t1": [{"a": 1, "b": 2}],
                "t2": []
            }
        }))
        .await
        .unwrap();

        let output = node
            .execute(json!({"operation": "describe"}), &ctx())
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["table_count"], 2);
    }

    #[tokio::test]
    async fn test_and_or_conditions() {
        let mut node = SqlQueryNode::new();
        node.init(&json!({
            "tables": {
                "items": [
                    {"name": "A", "price": 10, "qty": 5},
                    {"name": "B", "price": 20, "qty": 3},
                    {"name": "C", "price": 5, "qty": 10}
                ]
            }
        }))
        .await
        .unwrap();

        let output = node
            .execute(
                json!({
                    "operation": "select",
                    "from": "items",
                    "where": {
                        "AND": [
                            {"field": "price", "op": "gte", "value": 10},
                            {"field": "qty", "op": "gte", "value": 5}
                        ]
                    }
                }),
                &ctx(),
            )
            .await
            .unwrap();

        assert_eq!(output.ports["default"]["count"], 1);
        let rows = output.ports["default"]["rows"].as_array().unwrap();
        assert_eq!(rows[0]["name"], "A");
    }
}
