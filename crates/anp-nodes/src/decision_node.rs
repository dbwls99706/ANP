use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use std::time::Duration;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// Decision node — a typed, probabilistic branch.
///
/// The node answers one question over a closed set of choices. It never emits
/// free text: the result is a probability distribution over the declared
/// choices, and the input is routed to the port named after the winning choice.
/// When the winning probability is below `threshold`, the input is routed to
/// the `uncertain_port` instead, so low-confidence cases are always explicit.
///
/// Config example:
/// ```json
/// {
///   "question": "What does the customer want?",
///   "choices": [
///     { "name": "refund", "description": "Wants money back" },
///     { "name": "support", "description": "Needs help using the product" }
///   ],
///   "threshold": 0.7,
///   "provider": "openai"
/// }
/// ```
///
/// Providers:
/// - `openai`: reads token log-probabilities of a single-letter answer
/// - `anthropic`: asks the model for a score per choice
/// - `rules`: deterministic keyword counting (`keywords` config), no network
/// - `input`: reads scores produced upstream from `scores_field`
///
/// Malformed model output can never leave the declared output space: it
/// degrades to a uniform distribution, which routes to the uncertain port.
///
/// Output ports: one per choice name + the uncertain port. The payload is the
/// input with a `_decision` object attached (non-object inputs are wrapped as
/// `{ "value": ..., "_decision": ... }`).
pub struct DecisionNode {
    question: String,
    choices: Vec<Choice>,
    threshold: f64,
    uncertain_port: String,
    field: Option<String>,
    provider: Provider,
    model: String,
    scores_field: String,
    timeout_secs: u64,
}

#[derive(Debug, Clone)]
struct Choice {
    name: String,
    description: String,
    keywords: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Provider {
    Rules,
    Input,
    OpenAI,
    Anthropic,
}

impl Provider {
    fn as_str(self) -> &'static str {
        match self {
            Provider::Rules => "rules",
            Provider::Input => "input",
            Provider::OpenAI => "openai",
            Provider::Anthropic => "anthropic",
        }
    }
}

/// OpenAI returns at most 20 alternatives per token position.
const MAX_LOGPROB_CHOICES: usize = 20;
const MAX_CHOICES: usize = 255;

impl Default for DecisionNode {
    fn default() -> Self {
        Self::new()
    }
}

impl DecisionNode {
    pub fn new() -> Self {
        Self {
            question: String::new(),
            choices: Vec::new(),
            threshold: 0.7,
            uncertain_port: "uncertain".to_string(),
            field: None,
            provider: Provider::Rules,
            model: String::new(),
            scores_field: "scores".to_string(),
            timeout_secs: 30,
        }
    }

    /// The text the decision is made about.
    fn state_text(&self, input: &serde_json::Value) -> String {
        let target = match &self.field {
            Some(field) => resolve_path(input, field).unwrap_or(&serde_json::Value::Null),
            None => input,
        };
        match target {
            serde_json::Value::String(s) => s.clone(),
            other => serde_json::to_string(other).unwrap_or_default(),
        }
    }

    fn rule_scores(&self, text: &str) -> Vec<f64> {
        let lower = text.to_lowercase();
        self.choices
            .iter()
            .map(|c| {
                c.keywords
                    .iter()
                    .map(|kw| lower.matches(kw.as_str()).count())
                    .sum::<usize>() as f64
            })
            .collect()
    }

