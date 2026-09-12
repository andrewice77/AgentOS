//! Generic OAuth 2.1/PKCE helper for remote MCP servers.
//! Tokens live in the OS keyring, keyed by MCP server name.

use crate::config::McpOAuthConfig;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const KEYRING_SERVICE: &str = "agentos";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_at: i64,
    #[serde(default)]
    pub token_type: String,
}

impl TokenSet {
    pub fn is_expired(&self) -> bool {
        if self.expires_at <= 0 {
            return false;
        }
        let now = now_unix();
        now + 60 >= self.expires_at
    }
}

pub fn token_account(server: &str) -> String {
    format!("mcp:{server}")
}

pub fn load_tokens(server: &str) -> Result<Option<TokenSet>, String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &token_account(server))
        .map_err(|e| e.to_string())?;
    match entry.get_password() {
        Ok(raw) => serde_json::from_str(&raw).map(Some).map_err(|e| e.to_string()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn save_tokens(server: &str, tokens: &TokenSet) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &token_account(server))
        .map_err(|e| e.to_string())?;
    let raw = serde_json::to_string(tokens).map_err(|e| e.to_string())?;
    entry.set_password(&raw).map_err(|e| e.to_string())
}

pub fn clear_tokens(server: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &token_account(server))
        .map_err(|e| e.to_string())?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn is_authenticated(server: &str) -> bool {
    load_tokens(server)
        .ok()
        .flatten()
        .map(|t| !t.access_token.is_empty())
        .unwrap_or(false)
}

pub async fn bearer_token(server: &str, oauth: &McpOAuthConfig) -> Result<String, String> {
    let mut tokens = load_tokens(server)?
        .ok_or_else(|| {
            format!("MCP '{server}' richiede login OAuth. Apri MCP → Accedi.")
        })?;
    if tokens.is_expired() {
        if let Some(refresh) = tokens.refresh_token.clone() {
            tokens = refresh_tokens(oauth, &refresh).await?;
            save_tokens(server, &tokens)?;
        } else {
            return Err(format!(
                "Token OAuth di '{server}' scaduto. Riautentica dal pannello MCP."
            ));
        }
    }
    Ok(tokens.access_token)
}

pub async fn authorize(
    app: &tauri::AppHandle,
    server: &str,
    oauth: &McpOAuthConfig,
) -> Result<TokenSet, String> {
    if !oauth.is_configured() {
        return Err("OAuth incompleto: servono client_id, auth_url e token_url.".into());
    }
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("bind loopback OAuth: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| e.to_string())?
        .port();
    let redirect = format!("http://127.0.0.1:{port}/callback");
    let verifier = random_urlsafe(32);
    let challenge = pkce_challenge(&verifier);
    let state = random_urlsafe(16);

    let mut params: HashMap<String, String> = HashMap::new();
    params.insert("response_type".into(), "code".into());
    params.insert("client_id".into(), oauth.client_id.clone());
    params.insert("redirect_uri".into(), redirect.clone());
    params.insert("state".into(), state.clone());
    params.insert("code_challenge".into(), challenge);
    params.insert("code_challenge_method".into(), "S256".into());
    if !oauth.scopes.is_empty() {
        params.insert("scope".into(), oauth.scopes.join(" "));
    }
    for (k, v) in &oauth.extra_auth_params {
        params.insert(k.clone(), v.clone());
    }
    let url = format!("{}?{}", oauth.auth_url.trim(), encode_query(&params));

    open_browser(app, &url)?;

    let (code, got_state) = wait_for_code(listener).await?;
    if got_state != state {
        return Err("OAuth state mismatch — riprova il login.".into());
    }
    let tokens = exchange_code(oauth, &redirect, &code, &verifier).await?;
    save_tokens(server, &tokens)?;
    Ok(tokens)
}

fn open_browser(app: &tauri::AppHandle, url: &str) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| format!("impossibile aprire il browser: {e}"))
}

