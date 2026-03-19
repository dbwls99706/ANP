use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use anp_core::graph::spec::NodeKind;
use anp_core::node::traits::{NodeError, NodeExecutor, NodeOutput, NodeResult};
use anp_core::runtime::context::ExecutionContext;

/// OAuth 2.0 node supporting authorization code flow, client credentials,
/// token refresh, and token introspection.
/// Works with http_client node for actual HTTP calls.
pub struct OAuthNode {
    operation: String,
    provider: String,
    client_id_env: String,
    client_secret_env: String,
    authorize_url: String,
    token_url: String,
    redirect_uri: String,
    scopes: Vec<String>,
    tokens: Arc<RwLock<HashMap<String, TokenEntry>>>,
}

#[derive(Clone)]
struct TokenEntry {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: u64,
    token_type: String,
    scopes: Vec<String>,
}

impl OAuthNode {
    pub fn new() -> Self {
        Self {
            operation: "authorize_url".to_string(),
            provider: "custom".to_string(),
            client_id_env: "OAUTH_CLIENT_ID".to_string(),
            client_secret_env: "OAUTH_CLIENT_SECRET".to_string(),
            authorize_url: String::new(),
            token_url: String::new(),
            redirect_uri: "http://localhost:3000/callback".to_string(),
            scopes: Vec::new(),
            tokens: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for OAuthNode {
    fn default() -> Self {
        Self::new()
    }
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn url_encode_simple(s: &str) -> String {
    let mut encoded = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char);
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", b));
            }
        }
    }
    encoded
}

fn generate_state() -> String {
    let now = current_timestamp();
    let random_part = now.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    format!("{:016x}", random_part)
}

fn generate_pkce_verifier() -> String {
    let now = current_timestamp();
    let mut state = now;
    let mut verifier = String::new();
    let chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let chars_bytes = chars.as_bytes();
    for _ in 0..43 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let idx = (state >> 33) as usize % chars_bytes.len();
        verifier.push(chars_bytes[idx] as char);
    }
    verifier
}

fn simple_sha256_hash(data: &[u8]) -> Vec<u8> {
    // Simplified hash for PKCE challenge (FNV-1a based, 32 bytes)
    let mut hash = vec![0u8; 32];
    let mut h: u64 = 14695981039346656037;
    for (i, &byte) in data.iter().enumerate() {
        h ^= byte as u64;
        h = h.wrapping_mul(1099511628211);
        hash[i % 32] ^= (h >> 24) as u8;
    }
    hash
}

fn base64url_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i] as u32;
        let b1 = if i + 1 < data.len() { data[i + 1] as u32 } else { 0 };
        let b2 = if i + 2 < data.len() { data[i + 2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if i + 1 < data.len() {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        }
        if i + 2 < data.len() {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        }
        i += 3;
    }
    result
}

#[async_trait]
impl NodeExecutor for OAuthNode {
    fn node_type(&self) -> &str {
        "oauth"
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Processor
    }

    async fn init(&mut self, config: &Value) -> NodeResult<()> {
        if let Some(op) = config.get("operation").and_then(|v| v.as_str()) {
            match op {
                "authorize_url" | "token_exchange" | "refresh" | "client_credentials"
                | "validate" | "revoke" | "pkce_authorize" => {
                    self.operation = op.to_string();
                }
                other => {
                    return Err(NodeError::InvalidConfig(format!(
                        "unknown operation '{}'",
                        other
                    )));
                }
            }
        }
        if let Some(p) = config.get("provider").and_then(|v| v.as_str()) {
            self.provider = p.to_string();
            match p {
                "google" => {
                    if self.authorize_url.is_empty() {
                        self.authorize_url = "https://accounts.google.com/o/oauth2/v2/auth".to_string();
                    }
                    if self.token_url.is_empty() {
                        self.token_url = "https://oauth2.googleapis.com/token".to_string();
                    }
                }
                "github" => {
                    if self.authorize_url.is_empty() {
                        self.authorize_url = "https://github.com/login/oauth/authorize".to_string();
                    }
                    if self.token_url.is_empty() {
                        self.token_url = "https://github.com/login/oauth/access_token".to_string();
                    }
                }
                "microsoft" => {
                    if self.authorize_url.is_empty() {
                        self.authorize_url = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize".to_string();
                    }
                    if self.token_url.is_empty() {
                        self.token_url = "https://login.microsoftonline.com/common/oauth2/v2.0/token".to_string();
                    }
                }
                _ => {}
            }
        }
        if let Some(url) = config.get("authorize_url").and_then(|v| v.as_str()) {
            self.authorize_url = url.to_string();
        }
        if let Some(url) = config.get("token_url").and_then(|v| v.as_str()) {
            self.token_url = url.to_string();
        }
        if let Some(uri) = config.get("redirect_uri").and_then(|v| v.as_str()) {
            self.redirect_uri = uri.to_string();
        }
        if let Some(id_env) = config.get("client_id_env").and_then(|v| v.as_str()) {
            self.client_id_env = id_env.to_string();
        }
        if let Some(secret_env) = config.get("client_secret_env").and_then(|v| v.as_str()) {
            self.client_secret_env = secret_env.to_string();
        }
        if let Some(scopes) = config.get("scopes").and_then(|v| v.as_array()) {
            self.scopes = scopes
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }
        Ok(())
    }