    fn input_scores(&self, input: &serde_json::Value) -> Vec<f64> {
        let scores = resolve_path(input, &self.scores_field);
        self.choices
            .iter()
            .map(|c| {
                scores
                    .and_then(|s| s.get(&c.name))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0)
            })
            .collect()
    }

    fn options_block(&self, labelled: bool) -> String {
        self.choices
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let key = if labelled {
                    letter(i).to_string()
                } else {
                    c.name.clone()
                };
                if c.description.is_empty() || labelled && c.description == c.name {
                    format!("{}: {}", key, c.name)
                } else if labelled {
                    format!("{}: {} — {}", key, c.name, c.description)
                } else {
                    format!("{}: {}", key, c.description)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    async fn openai_scores(&self, text: &str) -> Result<Vec<f64>, NodeError> {
        let api_key = env_key("OPENAI_API_KEY")?;
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {
                    "role": "system",
                    "content": "You are a decision function. Answer with the single letter of the best option and nothing else."
                },
                {
                    "role": "user",
                    "content": format!(
                        "Question: {}\n\nOptions:\n{}\n\nInput:\n{}",
                        self.question,
                        self.options_block(true),
                        text
                    )
                }
            ],
            "temperature": 0.0,
            "max_tokens": 1,
            "logprobs": true,
            "top_logprobs": MAX_LOGPROB_CHOICES
        });

        let resp = post_json(
            "https://api.openai.com/v1/chat/completions",
            &[("Authorization", format!("Bearer {}", api_key))],
            &body,
            self.timeout_secs,
            "OpenAI",
        )
        .await?;

        Ok(scores_from_logprobs(&resp, self.choices.len()))
    }

    async fn anthropic_scores(&self, text: &str) -> Result<Vec<f64>, NodeError> {
        let api_key = env_key("ANTHROPIC_API_KEY")?;
        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 1024,
            "system": "You are a decision function. Reply with a single JSON object that maps every option name to its probability between 0 and 1. The probabilities must sum to 1. Output the JSON object only.",
            "messages": [{
                "role": "user",
                "content": format!(
                    "Question: {}\n\nOptions:\n{}\n\nInput:\n{}",
                    self.question,
                    self.options_block(false),
                    text
                )
            }]
        });

        let resp = post_json(
            "https://api.anthropic.com/v1/messages",
            &[
                ("x-api-key", api_key),
                ("anthropic-version", "2023-06-01".to_string()),
            ],
            &body,
            self.timeout_secs,
            "Anthropic",
        )
        .await?;

        let reply = resp
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|blocks| blocks.iter().find_map(|b| b.get("text")?.as_str()))
            .unwrap_or("");
        let names: Vec<&str> = self.choices.iter().map(|c| c.name.as_str()).collect();
        Ok(scores_from_json_reply(reply, &names))
    }
}

fn letter(index: usize) -> char {
    (b'A' + index as u8) as char
}

fn resolve_path<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut current = value;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

fn env_key(var: &str) -> Result<String, NodeError> {
    std::env::var(var)
        .ok()
        .filter(|k| !k.is_empty())
        .ok_or_else(|| NodeError::ExecutionFailed(format!("{} not set", var)))
}

fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

async fn post_json(
    url: &str,
    headers: &[(&str, String)],
    body: &serde_json::Value,
    timeout_secs: u64,
    api_name: &str,
) -> Result<serde_json::Value, NodeError> {
    let mut request = http_client()
        .post(url)
        .timeout(Duration::from_secs(timeout_secs))
        .json(body);
    for (name, value) in headers {
        request = request.header(*name, value);
    }

    let resp = request
        .send()
        .await
        .map_err(|e| NodeError::ExecutionFailed(format!("{} request failed: {}", api_name, e)))?;
    let status = resp.status();
    let parsed: serde_json::Value = resp.json().await.map_err(|e| {
        NodeError::ExecutionFailed(format!("{} response was not JSON: {}", api_name, e))
    })?;

    if !status.is_success() {
        let message = parsed["error"]["message"]
            .as_str()
            .unwrap_or("unknown API error");
        return Err(NodeError::ExecutionFailed(format!(
            "{} API error ({}): {}",
            api_name, status, message
        )));
    }
    Ok(parsed)
}

/// Turn raw non-negative scores into a probability distribution.
/// Invalid scores count as zero; an all-zero vector becomes uniform.
fn normalize(scores: &[f64]) -> Vec<f64> {
    let cleaned: Vec<f64> = scores
        .iter()
        .map(|s| if s.is_finite() && *s > 0.0 { *s } else { 0.0 })
        .collect();
    let total: f64 = cleaned.iter().sum();
    if total > 0.0 && total.is_finite() {
        cleaned.iter().map(|s| s / total).collect()
    } else {
        vec![1.0 / scores.len() as f64; scores.len()]
    }
}

