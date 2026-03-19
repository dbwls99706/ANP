use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Redis-compatible in-memory data structure store.
/// Supports strings, hashes, lists, sets, sorted sets, and TTL.
/// Implements Redis-like commands: GET, SET, DEL, HGET, HSET, LPUSH, RPUSH,
/// LPOP, RPOP, SADD, SMEMBERS, ZADD, ZRANGE, EXPIRE, TTL, KEYS, etc.
pub struct RedisClientNode {
    operation: String,
    prefix: String,
    store: Arc<RwLock<RedisStore>>,
}

#[derive(Default)]
struct RedisStore {
    strings: HashMap<String, String>,
    hashes: HashMap<String, HashMap<String, String>>,
    lists: HashMap<String, Vec<String>>,
    sets: HashMap<String, Vec<String>>,
    sorted_sets: HashMap<String, Vec<(f64, String)>>,
    expires: HashMap<String, u64>,
}

impl RedisStore {
    fn is_expired(&self, key: &str) -> bool {
        if let Some(&exp) = self.expires.get(key) {
            let now = current_timestamp();
            now >= exp
        } else {
            false
        }
    }

    fn cleanup_expired(&mut self) {
        let now = current_timestamp();
        let expired_keys: Vec<String> = self
            .expires
            .iter()
            .filter(|(_, &exp)| now >= exp)
            .map(|(k, _)| k.clone())
            .collect();
        for key in expired_keys {
            self.strings.remove(&key);
            self.hashes.remove(&key);
            self.lists.remove(&key);
            self.sets.remove(&key);
            self.sorted_sets.remove(&key);
            self.expires.remove(&key);
        }
    }
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl RedisClientNode {
    pub fn new() -> Self {
        Self {
            operation: "get".to_string(),
            prefix: String::new(),
            store: Arc::new(RwLock::new(RedisStore::default())),
        }
    }

    fn prefixed_key(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            key.to_string()
        } else {
            format!("{}:{}", self.prefix, key)
        }
    }
}

impl Default for RedisClientNode {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NodeExecutor for RedisClientNode {
    fn node_type(&self) -> &str {
        "redis"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            self.operation = op.to_string();
        }
        if let Some(prefix) = config.get("prefix").and_then(|v| v.as_str()) {
            self.prefix = prefix.to_string();
        }
        // Pre-populate data
        if let Some(data) = config.get("initial_data").and_then(|v| v.as_object()) {
            let mut store = self.store.write().await;
            for (k, v) in data {
                if let Some(s) = v.as_str() {
                    store.strings.insert(k.clone(), s.to_string());
                } else {
                    store.strings.insert(k.clone(), v.to_string());
                }
            }
        }
        Ok(())
    }

