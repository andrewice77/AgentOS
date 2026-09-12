//! Generic MCP watches: poll any connected tool and notify on change / upcoming times.

use crate::agent::AppState;
use crate::config::McpWatchConfig;
use notify_rust::Notification;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Default, Serialize, Deserialize)]
struct WatchFile {
    hashes: HashMap<String, u64>,
    upcoming: HashSet<String>,
}

fn state_path() -> PathBuf {
    crate::config::data_dir().join("mcp_watch_state.json")
}

fn load_state() -> WatchFile {
    let path = state_path();
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_state(state: &WatchFile) {
    if let Ok(raw) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(state_path(), raw);
    }
}

fn hash_value(v: &Value) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(v).unwrap_or_default().hash(&mut h);
    h.finish()
}

fn flatten_text(v: &Value, out: &mut String, depth: usize) {
    if depth > 8 || out.len() > 400 {
        return;
    }
    match v {
        Value::String(s) => {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(s.trim());
        }
        Value::Number(n) => {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&n.to_string());
        }
        Value::Array(arr) => {
            for item in arr.iter().take(8) {
                flatten_text(item, out, depth + 1);
            }
        }
        Value::Object(map) => {
            for (k, val) in map.iter().take(12) {
                if matches!(k.as_str(), "type" | "isError" | "jsonrpc") {
                    continue;
                }
                flatten_text(val, out, depth + 1);
            }
        }
        _ => {}
    }
}

fn summarize(v: &Value) -> String {
    let mut s = String::new();
    flatten_text(v, &mut s, 0);
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.is_empty() {
        "Aggiornamento MCP".into()
    } else {
        s.chars().take(220).collect()
    }
}

fn collect_datetimes(v: &Value, out: &mut Vec<(String, i64)>) {
    match v {
        Value::String(s) => {
            if let Some(ts) = parse_datetime(s) {
                out.push((s.clone(), ts));
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_datetimes(item, out);
            }
        }
        Value::Object(map) => {
            for val in map.values() {
                collect_datetimes(val, out);
            }
        }
        _ => {}
    }
}

fn parse_datetime(s: &str) -> Option<i64> {
    let t = s.trim();
    if t.len() < 16 {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(t)
        .ok()
        .map(|d| d.timestamp())
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(t, "%Y-%m-%dT%H:%M:%S")
                .ok()
                .map(|d| d.and_utc().timestamp())
        })
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M")
                .ok()
                .map(|d| d.and_utc().timestamp())
        })
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    crate::debuglog::info(None, format!("mcp-watch: {title} — {body}"));
    let _ = Notification::new()
        .summary(title)
        .body(body)
        .appname("AgentOS")
        .sound_name("message-new-instant")
        .show();
    let _ = app.emit(
        "mcp-watch",
        serde_json::json!({ "title": title, "body": body }),
    );
}

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        let mut last_run: HashMap<String, u64> = HashMap::new();
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
            let Some(state) = app.try_state::<AppState>() else {
                continue;
            };
            if let Err(e) = tick(&app, &state, &mut last_run).await {
                crate::debuglog::warn(None, format!("mcp-watch: {e}"));
            }
        }
    });
}

async fn tick(
    app: &AppHandle,
    state: &AppState,
    last_run: &mut HashMap<String, u64>,
) -> Result<(), String> {
    let cfg = state.config.read().clone();
    let tools = state.mcp.list_tools().await.unwrap_or_default();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut persist = load_state();

    for server in cfg.mcp_servers.iter().filter(|s| s.enabled) {
        for watch in server.watches.iter().filter(|w| w.enabled) {
            let key = format!("{}::{}", server.name, watch.name);
            let interval = watch.interval_secs.max(30);
            if last_run.get(&key).copied().unwrap_or(0) + interval > now {
                continue;
            }
            let Some(tool) = state.mcp.resolve_watch_tool(watch, &tools, &server.name) else {
                continue;
            };
            last_run.insert(key.clone(), now);
            let args = if watch.arguments.is_null() {
                serde_json::json!({})
            } else {
                watch.arguments.clone()
            };
            match state
                .mcp
                .call_tool_trusted(
                    &state.db,
                    &cfg.permissions.default_mcp,
                    &server.name,
                    &tool,
                    args,
                )
                .await
            {
                Ok(result) => {
                    apply_watch(app, &mut persist, watch, &key, &result, now as i64);
                }
                Err(e) => {
                    crate::debuglog::warn(None, format!("mcp-watch {key} fail: {e}"));
                }
            }
        }
    }
    save_state(&persist);
    Ok(())
}

fn apply_watch(
    app: &AppHandle,
    persist: &mut WatchFile,
    watch: &McpWatchConfig,
    key: &str,
    result: &Value,
    now: i64,
) {
    let title = if watch.title.trim().is_empty() {
        format!("MCP · {}", watch.name)
    } else {
        watch.title.clone()
    };

    if watch.notify_on_change {
        let h = hash_value(result);
        let prev = persist.hashes.get(key).copied();
        persist.hashes.insert(key.to_string(), h);
        if prev.is_some() && prev != Some(h) {
            notify(app, &title, &summarize(result));
        }
    }

    if watch.upcoming_minutes > 0 {
        let window = (watch.upcoming_minutes as i64) * 60;
        let mut times = Vec::new();
        collect_datetimes(result, &mut times);
        for (label, ts) in times {
            let delta = ts - now;
            if delta < 0 || delta > window {
                continue;
            }
            let stamp = format!("{key}|{ts}|{label}");
            if persist.upcoming.contains(&stamp) {
                continue;
            }
            persist.upcoming.insert(stamp);
            let mins = (delta / 60).max(0);
            notify(
                app,
                &title,
                &format!("Tra {mins} min: {}", label.chars().take(160).collect::<String>()),
            );
        }
    }
}
