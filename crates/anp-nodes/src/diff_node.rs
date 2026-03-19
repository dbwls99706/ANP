use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// DiffNode — JSON diffing and patching
pub struct DiffNode {
    operation: String,
}

impl Default for DiffNode {
    fn default() -> Self {
        Self::new()
    }
}

impl DiffNode {
    pub fn new() -> Self {
        Self {
            operation: "diff".to_string(),
        }
    }
}

fn compute_diff(
    a: &serde_json::Value,
    b: &serde_json::Value,
    path: &str,
    changes: &mut Vec<serde_json::Value>,
) {
    if a == b {
        return;
    }

    match (a, b) {
        (serde_json::Value::Object(obj_a), serde_json::Value::Object(obj_b)) => {
            // Check for removed and changed keys
            for (key, val_a) in obj_a {
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", path, key)
                };
                match obj_b.get(key) {
                    Some(val_b) => {
                        compute_diff(val_a, val_b, &child_path, changes);
                    }
                    None => {
                        changes.push(serde_json::json!({
                            "path": child_path,
                            "type": "removed",
                            "old_value": val_a,
                        }));
                    }
                }
            }
            // Check for added keys
            for (key, val_b) in obj_b {
                if !obj_a.contains_key(key) {
                    let child_path = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{}.{}", path, key)
                    };
                    changes.push(serde_json::json!({
                        "path": child_path,
                        "type": "added",
                        "new_value": val_b,
                    }));
                }
            }
        }
        (serde_json::Value::Array(arr_a), serde_json::Value::Array(arr_b)) => {
            let max_len = arr_a.len().max(arr_b.len());
            for i in 0..max_len {
                let child_path = if path.is_empty() {
                    format!("{}", i)
                } else {
                    format!("{}.{}", path, i)
                };
                match (arr_a.get(i), arr_b.get(i)) {
                    (Some(va), Some(vb)) => {
                        compute_diff(va, vb, &child_path, changes);
                    }
                    (Some(va), None) => {
                        changes.push(serde_json::json!({
                            "path": child_path,
                            "type": "removed",
                            "old_value": va,
                        }));
                    }
                    (None, Some(vb)) => {
                        changes.push(serde_json::json!({
                            "path": child_path,
                            "type": "added",
                            "new_value": vb,
                        }));
                    }
                    (None, None) => {}
                }
            }
        }
        _ => {
            changes.push(serde_json::json!({
                "path": if path.is_empty() { "." } else { path },
                "type": "changed",
                "old_value": a,
                "new_value": b,
            }));
        }
    }
}

fn apply_patch(
    target: &mut serde_json::Value,
    changes: &[serde_json::Value],
) -> Result<(), String> {
    for change in changes {
        let path = change
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or("change missing 'path'")?;
        let change_type = change
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or("change missing 'type'")?;

        match change_type {
            "added" => {
                let new_value = change
                    .get("new_value")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                set_path(target, path, new_value)?;
            }
            "changed" => {
                let new_value = change
                    .get("new_value")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                set_path(target, path, new_value)?;
            }
            "removed" => {
                remove_path(target, path)?;
            }
            other => {
                return Err(format!("unknown change type: {}", other));
            }
        }
    }
    Ok(())
}

fn set_path(target: &mut serde_json::Value, path: &str, value: serde_json::Value) -> Result<(), String> {
    if path == "." {
        *target = value;
        return Ok(());
    }
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = target;

    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            // Last part — set the value
            if let Some(obj) = current.as_object_mut() {
                obj.insert(part.to_string(), value);
                return Ok(());
            } else if let Some(arr) = current.as_array_mut() {
                if let Ok(idx) = part.parse::<usize>() {
                    while arr.len() <= idx {
                        arr.push(serde_json::Value::Null);
                    }
                    arr[idx] = value;
                    return Ok(());
                }
            }
            return Err(format!("cannot set path '{}'", path));
        }

        // Navigate deeper
        if let Ok(idx) = part.parse::<usize>() {
            if let Some(arr) = current.as_array_mut() {
                if idx < arr.len() {
                    current = &mut arr[idx];
                    continue;
                }
            }
            return Err(format!("invalid array index at '{}'", part));
        }

        if current.get(part).is_none() {
            if let Some(obj) = current.as_object_mut() {
                obj.insert(part.to_string(), serde_json::json!({}));
            }
        }
        current = current
            .get_mut(part)
            .ok_or_else(|| format!("cannot navigate to '{}'", part))?;
    }

    Ok(())
}

fn remove_path(target: &mut serde_json::Value, path: &str) -> Result<(), String> {
    if path == "." {
        *target = serde_json::Value::Null;
        return Ok(());
    }
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = target;

    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            if let Some(obj) = current.as_object_mut() {
                obj.remove(*part);
                return Ok(());
            }
            return Err(format!("cannot remove path '{}'", path));
        }

        if let Ok(idx) = part.parse::<usize>() {
            if let Some(arr) = current.as_array_mut() {
                if idx < arr.len() {
                    current = &mut arr[idx];
                    continue;
                }
            }
            return Err(format!("invalid array index at '{}'", part));
        }

        current = current
            .get_mut(part)
            .ok_or_else(|| format!("cannot navigate to '{}'", part))?;
    }

    Ok(())
}