/// Probability mass per choice letter from an OpenAI `logprobs` response.
fn scores_from_logprobs(resp: &serde_json::Value, choice_count: usize) -> Vec<f64> {
    let mut scores = vec![0.0; choice_count];
    let candidates = resp["choices"][0]["logprobs"]["content"][0]["top_logprobs"].as_array();
    for candidate in candidates.into_iter().flatten() {
        let token = candidate["token"].as_str().unwrap_or("").trim();
        let logprob = candidate["logprob"].as_f64();
        let mut chars = token.chars();
        if let (Some(c), None, Some(lp)) = (chars.next(), chars.next(), logprob) {
            let c = c.to_ascii_uppercase();
            if c.is_ascii_uppercase() {
                let index = (c as u8 - b'A') as usize;
                if index < choice_count {
                    scores[index] += lp.exp();
                }
            }
        }
    }
    scores
}

/// Scores per choice from a model reply that should contain one JSON object.
/// Unknown keys are ignored and missing choices score zero.
fn scores_from_json_reply(reply: &str, names: &[&str]) -> Vec<f64> {
    let parsed = match (reply.find('{'), reply.rfind('}')) {
        (Some(start), Some(end)) if start < end => {
            serde_json::from_str::<serde_json::Value>(&reply[start..=end]).ok()
        }
        _ => None,
    };
    names
        .iter()
        .map(|name| {
            parsed
                .as_ref()
                .and_then(|p| p.get(*name))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0)
        })
        .collect()
}