    async fn execute(&self, input: Value, _ctx: &ExecutionContext) -> NodeResult<NodeOutput> {
        let op = input
            .get("command")
            .or_else(|| input.get("operation"))
            .and_then(|v| v.as_str())
            .unwrap_or(&self.operation);

        let raw_key = input.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let key = self.prefixed_key(raw_key);

        let mut store = self.store.write().await;
        store.cleanup_expired();

        match op.to_uppercase().as_str() {
            // String commands
            "GET" => {
                if store.is_expired(&key) {
                    store.strings.remove(&key);
                    store.expires.remove(&key);
                    Ok(NodeOutput::data(json!({"value": null, "exists": false})))
                } else {
                    let value = store.strings.get(&key).cloned();
                    Ok(NodeOutput::data(json!({
                        "value": value,
                        "exists": value.is_some(),
                    })))
                }
            }

            "SET" => {
                let value = input
                    .get("value")
                    .map(|v| {
                        v.as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| v.to_string())
                    })
                    .unwrap_or_default();

                store.strings.insert(key.clone(), value);

                // Handle EX (expire in seconds)
                if let Some(ex) = input.get("ex").and_then(|v| v.as_u64()) {
                    let exp = current_timestamp() + ex;
                    store.expires.insert(key.clone(), exp);
                }
                // Handle PX (expire in milliseconds)
                if let Some(px) = input.get("px").and_then(|v| v.as_u64()) {
                    let exp = current_timestamp() + (px / 1000);
                    store.expires.insert(key.clone(), exp);
                }

                Ok(NodeOutput::data(json!({"ok": true})))
            }

            "DEL" => {
                let mut deleted = 0u64;
                if store.strings.remove(&key).is_some() {
                    deleted += 1;
                }
                if store.hashes.remove(&key).is_some() {
                    deleted += 1;
                }
                if store.lists.remove(&key).is_some() {
                    deleted += 1;
                }
                if store.sets.remove(&key).is_some() {
                    deleted += 1;
                }
                if store.sorted_sets.remove(&key).is_some() {
                    deleted += 1;
                }
                store.expires.remove(&key);
                Ok(NodeOutput::data(json!({"deleted": deleted})))
            }

            "EXISTS" => {
                let exists = store.strings.contains_key(&key)
                    || store.hashes.contains_key(&key)
                    || store.lists.contains_key(&key)
                    || store.sets.contains_key(&key)
                    || store.sorted_sets.contains_key(&key);
                Ok(NodeOutput::data(json!({"exists": exists})))
            }

            "INCR" => {
                let current = store
                    .strings
                    .get(&key)
                    .and_then(|v| v.parse::<i64>().ok())
                    .unwrap_or(0);
                let new_val = current + 1;
                store.strings.insert(key, new_val.to_string());
                Ok(NodeOutput::data(json!({"value": new_val})))
            }

            "DECR" => {
                let current = store
                    .strings
                    .get(&key)
                    .and_then(|v| v.parse::<i64>().ok())
                    .unwrap_or(0);
                let new_val = current - 1;
                store.strings.insert(key, new_val.to_string());
                Ok(NodeOutput::data(json!({"value": new_val})))
            }

            "INCRBY" => {
                let by = input.get("value").and_then(|v| v.as_i64()).unwrap_or(1);
                let current = store
                    .strings
                    .get(&key)
                    .and_then(|v| v.parse::<i64>().ok())
                    .unwrap_or(0);
                let new_val = current + by;
                store.strings.insert(key, new_val.to_string());
                Ok(NodeOutput::data(json!({"value": new_val})))
            }

            "MGET" => {
                let keys = input
                    .get("keys")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let values: Vec<Value> = keys
                    .iter()
                    .filter_map(|k| k.as_str())
                    .map(|k| {
                        let pk = self.prefixed_key(k);
                        store
                            .strings
                            .get(&pk)
                            .map(|v| json!(v))
                            .unwrap_or(Value::Null)
                    })
                    .collect();
                Ok(NodeOutput::data(json!({"values": values})))
            }

            "MSET" => {
                if let Some(pairs) = input.get("pairs").and_then(|v| v.as_object()) {
                    for (k, v) in pairs {
                        let pk = self.prefixed_key(k);
                        let val = v
                            .as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| v.to_string());
                        store.strings.insert(pk, val);
                    }
                }
                Ok(NodeOutput::data(json!({"ok": true})))
            }