async fn wait_for_code(listener: TcpListener) -> Result<(String, String), String> {
    let (mut socket, _) = tokio::time::timeout(
        std::time::Duration::from_secs(180),
        listener.accept(),
    )
    .await
    .map_err(|_| "Timeout login OAuth (3 minuti). Riprova.".to_string())?
    .map_err(|e| e.to_string())?;

    let mut buf = vec![0u8; 8192];
    let n = socket.read(&mut buf).await.map_err(|e| e.to_string())?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let first = req.lines().next().unwrap_or("");
    let path = first.split_whitespace().nth(1).unwrap_or("");
    let query = path.splitn(2, '?').nth(1).unwrap_or("");
    let qs = parse_query(query);
    if let Some(err) = qs.get("error") {
        let desc = qs.get("error_description").cloned().unwrap_or_default();
        let body = html_page(
            "Login MCP fallito",
            &format!("<p>{err} {desc}</p><p>Puoi chiudere questa finestra.</p>"),
        );
        let _ = socket.write_all(http_response(&body).as_bytes()).await;
        return Err(format!("OAuth error: {err} {desc}"));
    }
    let code = qs
        .get("code")
        .cloned()
        .ok_or_else(|| "callback OAuth senza code".to_string())?;
    let state = qs.get("state").cloned().unwrap_or_default();
    let body = html_page(
        "AgentOS collegato",
        "<p>Autenticazione riuscita. Puoi chiudere questa finestra e tornare ad AgentOS.</p>",
    );
    let _ = socket.write_all(http_response(&body).as_bytes()).await;
    Ok((code, state))
}

async fn exchange_code(
    oauth: &McpOAuthConfig,
    redirect: &str,
    code: &str,
    verifier: &str,
) -> Result<TokenSet, String> {
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect),
        ("client_id", oauth.client_id.as_str()),
        ("code_verifier", verifier),
    ];
    if !oauth.client_secret.is_empty() {
        form.push(("client_secret", oauth.client_secret.as_str()));
    }
    post_token(&oauth.token_url, &form).await
}

async fn refresh_tokens(oauth: &McpOAuthConfig, refresh: &str) -> Result<TokenSet, String> {
    let mut form = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh),
        ("client_id", oauth.client_id.as_str()),
    ];
    if !oauth.client_secret.is_empty() {
        form.push(("client_secret", oauth.client_secret.as_str()));
    }
    let mut tokens = post_token(&oauth.token_url, &form).await?;
    if tokens.refresh_token.is_none() {
        tokens.refresh_token = Some(refresh.to_string());
    }
    Ok(tokens)
}

async fn post_token(token_url: &str, form: &[(&str, &str)]) -> Result<TokenSet, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(token_url)
        .header("Accept", "application/json")
        .form(form)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("token request: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("token endpoint {status}: {text}"));
    }
    parse_token_response(&text)
}

fn parse_token_response(text: &str) -> Result<TokenSet, String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("token JSON: {e} — {text}"))?;
    let access = v
        .get("access_token")
        .and_then(|x| x.as_str())
        .ok_or("token response senza access_token")?
        .to_string();
    let refresh = v
        .get("refresh_token")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let expires_in = v.get("expires_in").and_then(|x| x.as_i64()).unwrap_or(3600);
    Ok(TokenSet {
        access_token: access,
        refresh_token: refresh,
        expires_at: now_unix() + expires_in,
        token_type: v
            .get("token_type")
            .and_then(|x| x.as_str())
            .unwrap_or("Bearer")
            .to_string(),
    })
}

fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

fn random_urlsafe(nbytes: usize) -> String {
    let mut raw = Vec::with_capacity(nbytes);
    while raw.len() < nbytes {
        raw.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    }
    raw.truncate(nbytes);
    URL_SAFE_NO_PAD.encode(raw)
}

fn encode_query(params: &HashMap<String, String>) -> String {
    params
        .iter()
        .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn parse_query(q: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for part in q.split('&') {
        if part.is_empty() {
            continue;
        }
        let mut it = part.splitn(2, '=');
        let k = it.next().unwrap_or("");
        let v = it.next().unwrap_or("");
        out.insert(
            urlencoding::decode(k).unwrap_or_default().into_owned(),
            urlencoding::decode(v).unwrap_or_default().into_owned(),
        );
    }
    out
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn html_page(title: &str, inner: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=utf-8><title>{title}</title>
<style>body{{font-family:system-ui,sans-serif;background:#111;color:#eee;display:grid;place-items:center;height:100vh;margin:0}}
main{{max-width:28rem;padding:1.5rem;border:1px solid #333;border-radius:12px}}</style></head>
<body><main><h1>{title}</h1>{inner}</main></body></html>"
    )
}

fn http_response(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}