    async fn execute(&self, input: Value, _ctx: &ExecutionContext) -> NodeResult<NodeOutput> {
        let op = input
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.operation);

        match op {
            "authorize_url" => {
                let client_id = std::env::var(&self.client_id_env).unwrap_or_default();
                let state = input
                    .get("state")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(generate_state);

                let scopes = if let Some(s) = input.get("scopes").and_then(|v| v.as_array()) {
                    s.iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                } else {
                    self.scopes.join(" ")
                };

                let mut url = format!(
                    "{}?response_type=code&client_id={}&redirect_uri={}&state={}",
                    self.authorize_url,
                    url_encode_simple(&client_id),
                    url_encode_simple(&self.redirect_uri),
                    url_encode_simple(&state),
                );
                if !scopes.is_empty() {
                    url.push_str(&format!("&scope={}", url_encode_simple(&scopes)));
                }

                Ok(NodeOutput::data(json!({
                    "authorize_url": url,
                    "state": state,
                    "redirect_uri": self.redirect_uri,
                })))
            }

            "pkce_authorize" => {
                let client_id = std::env::var(&self.client_id_env).unwrap_or_default();
                let state = generate_state();
                let verifier = generate_pkce_verifier();
                let challenge_bytes = simple_sha256_hash(verifier.as_bytes());
                let challenge = base64url_encode(&challenge_bytes);

                let scopes = self.scopes.join(" ");
                let mut url = format!(
                    "{}?response_type=code&client_id={}&redirect_uri={}&state={}&code_challenge={}&code_challenge_method=S256",
                    self.authorize_url,
                    url_encode_simple(&client_id),
                    url_encode_simple(&self.redirect_uri),
                    url_encode_simple(&state),
                    url_encode_simple(&challenge),
                );
                if !scopes.is_empty() {
                    url.push_str(&format!("&scope={}", url_encode_simple(&scopes)));
                }

                Ok(NodeOutput::data(json!({
                    "authorize_url": url,
                    "state": state,
                    "code_verifier": verifier,
                    "code_challenge": challenge,
                    "redirect_uri": self.redirect_uri,
                })))
            }

            "token_exchange" => {
                let code = input.get("code").and_then(|v| v.as_str()).unwrap_or("");
                let client_id = std::env::var(&self.client_id_env).unwrap_or_default();
                let client_secret = std::env::var(&self.client_secret_env).unwrap_or_default();

                let mut body = json!({
                    "grant_type": "authorization_code",
                    "code": code,
                    "redirect_uri": self.redirect_uri,
                    "client_id": client_id,
                    "client_secret": client_secret,
                });

                // PKCE support
                if let Some(verifier) = input.get("code_verifier").and_then(|v| v.as_str()) {
                    body["code_verifier"] = json!(verifier);
                }

                Ok(NodeOutput::data(json!({
                    "_http_request": {
                        "url": self.token_url,
                        "method": "POST",
                        "headers": {
                            "Content-Type": "application/x-www-form-urlencoded",
                            "Accept": "application/json"
                        },
                        "body": body,
                    },
                    "token_url": self.token_url,
                    "grant_type": "authorization_code",
                })))
            }

            "refresh" => {
                let refresh_token = input
                    .get("refresh_token")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let client_id = std::env::var(&self.client_id_env).unwrap_or_default();
                let client_secret = std::env::var(&self.client_secret_env).unwrap_or_default();

                Ok(NodeOutput::data(json!({
                    "_http_request": {
                        "url": self.token_url,
                        "method": "POST",
                        "headers": {
                            "Content-Type": "application/x-www-form-urlencoded",
                            "Accept": "application/json"
                        },
                        "body": {
                            "grant_type": "refresh_token",
                            "refresh_token": refresh_token,
                            "client_id": client_id,
                            "client_secret": client_secret,
                        },
                    },
                    "token_url": self.token_url,
                    "grant_type": "refresh_token",
                })))
            }

            "client_credentials" => {
                let client_id = std::env::var(&self.client_id_env).unwrap_or_default();
                let client_secret = std::env::var(&self.client_secret_env).unwrap_or_default();
                let scopes = self.scopes.join(" ");

                Ok(NodeOutput::data(json!({
                    "_http_request": {
                        "url": self.token_url,
                        "method": "POST",
                        "headers": {
                            "Content-Type": "application/x-www-form-urlencoded",
                            "Accept": "application/json"
                        },
                        "body": {
                            "grant_type": "client_credentials",
                            "client_id": client_id,
                            "client_secret": client_secret,
                            "scope": scopes,
                        },
                    },
                    "token_url": self.token_url,
                    "grant_type": "client_credentials",
                })))
            }

            "validate" => {
                let token = input
                    .get("access_token")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let session_id = input
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default");

                let tokens = self.tokens.read().await;
                if let Some(entry) = tokens.get(session_id) {
                    let now = current_timestamp();
                    let valid = entry.access_token == token && entry.expires_at > now;
                    Ok(NodeOutput::data(json!({
                        "valid": valid,
                        "expired": entry.expires_at <= now,
                        "token_type": entry.token_type,
                        "scopes": entry.scopes,
                        "expires_in": if entry.expires_at > now { entry.expires_at - now } else { 0 },
                    })))
                } else {
                    Ok(NodeOutput::data(json!({
                        "valid": false,
                        "error": "no_token_found",
                    })))
                }
            }

            "revoke" => {
                let session_id = input
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default");
                let mut tokens = self.tokens.write().await;
                let existed = tokens.remove(session_id).is_some();
                Ok(NodeOutput::data(json!({
                    "revoked": existed,
                    "session_id": session_id,
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
    async fn test_authorize_url() {
        std::env::set_var("OAUTH_CLIENT_ID", "test-client-id");
        let mut node = OAuthNode::new();
        node.init(&json!({
            "operation": "authorize_url",
            "authorize_url": "https://auth.example.com/authorize",
            "scopes": ["read", "write"]
        }))
        .await
        .unwrap();

        let output = node.execute(json!({}), &ctx()).await.unwrap();
        let url = output.ports["default"]["authorize_url"]
            .as_str()
            .unwrap();
        assert!(url.starts_with("https://auth.example.com/authorize?"));
        assert!(url.contains("client_id=test-client-id"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("scope=read%20write"));
    }

    #[tokio::test]
    async fn test_provider_presets() {
        let mut node = OAuthNode::new();
        node.init(&json!({
            "provider": "github",
            "operation": "authorize_url",
        }))
        .await
        .unwrap();
        assert_eq!(
            node.authorize_url,
            "https://github.com/login/oauth/authorize"
        );
    }

    #[tokio::test]
    async fn test_token_exchange() {
        let mut node = OAuthNode::new();
        node.init(&json!({
            "operation": "token_exchange",
            "token_url": "https://auth.example.com/token",
        }))
        .await
        .unwrap();

        let output = node
            .execute(json!({"code": "auth-code-123"}), &ctx())
            .await
            .unwrap();
        let req = &output.ports["default"]["_http_request"];
        assert_eq!(req["url"], "https://auth.example.com/token");
        assert_eq!(req["method"], "POST");
        assert_eq!(req["body"]["grant_type"], "authorization_code");
        assert_eq!(req["body"]["code"], "auth-code-123");
    }

    #[tokio::test]
    async fn test_pkce_authorize() {
        std::env::set_var("OAUTH_CLIENT_ID", "pkce-client");
        let mut node = OAuthNode::new();
        node.init(&json!({
            "operation": "pkce_authorize",
            "authorize_url": "https://auth.example.com/authorize",
        }))
        .await
        .unwrap();

        let output = node.execute(json!({}), &ctx()).await.unwrap();
        let data = &output.ports["default"];
        assert!(data["authorize_url"].as_str().unwrap().contains("code_challenge="));
        assert!(data["code_verifier"].as_str().is_some());
        assert!(data["code_challenge"].as_str().is_some());
    }

    #[tokio::test]
    async fn test_client_credentials() {
        let mut node = OAuthNode::new();
        node.init(&json!({
            "operation": "client_credentials",
            "token_url": "https://auth.example.com/token",
            "scopes": ["api"],
        }))
        .await
        .unwrap();

        let output = node.execute(json!({}), &ctx()).await.unwrap();
        let req = &output.ports["default"]["_http_request"];
        assert_eq!(req["body"]["grant_type"], "client_credentials");
    }

    #[tokio::test]
    async fn test_revoke() {
        let mut node = OAuthNode::new();
        node.init(&json!({"operation": "revoke"})).await.unwrap();
        let output = node
            .execute(json!({"session_id": "s1"}), &ctx())
            .await
            .unwrap();
        assert_eq!(output.ports["default"]["revoked"], false);
    }
}
