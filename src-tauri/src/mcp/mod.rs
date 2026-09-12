//! Generic MCP client: stdio (NDJSON + Content-Length) and streamable HTTP, plus OAuth.

pub mod agent;
pub mod entity_ctx;
pub mod zoho_ctx;
mod oauth;
pub mod watch;

use crate::config::{McpOAuthConfig, McpServerConfig, McpWatchConfig};
use crate::db::{insert_ledger, Db};
use crate::permissions::level_for_mcp;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex as AsyncMutex;

pub use oauth::{clear_tokens, is_authenticated};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub server: String,
    pub name: String,
    pub description: String,
    /// JSON Schema from the MCP server (`inputSchema`), if provided.
    #[serde(default)]
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpStatus {
    pub servers: Vec<McpServerStatus>,
    pub tools: Vec<McpToolInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerStatus {
    pub name: String,
    pub enabled: bool,
    pub connected: bool,
    pub error: Option<String>,
    pub command: String,
    pub transport: String,
    pub authenticated: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingMcpCall {
    pub server: String,
    pub tool: String,
    pub arguments: Value,
    pub level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpAuthStatus {
    pub server: String,
    pub configured: bool,
    pub authenticated: bool,
}

enum LiveSession {
    Stdio {
        #[allow(dead_code)]
        child: Child,
        stdin: ChildStdin,
        stdout: BufReader<ChildStdout>,
        next_id: u64,
        framed: bool,
    },
    Http {
        url: String,
        headers: HashMap<String, String>,
        session_id: Option<String>,
        next_id: u64,
        oauth: Option<McpOAuthConfig>,
        server_name: String,
    },
}

pub struct McpHub {
    inner: AsyncMutex<HashMap<String, LiveSession>>,
    configs: Mutex<Vec<McpServerConfig>>,
    last_errors: Mutex<HashMap<String, String>>,
    pub pending: Mutex<Option<PendingMcpCall>>,
}

impl McpHub {
    pub fn new(configs: Vec<McpServerConfig>) -> Self {
        Self {
            inner: AsyncMutex::new(HashMap::new()),
            configs: Mutex::new(configs),
            last_errors: Mutex::new(HashMap::new()),
            pending: Mutex::new(None),
        }
    }

    pub fn update_configs(&self, configs: Vec<McpServerConfig>) {
        *self.configs.lock() = configs;
    }

    pub fn list_configs(&self) -> Vec<McpServerConfig> {
        self.configs.lock().clone()
    }

    pub fn take_pending(&self) -> Option<PendingMcpCall> {
        self.pending.lock().take()
    }

    pub fn peek_pending(&self) -> Option<PendingMcpCall> {
        self.pending.lock().clone()
    }

    pub async fn refresh_connections(&self) -> Result<(), String> {
        let configs = self.list_configs();
        {
            self.inner.lock().await.clear();
        }
        self.last_errors.lock().clear();
        for cfg in configs.into_iter().filter(|c| c.enabled) {
            match connect_server(&cfg).await {
                Ok(session) => {
                    self.inner.lock().await.insert(cfg.name.clone(), session);
                }
                Err(e) => {
                    tracing::warn!("MCP server {} failed: {e}", cfg.name);
                    crate::debuglog::warn(None, format!("mcp:start_fail {} — {e}", cfg.name));
                    self.last_errors.lock().insert(cfg.name.clone(), e);
                }
            }
        }
        Ok(())
    }

    pub async fn status(&self) -> Result<McpStatus, String> {
        let configs = self.list_configs();
        let connected: std::collections::HashSet<String> =
            self.inner.lock().await.keys().cloned().collect();
        let errs = self.last_errors.lock().clone();
        let servers = configs
            .iter()
            .map(|c| {
                let endpoint = if c.transport == "http" || !c.url.is_empty() {
                    c.url.clone()
                } else {
                    format!("{} {}", c.command, c.args.join(" "))
                };
                let authenticated = c.oauth.as_ref().and_then(|o| {
                    o.is_configured().then_some(is_authenticated(&c.name))
                });
                McpServerStatus {
                    name: c.name.clone(),
                    enabled: c.enabled,
                    connected: connected.contains(&c.name),
                    error: errs.get(&c.name).cloned(),
                    command: endpoint,
                    transport: c.transport.clone(),
                    authenticated,
                }
            })
            .collect();
        let tools = self.list_tools().await.unwrap_or_default();
        Ok(McpStatus { servers, tools })
    }

    pub fn auth_status(&self) -> Vec<McpAuthStatus> {
        self.list_configs()
            .into_iter()
            .map(|c| {
                let configured = c.oauth.as_ref().map(|o| o.is_configured()).unwrap_or(false);
                McpAuthStatus {
                    server: c.name.clone(),
                    configured,
                    authenticated: configured && is_authenticated(&c.name),
                }
            })
            .collect()
    }

    pub async fn list_tools(&self) -> Result<Vec<McpToolInfo>, String> {
        let mut out = Vec::new();
        let mut map = self.inner.lock().await;
        let names: Vec<String> = map.keys().cloned().collect();
        for name in names {
            let Some(session) = map.get_mut(&name) else {
                continue;
            };
            match rpc(session, "tools/list", json!({})).await {
                Ok(result) => {
                    if let Some(tools) = result.get("tools").and_then(|t| t.as_array()) {
                        for t in tools {
                            out.push(McpToolInfo {
                                server: name.clone(),
                                name: t
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("unknown")
                                    .to_string(),
                                description: t
                                    .get("description")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                input_schema: t
                                    .get("inputSchema")
                                    .or_else(|| t.get("input_schema"))
                                    .cloned()
                                    .unwrap_or(Value::Null),
                            });
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("tools/list failed for {name}: {e}");
                    crate::debuglog::warn(None, format!("mcp:tools_list_fail {name} — {e}"));
                }
            }
        }
        Ok(out)
    }

    pub fn resolve_watch_tool(&self, watch: &McpWatchConfig, tools: &[McpToolInfo], server: &str) -> Option<String> {
        if !watch.tool.trim().is_empty() {
            return Some(watch.tool.clone());
        }
        let hint = watch.tool_hint.to_lowercase();
        if hint.is_empty() {
            return None;
        }
        tools.iter().find(|t| {
            t.server == server
                && (t.name.to_lowercase().contains(&hint)
                    || t.description.to_lowercase().contains(&hint))
        })
        .map(|t| t.name.clone())
    }

    pub async fn call_tool(
        &self,
        db: &Db,
        default_mcp_level: &str,
        server: &str,
        tool: &str,
        arguments: Value,
        approved: bool,
    ) -> Result<Value, String> {
        self.call_tool_inner(db, default_mcp_level, server, tool, arguments, approved, false)
            .await
    }

    /// Watch polls skip the confirmation prompt (the user already opted in via config).
    pub async fn call_tool_trusted(
        &self,
        db: &Db,
        default_mcp_level: &str,
        server: &str,
        tool: &str,
        arguments: Value,
    ) -> Result<Value, String> {
        self.call_tool_inner(db, default_mcp_level, server, tool, arguments, true, true)
            .await
    }

    async fn call_tool_inner(
        &self,
        db: &Db,
        default_mcp_level: &str,
        server: &str,
        tool: &str,
        arguments: Value,
        approved: bool,
        trusted_watch: bool,
    ) -> Result<Value, String> {
        let level = level_for_mcp(default_mcp_level);
        let summary = format!("MCP {server}/{tool}");
        let needs_confirm = level.requires_confirmation() && !approved && !trusted_watch;
        {
            let conn = db.lock()?;
            insert_ledger(
                &conn,
                &format!("mcp:{server}/{tool}"),
                level.as_str(),
                if needs_confirm { "proposed" } else { "executed" },
                &summary,
                &json!({ "arguments": arguments, "watch": trusted_watch }),
            )?;
        }

        if needs_confirm {
            *self.pending.lock() = Some(PendingMcpCall {
                server: server.to_string(),
                tool: tool.to_string(),
                arguments: arguments.clone(),
                level: level.as_str().to_string(),
            });
            return Err(format!(
                "APPROVAL_REQUIRED:{}:{}:{}",
                level.as_str(),
                server,
                tool
            ));
        }

        let mut map = self.inner.lock().await;
        let live = map.get_mut(server).ok_or_else(|| {
            format!("MCP server '{server}' is not connected. Apri MCP → Riconnetti.")
        })?;
        let result = rpc(
            live,
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
        )
        .await?;

        {
            let conn = db.lock()?;
            insert_ledger(
                &conn,
                &format!("mcp:{server}/{tool}"),
                level.as_str(),
                "executed",
                &summary,
                &json!({ "result": result, "watch": trusted_watch }),
            )?;
        }
        *self.pending.lock() = None;
        Ok(result)
    }
}

async fn connect_server(cfg: &McpServerConfig) -> Result<LiveSession, String> {
    let http = cfg.transport == "http"
        || cfg.transport == "sse"
        || cfg.transport == "streamable-http"
        || !cfg.url.trim().is_empty();
    if http {
        connect_http(cfg).await
    } else {
        connect_stdio(cfg).await
    }
}

async fn connect_stdio(cfg: &McpServerConfig) -> Result<LiveSession, String> {
    if cfg.command.trim().is_empty() {
        return Err("stdio MCP richiede un comando".into());
    }
    let mut cmd = Command::new(&cfg.command);
    cmd.args(&cfg.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for (k, v) in cfg.env.iter() {
        cmd.env(k, v);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn {} {}: {e}", cfg.command, cfg.args.join(" ")))?;
    let stdin = child.stdin.take().ok_or("missing stdin")?;
    let stdout = child.stdout.take().ok_or("missing stdout")?;
    let mut session = LiveSession::Stdio {
        child,
        stdin,
        stdout: BufReader::new(stdout),
        next_id: 1,
        framed: true,
    };
    handshake(&mut session).await?;
    Ok(session)
}

async fn connect_http(cfg: &McpServerConfig) -> Result<LiveSession, String> {
    let url = cfg.url.trim();
    if url.is_empty() {
        return Err("HTTP MCP richiede un URL".into());
    }
    if let Some(oauth) = &cfg.oauth {
        if oauth.is_configured() && !is_authenticated(&cfg.name) {
            return Err("OAuth richiesto: premi Accedi nel pannello MCP".into());
        }
    }
    let mut session = LiveSession::Http {
        url: url.to_string(),
        headers: cfg.headers.clone(),
        session_id: None,
        next_id: 1,
        oauth: cfg.oauth.clone(),
        server_name: cfg.name.clone(),
    };
    handshake(&mut session).await?;
    Ok(session)
}

async fn handshake(session: &mut LiveSession) -> Result<(), String> {
    rpc(
        session,
        "initialize",
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "agentos", "version": "0.1.0" }
        }),
    )
    .await
    .map_err(|e| format!("initialize failed: {e}"))?;
    notify(session, "notifications/initialized", json!({})).await
}

fn next_id(session: &mut LiveSession) -> u64 {
    match session {
        LiveSession::Stdio { next_id, .. } | LiveSession::Http { next_id, .. } => {
            let id = *next_id;
            *next_id += 1;
            id
        }
    }
}

async fn notify(session: &mut LiveSession, method: &str, params: Value) -> Result<(), String> {
    let req = json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    });
    match session {
        LiveSession::Stdio {
            stdin,
            framed,
            ..
        } => write_stdio(stdin, &req, *framed).await,
        LiveSession::Http { .. } => {
            // Notifications must not require a JSON-RPC response body.
            match http_post(session, &req).await {
                Ok(_) => Ok(()),
                Err(e) if e.contains("EOF") || e.contains("empty") => Ok(()),
                Err(e) => Err(e),
            }
        }
    }
}

async fn rpc(session: &mut LiveSession, method: &str, params: Value) -> Result<Value, String> {
    let id = next_id(session);
    let req = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    match session {
        LiveSession::Stdio { .. } => rpc_stdio(session, id, &req, method).await,
        LiveSession::Http { .. } => rpc_http(session, id, &req, method).await,
    }
}

async fn rpc_stdio(
    session: &mut LiveSession,
    id: u64,
    req: &Value,
    method: &str,
) -> Result<Value, String> {
    let LiveSession::Stdio {
        stdin,
        stdout,
        framed,
        ..
    } = session
    else {
        unreachable!();
    };
    write_stdio(stdin, req, *framed).await?;
    let deadline = Duration::from_secs(if method == "tools/call" { 45 } else { 20 });
    let started = std::time::Instant::now();
    loop {
        if started.elapsed() > deadline {
            return Err(format!("MCP timeout waiting for {method} (id={id})"));
        }
        let v = read_stdio_message(stdout, framed).await?;
        if v.get("method").is_some() && v.get("id").is_none() {
            continue;
        }
        if !id_matches(&v, id) {
            continue;
        }
        if let Some(err) = v.get("error") {
            return Err(err.to_string());
        }
        return Ok(v.get("result").cloned().unwrap_or(Value::Null));
    }
}

async fn write_stdio(stdin: &mut ChildStdin, req: &Value, framed: bool) -> Result<(), String> {
    let payload = serde_json::to_vec(req).map_err(|e| e.to_string())?;
    // Spec servers speak Content-Length; the bundled echo still accepts NDJSON.
    if framed {
        let header = format!("Content-Length: {}\r\n\r\n", payload.len());
        stdin
            .write_all(header.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        stdin.write_all(&payload).await.map_err(|e| e.to_string())?;
    } else {
        stdin.write_all(&payload).await.map_err(|e| e.to_string())?;
        stdin.write_all(b"\n").await.map_err(|e| e.to_string())?;
    }
    stdin.flush().await.map_err(|e| e.to_string())
}

async fn read_stdio_message(
    stdout: &mut BufReader<ChildStdout>,
    framed: &mut bool,
) -> Result<Value, String> {
    let mut first = String::new();
    let n = tokio::time::timeout(Duration::from_secs(8), stdout.read_line(&mut first))
        .await
        .map_err(|_| "MCP read timeout".to_string())?
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err("MCP server closed stdout".into());
    }
    let trimmed = first.trim();
    if trimmed.is_empty() {
        return Box::pin(read_stdio_message(stdout, framed)).await;
    }
    if trimmed.to_ascii_lowercase().starts_with("content-length:") {
        *framed = true;
        let len: usize = trimmed
            .split(':')
            .nth(1)
            .ok_or("bad Content-Length")?
            .trim()
            .parse()
            .map_err(|e| format!("Content-Length: {e}"))?;
        loop {
            let mut line = String::new();
            stdout.read_line(&mut line).await.map_err(|e| e.to_string())?;
            if line.trim().is_empty() {
                break;
            }
        }
        let mut buf = vec![0u8; len];
        stdout.read_exact(&mut buf).await.map_err(|e| e.to_string())?;
        return serde_json::from_slice(&buf).map_err(|e| e.to_string());
    }
    *framed = false;
    serde_json::from_str(trimmed).map_err(|e| e.to_string())
}

async fn rpc_http(
    session: &mut LiveSession,
    id: u64,
    req: &Value,
    method: &str,
) -> Result<Value, String> {
    let v = http_post(session, req).await?;
    if v.get("method").is_some() && v.get("id").is_none() {
        return Err(format!("HTTP MCP returned a notification for {method}"));
    }
    if !id_matches(&v, id) && v.get("id").is_some() {
        return Err(format!("HTTP MCP id mismatch on {method}"));
    }
    if let Some(err) = v.get("error") {
        return Err(err.to_string());
    }
    Ok(v.get("result").cloned().unwrap_or(v))
}

async fn http_post(session: &mut LiveSession, req: &Value) -> Result<Value, String> {
    let LiveSession::Http {
        url,
        headers,
        session_id,
        oauth,
        server_name,
        ..
    } = session
    else {
        unreachable!();
    };
    let client = reqwest::Client::new();
    let mut builder = client
        .post(url.as_str())
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2024-11-05")
        .json(req)
        .timeout(Duration::from_secs(45));
    if let Some(sid) = session_id.as_ref() {
        builder = builder.header("Mcp-Session-Id", sid);
    }
    for (k, v) in headers.iter() {
        builder = builder.header(k, v);
    }
    if let Some(oauth) = oauth.as_ref() {
        if oauth.is_configured() {
            let token = oauth::bearer_token(server_name, oauth).await?;
            builder = builder.bearer_auth(token);
        }
    }
    let resp = builder.send().await.map_err(|e| format!("HTTP MCP: {e}"))?;
    if let Some(sid) = resp
        .headers()
        .get("mcp-session-id")
        .or_else(|| resp.headers().get("Mcp-Session-Id"))
    {
        if let Ok(s) = sid.to_str() {
            *session_id = Some(s.to_string());
        }
    }
    let status = resp.status();
    let ctype = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if status.as_u16() == 401 {
        return Err("HTTP MCP 401 — riautentica OAuth".into());
    }
    if !status.is_success() {
        return Err(format!("HTTP MCP {status}: {text}"));
    }
    // Zoho (and many streamable HTTP servers) ack notifications with 202 + empty body.
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    if ctype.contains("text/event-stream") {
        return parse_sse_jsonrpc(&text);
    }
    serde_json::from_str(&text).map_err(|e| format!("HTTP MCP JSON: {e} — {text}"))
}

fn parse_sse_jsonrpc(text: &str) -> Result<Value, String> {
    let mut last: Option<Value> = None;
    for block in text.split("\n\n") {
        for line in block.lines() {
            let line = line.trim();
            let data = if let Some(rest) = line.strip_prefix("data:") {
                rest.trim()
            } else {
                continue;
            };
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<Value>(data) {
                if v.get("id").is_some() {
                    return Ok(v);
                }
                last = Some(v);
            }
        }
    }
    last.ok_or_else(|| format!("SSE MCP senza JSON-RPC: {text}"))
}

fn id_matches(v: &Value, id: u64) -> bool {
    match v.get("id") {
        Some(Value::Number(n)) => {
            n.as_u64() == Some(id)
                || n.as_i64() == Some(id as i64)
                || n.as_f64().map(|f| f as u64) == Some(id)
        }
        Some(Value::String(s)) => s.parse::<u64>().ok() == Some(id),
        _ => false,
    }
}

pub fn echo_server_config() -> McpServerConfig {
    let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.join("scripts/mcp_echo.py"))
        .unwrap_or_else(|| std::path::PathBuf::from("scripts/mcp_echo.py"));
    McpServerConfig {
        name: "echo".into(),
        transport: "stdio".into(),
        command: "python3".into(),
        args: vec![script.display().to_string()],
        env: Default::default(),
        url: String::new(),
        headers: Default::default(),
        oauth: None,
        watches: Vec::new(),
        enabled: true,
    }
}

pub async fn start_oauth(app: &tauri::AppHandle, hub: &McpHub, name: &str) -> Result<String, String> {
    let cfg = hub
        .list_configs()
        .into_iter()
        .find(|c| c.name == name)
        .ok_or_else(|| format!("server MCP '{name}' non trovato"))?;
    let oauth = cfg
        .oauth
        .ok_or_else(|| "questo server non ha OAuth configurato".to_string())?;
    oauth::authorize(app, name, &oauth).await?;
    hub.refresh_connections().await?;
    Ok(format!("OAuth completato per '{name}'."))
}

pub fn logout(hub: &McpHub, name: &str) -> Result<String, String> {
    clear_tokens(name)?;
    let _ = hub;
    Ok(format!("Token OAuth di '{name}' rimossi."))
}