#[async_trait::async_trait]
impl NodeExecutor for DiffNode {
    fn node_type(&self) -> &str {
        "diff"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "diff" | "patch" | "equals" => self.operation = op.to_string(),
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown operation '{}'. Use: diff, patch, equals",
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
        let result = match self.operation.as_str() {
            "diff" => {
                let a = input
                    .get("a")
                    .ok_or_else(|| NodeError::ExecutionFailed("diff requires field 'a'".into()))?;
                let b = input
                    .get("b")
                    .ok_or_else(|| NodeError::ExecutionFailed("diff requires field 'b'".into()))?;

                let mut changes = Vec::new();
                compute_diff(a, b, "", &mut changes);
                let count = changes.len();

                serde_json::json!({
                    "changes": changes,
                    "change_count": count,
                    "has_changes": count > 0,
                })
            }
            "patch" => {
                let mut target = input
                    .get("target")
                    .cloned()
                    .ok_or_else(|| {
                        NodeError::ExecutionFailed("patch requires field 'target'".into())
                    })?;
                let changes = input
                    .get("changes")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| {
                        NodeError::ExecutionFailed(
                            "patch requires field 'changes' as array".into(),
                        )
                    })?;

                apply_patch(&mut target, changes).map_err(|e| {
                    NodeError::ExecutionFailed(format!("patch failed: {}", e))
                })?;

                target
            }
            "equals" => {
                let a = input
                    .get("a")
                    .ok_or_else(|| {
                        NodeError::ExecutionFailed("equals requires field 'a'".into())
                    })?;
                let b = input
                    .get("b")
                    .ok_or_else(|| {
                        NodeError::ExecutionFailed("equals requires field 'b'".into())
                    })?;

                let mut changes = Vec::new();
                compute_diff(a, b, "", &mut changes);
                let count = changes.len();

                serde_json::json!({
                    "equal": count == 0,
                    "differences": count,
                })
            }
            _ => {
                return Err(NodeError::ExecutionFailed(format!(
                    "unknown operation: {}",
                    self.operation
                )));
            }
        };

        Ok(NodeOutput::data(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> ExecutionContext {
        ExecutionContext::new()
    }

    #[tokio::test]
    async fn test_diff_added_field() {
        let mut node = DiffNode::new();
        node.init(&json!({"operation": "diff"})).await.unwrap();
        let input = json!({"a": {"name": "John"}, "b": {"name": "John", "age": 30}});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["change_count"], 1);
        assert_eq!(data["has_changes"], true);
        assert_eq!(data["changes"][0]["type"], "added");
        assert_eq!(data["changes"][0]["path"], "age");
    }

    #[tokio::test]
    async fn test_diff_removed_field() {
        let mut node = DiffNode::new();
        node.init(&json!({"operation": "diff"})).await.unwrap();
        let input = json!({"a": {"name": "John", "age": 30}, "b": {"name": "John"}});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["change_count"], 1);
        assert_eq!(data["changes"][0]["type"], "removed");
    }

    #[tokio::test]
    async fn test_diff_changed_field() {
        let mut node = DiffNode::new();
        node.init(&json!({"operation": "diff"})).await.unwrap();
        let input = json!({"a": {"name": "John"}, "b": {"name": "Jane"}});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["change_count"], 1);
        assert_eq!(data["changes"][0]["type"], "changed");
        assert_eq!(data["changes"][0]["old_value"], "John");
        assert_eq!(data["changes"][0]["new_value"], "Jane");
    }

    #[tokio::test]
    async fn test_equals_true() {
        let mut node = DiffNode::new();
        node.init(&json!({"operation": "equals"})).await.unwrap();
        let input = json!({"a": {"x": 1, "y": 2}, "b": {"x": 1, "y": 2}});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["equal"], true);
        assert_eq!(data["differences"], 0);
    }

    #[tokio::test]
    async fn test_equals_false() {
        let mut node = DiffNode::new();
        node.init(&json!({"operation": "equals"})).await.unwrap();
        let input = json!({"a": {"x": 1}, "b": {"x": 2}});
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["equal"], false);
    }

    #[tokio::test]
    async fn test_patch_apply() {
        let mut node = DiffNode::new();
        node.init(&json!({"operation": "patch"})).await.unwrap();
        let input = json!({
            "target": {"name": "John", "age": 30},
            "changes": [
                {"path": "age", "type": "changed", "new_value": 31},
                {"path": "email", "type": "added", "new_value": "john@example.com"}
            ]
        });
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["name"], "John");
        assert_eq!(data["age"], 31);
        assert_eq!(data["email"], "john@example.com");
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let mut node = DiffNode::new();
        let result = node.init(&json!({"operation": "invalid"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_diff_nested_objects() {
        let mut node = DiffNode::new();
        node.init(&json!({"operation": "diff"})).await.unwrap();
        let input = json!({
            "a": {"user": {"name": "John", "address": {"city": "NYC"}}},
            "b": {"user": {"name": "John", "address": {"city": "LA"}}}
        });
        let output = node.execute(input, &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert_eq!(data["change_count"], 1);
        assert_eq!(data["changes"][0]["path"], "user.address.city");
    }
}