            // Hash commands
            "HSET" => {
                let field = input.get("field").and_then(|v| v.as_str()).unwrap_or("");
                let value = input
                    .get("value")
                    .map(|v| {
                        v.as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| v.to_string())
                    })
                    .unwrap_or_default();
                let hash = store.hashes.entry(key).or_default();
                let is_new = !hash.contains_key(field);
                hash.insert(field.to_string(), value);
                Ok(NodeOutput::data(json!({"created": is_new})))
            }

            "HGET" => {
                let field = input.get("field").and_then(|v| v.as_str()).unwrap_or("");
                let value = store.hashes.get(&key).and_then(|h| h.get(field)).cloned();
                Ok(NodeOutput::data(json!({"value": value})))
            }

            "HGETALL" => {
                let hash = store.hashes.get(&key).cloned().unwrap_or_default();
                Ok(NodeOutput::data(json!({"fields": hash})))
            }

            "HDEL" => {
                let field = input.get("field").and_then(|v| v.as_str()).unwrap_or("");
                let deleted = store
                    .hashes
                    .get_mut(&key)
                    .map(|h| h.remove(field).is_some())
                    .unwrap_or(false);
                Ok(NodeOutput::data(json!({"deleted": deleted})))
            }

            "HMSET" => {
                if let Some(fields) = input.get("fields").and_then(|v| v.as_object()) {
                    let hash = store.hashes.entry(key).or_default();
                    for (k, v) in fields {
                        let val = v
                            .as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| v.to_string());
                        hash.insert(k.clone(), val);
                    }
                }
                Ok(NodeOutput::data(json!({"ok": true})))
            }

            // List commands
            "LPUSH" => {
                let value = input
                    .get("value")
                    .map(|v| {
                        v.as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| v.to_string())
                    })
                    .unwrap_or_default();
                let list = store.lists.entry(key).or_default();
                list.insert(0, value);
                Ok(NodeOutput::data(json!({"length": list.len()})))
            }

            "RPUSH" => {
                let value = input
                    .get("value")
                    .map(|v| {
                        v.as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| v.to_string())
                    })
                    .unwrap_or_default();
                let list = store.lists.entry(key).or_default();
                list.push(value);
                Ok(NodeOutput::data(json!({"length": list.len()})))
            }

            "LPOP" => {
                let value = store.lists.get_mut(&key).and_then(|list| {
                    if list.is_empty() {
                        None
                    } else {
                        Some(list.remove(0))
                    }
                });
                Ok(NodeOutput::data(json!({"value": value})))
            }

            "RPOP" => {
                let value = store.lists.get_mut(&key).and_then(|list| list.pop());
                Ok(NodeOutput::data(json!({"value": value})))
            }

            "LRANGE" => {
                let start = input.get("start").and_then(|v| v.as_i64()).unwrap_or(0);
                let stop = input.get("stop").and_then(|v| v.as_i64()).unwrap_or(-1);
                let list = store.lists.get(&key).cloned().unwrap_or_default();
                let len = list.len() as i64;
                let s = if start < 0 {
                    (len + start).max(0) as usize
                } else {
                    start as usize
                };
                let e = if stop < 0 {
                    (len + stop + 1).max(0) as usize
                } else {
                    (stop + 1).min(len) as usize
                };
                let range: Vec<&str> = if s < e {
                    list[s..e].iter().map(|s| s.as_str()).collect()
                } else {
                    Vec::new()
                };
                Ok(NodeOutput::data(
                    json!({"values": range, "length": list.len()}),
                ))
            }

            "LLEN" => {
                let len = store.lists.get(&key).map(|l| l.len()).unwrap_or(0);
                Ok(NodeOutput::data(json!({"length": len})))
            }

            // Set commands
            "SADD" => {
                let value = input
                    .get("value")
                    .map(|v| {
                        v.as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| v.to_string())
                    })
                    .unwrap_or_default();
                let set = store.sets.entry(key).or_default();
                let added = if set.contains(&value) {
                    false
                } else {
                    set.push(value);
                    true
                };
                Ok(NodeOutput::data(json!({"added": added, "size": set.len()})))
            }

            "SREM" => {
                let value = input
                    .get("value")
                    .map(|v| {
                        v.as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| v.to_string())
                    })
                    .unwrap_or_default();
                let removed = store
                    .sets
                    .get_mut(&key)
                    .map(|set| {
                        if let Some(pos) = set.iter().position(|x| x == &value) {
                            set.remove(pos);
                            true
                        } else {
                            false
                        }
                    })
                    .unwrap_or(false);
                Ok(NodeOutput::data(json!({"removed": removed})))
            }

            "SMEMBERS" => {
                let members = store.sets.get(&key).cloned().unwrap_or_default();
                Ok(NodeOutput::data(
                    json!({"members": members, "size": members.len()}),
                ))
            }

            "SISMEMBER" => {
                let value = input
                    .get("value")
                    .map(|v| {
                        v.as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| v.to_string())
                    })
                    .unwrap_or_default();
                let is_member = store
                    .sets
                    .get(&key)
                    .map(|set| set.contains(&value))
                    .unwrap_or(false);
                Ok(NodeOutput::data(json!({"is_member": is_member})))
            }

            "SCARD" => {
                let size = store.sets.get(&key).map(|s| s.len()).unwrap_or(0);
                Ok(NodeOutput::data(json!({"size": size})))
            }

            // Sorted Set commands
            "ZADD" => {
                let score = input.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let member = input
                    .get("member")
                    .or_else(|| input.get("value"))
                    .map(|v| {
                        v.as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| v.to_string())
                    })
                    .unwrap_or_default();
                let zset = store.sorted_sets.entry(key).or_default();
                zset.retain(|(_, m)| m != &member);
                zset.push((score, member));
                zset.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                Ok(NodeOutput::data(json!({"size": zset.len()})))
            }

            "ZRANGE" => {
                let start = input.get("start").and_then(|v| v.as_i64()).unwrap_or(0) as usize;
                let stop = input.get("stop").and_then(|v| v.as_i64()).unwrap_or(-1);
                let zset = store.sorted_sets.get(&key).cloned().unwrap_or_default();
                let len = zset.len();
                let e = if stop < 0 {
                    len
                } else {
                    (stop as usize + 1).min(len)
                };
                let range: Vec<Value> = if start < e {
                    zset[start..e]
                        .iter()
                        .map(|(score, member)| json!({"member": member, "score": score}))
                        .collect()
                } else {
                    Vec::new()
                };
                Ok(NodeOutput::data(json!({"members": range})))
            }

            "ZSCORE" => {
                let member = input.get("member").and_then(|v| v.as_str()).unwrap_or("");
                let score = store
                    .sorted_sets
                    .get(&key)
                    .and_then(|zset| zset.iter().find(|(_, m)| m == member).map(|(s, _)| *s));
                Ok(NodeOutput::data(json!({"score": score})))
            }

            "ZCARD" => {
                let size = store.sorted_sets.get(&key).map(|z| z.len()).unwrap_or(0);
                Ok(NodeOutput::data(json!({"size": size})))
            }

            // Key commands
            "EXPIRE" => {
                let seconds = input.get("seconds").and_then(|v| v.as_u64()).unwrap_or(60);
                let exp = current_timestamp() + seconds;
                store.expires.insert(key, exp);
                Ok(NodeOutput::data(json!({"ok": true})))
            }

            "TTL" => {
                let ttl = store
                    .expires
                    .get(&key)
                    .map(|&exp| {
                        let now = current_timestamp();
                        if exp > now {
                            (exp - now) as i64
                        } else {
                            -2i64
                        }
                    })
                    .unwrap_or(-1);
                Ok(NodeOutput::data(json!({"ttl": ttl})))
            }

            "KEYS" => {
                let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("*");
                let all_keys: Vec<String> = store
                    .strings
                    .keys()
                    .chain(store.hashes.keys())
                    .chain(store.lists.keys())
                    .chain(store.sets.keys())
                    .chain(store.sorted_sets.keys())
                    .cloned()
                    .collect::<std::collections::HashSet<_>>()
                    .into_iter()
                    .filter(|k| {
                        if pattern == "*" {
                            true
                        } else {
                            k.contains(pattern.trim_matches('*'))
                        }
                    })
                    .collect();
                Ok(NodeOutput::data(
                    json!({"keys": all_keys, "count": all_keys.len()}),
                ))
            }

            "FLUSHALL" => {
                store.strings.clear();
                store.hashes.clear();
                store.lists.clear();
                store.sets.clear();
                store.sorted_sets.clear();
                store.expires.clear();
                Ok(NodeOutput::data(json!({"ok": true})))
            }

            "DBSIZE" => {
                let size = store.strings.len()
                    + store.hashes.len()
                    + store.lists.len()
                    + store.sets.len()
                    + store.sorted_sets.len();
                Ok(NodeOutput::data(json!({"size": size})))
            }

            "TYPE" => {
                let type_name = if store.strings.contains_key(&key) {
                    "string"
                } else if store.hashes.contains_key(&key) {
                    "hash"
                } else if store.lists.contains_key(&key) {
                    "list"
                } else if store.sets.contains_key(&key) {
                    "set"
                } else if store.sorted_sets.contains_key(&key) {
                    "zset"
                } else {
                    "none"
                };
                Ok(NodeOutput::data(json!({"type": type_name})))
            }

            other => Err(NodeError::ExecutionFailed(format!(
                "unknown Redis command: {}",
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
    async fn test_string_ops() {
        let mut node = RedisClientNode::new();
        node.init(&json!({})).await.unwrap();

        // SET
        let output = node
            .execute(
                json!({"command": "SET", "key": "name", "value": "Alice"}),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["ok"], true);

        // GET
        let output = node
            .execute(json!({"command": "GET", "key": "name"}), &ctx())
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["value"], "Alice");

        // INCR
        node.execute(
            json!({"command": "SET", "key": "counter", "value": "10"}),
            &ctx(),
        )
        .await
        .unwrap();
        let output = node
            .execute(json!({"command": "INCR", "key": "counter"}), &ctx())
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["value"], 11);
    }

    #[tokio::test]
    async fn test_hash_ops() {
        let mut node = RedisClientNode::new();
        node.init(&json!({})).await.unwrap();

        node.execute(
            json!({"command": "HSET", "key": "user:1", "field": "name", "value": "Bob"}),
            &ctx(),
        )
        .await
        .unwrap();

        let output = node
            .execute(
                json!({"command": "HGET", "key": "user:1", "field": "name"}),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["value"], "Bob");

        let output = node
            .execute(json!({"command": "HGETALL", "key": "user:1"}), &ctx())
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["fields"]["name"], "Bob");
    }

    #[tokio::test]
    async fn test_list_ops() {
        let mut node = RedisClientNode::new();
        node.init(&json!({})).await.unwrap();

        node.execute(
            json!({"command": "RPUSH", "key": "q", "value": "a"}),
            &ctx(),
        )
        .await
        .unwrap();
        node.execute(
            json!({"command": "RPUSH", "key": "q", "value": "b"}),
            &ctx(),
        )
        .await
        .unwrap();
        node.execute(
            json!({"command": "LPUSH", "key": "q", "value": "z"}),
            &ctx(),
        )
        .await
        .unwrap();

        let output = node
            .execute(
                json!({"command": "LRANGE", "key": "q", "start": 0, "stop": -1}),
                &ctx(),
            )
            .await
            .unwrap();
        let values = output.ports["default"]["values"].as_array().unwrap();
        assert_eq!(values, &[json!("z"), json!("a"), json!("b")]);

        let output = node
            .execute(json!({"command": "LPOP", "key": "q"}), &ctx())
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["value"], "z");
    }

    #[tokio::test]
    async fn test_set_ops() {
        let mut node = RedisClientNode::new();
        node.init(&json!({})).await.unwrap();

        node.execute(
            json!({"command": "SADD", "key": "tags", "value": "rust"}),
            &ctx(),
        )
        .await
        .unwrap();
        node.execute(
            json!({"command": "SADD", "key": "tags", "value": "async"}),
            &ctx(),
        )
        .await
        .unwrap();
        node.execute(
            json!({"command": "SADD", "key": "tags", "value": "rust"}),
            &ctx(),
        )
        .await
        .unwrap(); // duplicate

        let output = node
            .execute(json!({"command": "SCARD", "key": "tags"}), &ctx())
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["size"], 2);
    }

    #[tokio::test]
    async fn test_sorted_set() {
        let mut node = RedisClientNode::new();
        node.init(&json!({})).await.unwrap();

        node.execute(
            json!({"command": "ZADD", "key": "scores", "member": "alice", "score": 100}),
            &ctx(),
        )
        .await
        .unwrap();
        node.execute(
            json!({"command": "ZADD", "key": "scores", "member": "bob", "score": 85}),
            &ctx(),
        )
        .await
        .unwrap();

        let output = node
            .execute(
                json!({"command": "ZRANGE", "key": "scores", "start": 0, "stop": -1}),
                &ctx(),
            )
            .await
            .unwrap();
        let members = output.ports["default"]["members"].as_array().unwrap();
        assert_eq!(members[0]["member"], "bob"); // lower score first
        assert_eq!(members[1]["member"], "alice");
    }

    #[tokio::test]
    async fn test_prefix() {
        let mut node = RedisClientNode::new();
        node.init(&json!({"prefix": "app"})).await.unwrap();

        node.execute(
            json!({"command": "SET", "key": "name", "value": "test"}),
            &ctx(),
        )
        .await
        .unwrap();

        let output = node
            .execute(json!({"command": "KEYS", "pattern": "*"}), &ctx())
            .await
            .unwrap();
        let keys = output.ports["default"]["keys"].as_array().unwrap();
        assert!(keys.iter().any(|k| k.as_str().unwrap().starts_with("app:")));
    }

    #[tokio::test]
    async fn test_del_exists() {
        let mut node = RedisClientNode::new();
        node.init(&json!({})).await.unwrap();

        node.execute(
            json!({"command": "SET", "key": "tmp", "value": "val"}),
            &ctx(),
        )
        .await
        .unwrap();

        let output = node
            .execute(json!({"command": "EXISTS", "key": "tmp"}), &ctx())
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["exists"], true);

        node.execute(json!({"command": "DEL", "key": "tmp"}), &ctx())
            .await
            .unwrap();

        let output = node
            .execute(json!({"command": "EXISTS", "key": "tmp"}), &ctx())
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["exists"], false);
    }
}