#[async_trait::async_trait]
impl NodeExecutor for DecisionNode {
    fn node_type(&self) -> &str {
        "decision"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &serde_json::Value) -> NodeResult<()> {
        self.question = config
            .get("question")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .ok_or_else(|| NodeError::InvalidConfig("missing 'question' in config".into()))?
            .to_string();

        if let Some(port) = config.get("uncertain_port").and_then(|v| v.as_str()) {
            if port.is_empty() {
                return Err(NodeError::InvalidConfig(
                    "'uncertain_port' must not be empty".into(),
                ));
            }
            self.uncertain_port = port.to_string();
        }

        let raw_choices = config
            .get("choices")
            .and_then(|v| v.as_array())
            .ok_or_else(|| NodeError::InvalidConfig("missing 'choices' array in config".into()))?;
        let keyword_map = config.get("keywords").and_then(|v| v.as_object());

        self.choices = Vec::with_capacity(raw_choices.len());
        let mut seen = HashSet::new();
        for (i, raw) in raw_choices.iter().enumerate() {
            let (name, description) = match raw {
                serde_json::Value::String(s) => (s.clone(), String::new()),
                other => (
                    other
                        .get("name")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            NodeError::InvalidConfig(format!("choice {} missing 'name'", i))
                        })?
                        .to_string(),
                    other
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                ),
            };
            if name.is_empty() {
                return Err(NodeError::InvalidConfig(format!(
                    "choice {} has an empty name",
                    i
                )));
            }
            if name == self.uncertain_port {
                return Err(NodeError::InvalidConfig(format!(
                    "choice '{}' collides with the uncertain port",
                    name
                )));
            }
            if !seen.insert(name.clone()) {
                return Err(NodeError::InvalidConfig(format!(
                    "duplicate choice '{}'",
                    name
                )));
            }
            let keywords = keyword_map
                .and_then(|m| m.get(&name))
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .map(|s| s.to_lowercase())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            self.choices.push(Choice {
                name,
                description,
                keywords,
            });
        }

        if self.choices.len() < 2 {
            return Err(NodeError::InvalidConfig(
                "a decision needs at least 2 choices".into(),
            ));
        }
        if self.choices.len() > MAX_CHOICES {
            return Err(NodeError::InvalidConfig(format!(
                "a decision supports at most {} choices",
                MAX_CHOICES
            )));
        }

        if let Some(threshold) = config.get("threshold") {
            let value = threshold
                .as_f64()
                .filter(|t| (0.0..=1.0).contains(t))
                .ok_or_else(|| {
                    NodeError::InvalidConfig("'threshold' must be a number from 0 to 1".into())
                })?;
            self.threshold = value;
        }

        if let Some(field) = config.get("field").and_then(|v| v.as_str()) {
            self.field = Some(field.to_string()).filter(|f| !f.is_empty());
        }
        if let Some(field) = config.get("scores_field").and_then(|v| v.as_str()) {
            if field.is_empty() {
                return Err(NodeError::InvalidConfig(
                    "'scores_field' must not be empty".into(),
                ));
            }
            self.scores_field = field.to_string();
        }
        if let Some(secs) = config.get("timeout_secs").and_then(|v| v.as_u64()) {
            self.timeout_secs = secs.clamp(1, 300);
        }

        self.provider = match config.get("provider").and_then(|v| v.as_str()) {
            Some("rules") => Provider::Rules,
            Some("input") => Provider::Input,
            Some("openai") => Provider::OpenAI,
            Some("anthropic") => Provider::Anthropic,
            Some(other) => {
                return Err(NodeError::InvalidConfig(format!(
                    "unknown provider: '{}'. Use: openai, anthropic, rules, input",
                    other
                )));
            }
            None if keyword_map.is_some() => Provider::Rules,
            None if config.get("scores_field").is_some() => Provider::Input,
            None if std::env::var("ANTHROPIC_API_KEY").is_ok_and(|k| !k.is_empty()) => {
                Provider::Anthropic
            }
            None if std::env::var("OPENAI_API_KEY").is_ok_and(|k| !k.is_empty()) => {
                Provider::OpenAI
            }
            None => {
                return Err(NodeError::InvalidConfig(
                    "no 'provider' given and none could be detected: set ANTHROPIC_API_KEY or OPENAI_API_KEY, or configure 'keywords' or 'scores_field'".into(),
                ));
            }
        };

        match self.provider {
            Provider::Rules if self.choices.iter().all(|c| c.keywords.is_empty()) => {
                return Err(NodeError::InvalidConfig(
                    "provider 'rules' needs 'keywords' for at least one choice".into(),
                ));
            }
            Provider::OpenAI if self.choices.len() > MAX_LOGPROB_CHOICES => {
                return Err(NodeError::InvalidConfig(format!(
                    "provider 'openai' supports at most {} choices",
                    MAX_LOGPROB_CHOICES
                )));
            }
            _ => {}
        }

        self.model = config
            .get("model")
            .and_then(|v| v.as_str())
            .filter(|m| !m.is_empty())
            .unwrap_or(match self.provider {
                Provider::OpenAI => "gpt-4o-mini",
                Provider::Anthropic => "claude-haiku-5-5",
                Provider::Rules | Provider::Input => "",
            })
            .to_string();

        Ok(())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> NodeResult<NodeOutput> {
        let scores = match self.provider {
            Provider::Rules => self.rule_scores(&self.state_text(&input)),
            Provider::Input => self.input_scores(&input),
            Provider::OpenAI => self.openai_scores(&self.state_text(&input)).await?,
            Provider::Anthropic => self.anthropic_scores(&self.state_text(&input)).await?,
        };
        let probabilities = normalize(&scores);

        // First maximum in declared order, so ties resolve deterministically.
        let (top_index, confidence) =
            probabilities
                .iter()
                .copied()
                .enumerate()
                .fold(
                    (0, f64::MIN),
                    |best, (i, p)| if p > best.1 { (i, p) } else { best },
                );
        let choice = &self.choices[top_index].name;
        let decided = confidence >= self.threshold;
        let port = if decided {
            choice.clone()
        } else {
            self.uncertain_port.clone()
        };

        let distribution: serde_json::Map<String, serde_json::Value> = self
            .choices
            .iter()
            .zip(&probabilities)
            .map(|(c, p)| (c.name.clone(), serde_json::json!(p)))
            .collect();
        let decision = serde_json::json!({
            "question": self.question,
            "choice": choice,
            "confidence": confidence,
            "decided": decided,
            "threshold": self.threshold,
            "probabilities": distribution,
            "provider": self.provider.as_str(),
        });

        let payload = match input {
            serde_json::Value::Object(mut map) => {
                map.insert("_decision".to_string(), decision);
                serde_json::Value::Object(map)
            }
            other => serde_json::json!({ "value": other, "_decision": decision }),
        };

        let log = if decided {
            format!("decided '{}' ({:.1}%)", choice, confidence * 100.0)
        } else {
            format!(
                "uncertain: best '{}' at {:.1}% is below {:.1}%, routed to '{}'",
                choice,
                confidence * 100.0,
                self.threshold * 100.0,
                port
            )
        };

        let mut ports = HashMap::new();
        ports.insert(port, payload);
        Ok(NodeOutput::multi(ports).with_log(log))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn rules_node(threshold: f64) -> DecisionNode {
        let mut node = DecisionNode::new();
        node.init(&json!({
            "question": "What does the customer want?",
            "choices": ["refund", "support", "sales"],
            "threshold": threshold,
            "field": "message",
            "keywords": {
                "refund": ["refund", "money back"],
                "support": ["error", "broken", "help"],
                "sales": ["pricing", "upgrade"]
            }
        }))
        .await
        .unwrap();
        node
    }

    #[tokio::test]
    async fn routes_confident_decision_to_choice_port() {
        let node = rules_node(0.7).await;
        let out = node
            .execute(
                json!({"message": "I want a refund, give me my money back"}),
                &ExecutionContext::new(),
            )
            .await
            .unwrap();

        assert_eq!(out.ports.len(), 1);
        let payload = &out.ports["refund"];
        assert_eq!(payload["message"], "I want a refund, give me my money back");
        assert_eq!(payload["_decision"]["choice"], "refund");
        assert_eq!(payload["_decision"]["decided"], true);
        assert_eq!(payload["_decision"]["confidence"], 1.0);
    }

    #[tokio::test]
    async fn routes_split_evidence_to_uncertain_port() {
        let node = rules_node(0.7).await;
        let out = node
            .execute(
                json!({"message": "I need help with pricing"}),
                &ExecutionContext::new(),
            )
            .await
            .unwrap();

        let payload = &out.ports["uncertain"];
        assert_eq!(payload["_decision"]["decided"], false);
        assert_eq!(payload["_decision"]["confidence"], 0.5);
    }

    #[tokio::test]
    async fn no_evidence_is_uniform_and_uncertain() {
        let node = rules_node(0.7).await;
        let out = node
            .execute(json!({"message": "hello there"}), &ExecutionContext::new())
            .await
            .unwrap();

        let probs = &out.ports["uncertain"]["_decision"]["probabilities"];
        for name in ["refund", "support", "sales"] {
            assert!((probs[name].as_f64().unwrap() - 1.0 / 3.0).abs() < 1e-9);
        }
    }

    #[tokio::test]
    async fn probabilities_always_sum_to_one() {
        let node = rules_node(0.0).await;
        let out = node
            .execute(
                json!({"message": "refund refund error upgrade"}),
                &ExecutionContext::new(),
            )
            .await
            .unwrap();

        let payload = out.ports.values().next().unwrap();
        let total: f64 = payload["_decision"]["probabilities"]
            .as_object()
            .unwrap()
            .values()
            .map(|v| v.as_f64().unwrap())
            .sum();
        assert!((total - 1.0).abs() < 1e-9);
        assert!(out.ports.contains_key("refund"));
    }

    #[tokio::test]
    async fn non_object_input_is_wrapped() {
        let mut node = DecisionNode::new();
        node.init(&json!({
            "question": "Is this spam?",
            "choices": ["spam", "ham"],
            "keywords": { "spam": ["winner", "free money"] }
        }))
        .await
        .unwrap();

        let out = node
            .execute(json!("You are a WINNER"), &ExecutionContext::new())
            .await
            .unwrap();
        let payload = &out.ports["spam"];
        assert_eq!(payload["value"], "You are a WINNER");
        assert_eq!(payload["_decision"]["provider"], "rules");
    }

    #[tokio::test]
    async fn input_provider_reads_upstream_scores() {
        let mut node = DecisionNode::new();
        node.init(&json!({
            "question": "Approve the loan?",
            "choices": ["approve", "reject"],
            "threshold": 0.8,
            "scores_field": "model.scores"
        }))
        .await
        .unwrap();
        let ctx = ExecutionContext::new();

        let out = node
            .execute(
                json!({"model": {"scores": {"approve": 9, "reject": 1}}}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(out.ports["approve"]["_decision"]["confidence"], 0.9);

        let out = node
            .execute(
                json!({"model": {"scores": {"approve": 0.6, "reject": 0.4}}}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(out.ports.contains_key("uncertain"));

        // Missing or malformed scores stay inside the declared output space.
        let out = node.execute(json!({"model": "oops"}), &ctx).await.unwrap();
        assert!(out.ports.contains_key("uncertain"));
    }

    #[tokio::test]
    async fn custom_uncertain_port() {
        let mut node = DecisionNode::new();
        node.init(&json!({
            "question": "q",
            "choices": ["a", "b"],
            "uncertain_port": "review",
            "keywords": { "a": ["x"] }
        }))
        .await
        .unwrap();
        let out = node
            .execute(json!("nothing"), &ExecutionContext::new())
            .await
            .unwrap();
        assert!(out.ports.contains_key("review"));
    }

    #[tokio::test]
    async fn init_rejects_invalid_config() {
        let bad = [
            json!({"choices": ["a", "b"], "keywords": {"a": ["x"]}}),
            json!({"question": "q", "keywords": {"a": ["x"]}}),
            json!({"question": "q", "choices": ["a"], "keywords": {"a": ["x"]}}),
            json!({"question": "q", "choices": ["a", "a"], "keywords": {"a": ["x"]}}),
            json!({"question": "q", "choices": ["a", ""], "keywords": {"a": ["x"]}}),
            json!({"question": "q", "choices": ["a", "uncertain"], "keywords": {"a": ["x"]}}),
            json!({"question": "q", "choices": ["a", "b"], "threshold": 1.5, "keywords": {"a": ["x"]}}),
            json!({"question": "q", "choices": ["a", "b"], "provider": "magic"}),
            json!({"question": "q", "choices": ["a", "b"], "provider": "rules"}),
        ];
        for config in bad {
            let mut node = DecisionNode::new();
            assert!(node.init(&config).await.is_err(), "accepted: {}", config);
        }
    }

    #[tokio::test]
    async fn openai_provider_limits_choice_count() {
        let choices: Vec<String> = (0..21).map(|i| format!("c{}", i)).collect();
        let mut node = DecisionNode::new();
        let result = node
            .init(&json!({"question": "q", "choices": choices, "provider": "openai"}))
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn normalize_handles_bad_scores() {
        assert_eq!(normalize(&[1.0, 3.0]), vec![0.25, 0.75]);
        assert_eq!(normalize(&[0.0, 0.0]), vec![0.5, 0.5]);
        assert_eq!(normalize(&[-2.0, f64::NAN, 4.0]), vec![0.0, 0.0, 1.0]);
        assert_eq!(normalize(&[f64::INFINITY, 1.0]), vec![0.0, 1.0]);
    }

    #[test]
    fn logprobs_are_mapped_to_choices() {
        let resp = json!({
            "choices": [{ "logprobs": { "content": [{ "top_logprobs": [
                { "token": "A", "logprob": (0.6f64).ln() },
                { "token": " a", "logprob": (0.1f64).ln() },
                { "token": "B", "logprob": (0.2f64).ln() },
                { "token": "Z", "logprob": (0.05f64).ln() },
                { "token": "The", "logprob": (0.05f64).ln() }
            ]}]}}]
        });
        let probs = normalize(&scores_from_logprobs(&resp, 3));
        assert!((probs[0] - 0.7 / 0.9).abs() < 1e-9);
        assert!((probs[1] - 0.2 / 0.9).abs() < 1e-9);
        assert_eq!(probs[2], 0.0);

        // A response without logprobs degrades to uniform.
        assert_eq!(
            normalize(&scores_from_logprobs(&json!({}), 2)),
            vec![0.5, 0.5]
        );
    }

    #[test]
    fn json_reply_is_parsed_defensively() {
        let names = ["yes", "no"];
        assert_eq!(
            scores_from_json_reply("Sure:\n```json\n{\"yes\": 0.8, \"no\": 0.2}\n```", &names),
            vec![0.8, 0.2]
        );
        assert_eq!(
            scores_from_json_reply("{\"yes\": 1, \"maybe\": 5}", &names),
            vec![1.0, 0.0]
        );
        assert_eq!(
            scores_from_json_reply("I think yes", &names),
            vec![0.0, 0.0]
        );
        assert_eq!(scores_from_json_reply("}{", &names), vec![0.0, 0.0]);
    }
}
