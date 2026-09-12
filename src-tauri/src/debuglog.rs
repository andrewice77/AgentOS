use parking_lot::Mutex;
use serde::Serialize;
use std::collections::VecDeque;
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

const MAX_LINES: usize = 300;

#[derive(Clone, Serialize)]
pub struct DebugLine {
    pub ts: String,
    pub level: String,
    pub message: String,
}

struct DebugBus {
    lines: Mutex<VecDeque<DebugLine>>,
}

fn bus() -> &'static DebugBus {
    static BUS: OnceLock<DebugBus> = OnceLock::new();
    BUS.get_or_init(|| DebugBus {
        lines: Mutex::new(VecDeque::with_capacity(MAX_LINES)),
    })
}

fn now_ts() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // keep it simple/readable for copy-paste
    format!("{secs}")
}

fn append_file(line: &DebugLine) {
    let dir = crate::config::data_dir().join("logs");
    let _ = create_dir_all(&dir);
    let path = dir.join("agentos.log");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "[{}] {}: {}", line.ts, line.level, line.message);
        let _ = f.flush();
    }
}

pub fn log(app: Option<&AppHandle>, level: &str, message: impl Into<String>) {
    let line = DebugLine {
        ts: now_ts(),
        level: level.into(),
        message: message.into(),
    };
    {
        let mut q = bus().lines.lock();
        if q.len() >= MAX_LINES {
            q.pop_front();
        }
        q.push_back(line.clone());
    }
    append_file(&line);
    if let Some(app) = app {
        let _ = app.emit("debug-log", &line);
    }
    match level {
        "error" => tracing::error!("{}", line.message),
        "warn" => tracing::warn!("{}", line.message),
        _ => tracing::info!("{}", line.message),
    }
}

pub fn info(app: Option<&AppHandle>, message: impl Into<String>) {
    log(app, "info", message);
}

pub fn warn(app: Option<&AppHandle>, message: impl Into<String>) {
    log(app, "warn", message);
}

pub fn error(app: Option<&AppHandle>, message: impl Into<String>) {
    log(app, "error", message);
}

pub fn snapshot() -> Vec<DebugLine> {
    bus().lines.lock().iter().cloned().collect()
}

pub fn clear() {
    bus().lines.lock().clear();
}
