use crate::config::MemoryConfig;
use crate::db::{now, Db};
use regex::Regex;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: i64,
    pub store: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingMemory {
    pub id: i64,
    pub store: String,
    pub action: String,
    pub content: Option<String>,
    pub old_text: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySnapshot {
    pub user: Vec<MemoryEntry>,
    pub memory: Vec<MemoryEntry>,
    pub user_chars: usize,
    pub memory_chars: usize,
    pub user_limit: usize,
    pub memory_limit: usize,
}

fn injection_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)(ignore (all )?previous instructions|system prompt|exfiltrat|curl\s+[^ ]+\s*\|)|[\u200B-\u200F\u202A-\u202E]",
        )
        .expect("regex")
    })
}

pub fn scan_memory_content(content: &str) -> Result<(), String> {
    if injection_re().is_match(content) {
        return Err("Memory entry blocked by security scan".into());
    }
    Ok(())
}

pub fn list_store(db: &Db, store: &str) -> Result<Vec<MemoryEntry>, String> {
    let conn = db.lock()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, store, content, created_at, updated_at FROM memories WHERE store = ?1 ORDER BY id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![store], |row| {
            Ok(MemoryEntry {
                id: row.get(0)?,
                store: row.get(1)?,
                content: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn store_chars(entries: &[MemoryEntry]) -> usize {
    entries.iter().map(|e| e.content.chars().count()).sum()
}

pub fn snapshot(db: &Db, cfg: &MemoryConfig) -> Result<MemorySnapshot, String> {
    let user = list_store(db, "user")?;
    let memory = list_store(db, "memory")?;
    Ok(MemorySnapshot {
        user_chars: store_chars(&user),
        memory_chars: store_chars(&memory),
        user_limit: cfg.user_char_limit,
        memory_limit: cfg.memory_char_limit,
        user,
        memory,
    })
}

pub fn render_frozen_prompt(snap: &MemorySnapshot) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "══════════════════════════════════════════════\nUSER PROFILE [{pct:.0}% — {used}/{limit} chars]\n══════════════════════════════════════════════\n",
        pct = (snap.user_chars as f64 / snap.user_limit.max(1) as f64) * 100.0,
        used = snap.user_chars,
        limit = snap.user_limit
    ));
    if snap.user.is_empty() {
        out.push_str("(empty)\n");
    } else {
        out.push_str(
            &snap
                .user
                .iter()
                .map(|e| e.content.as_str())
                .collect::<Vec<_>>()
                .join(" § "),
        );
        out.push('\n');
    }
    out.push_str(&format!(
        "\n══════════════════════════════════════════════\nMEMORY (agent notes) [{pct:.0}% — {used}/{limit} chars]\n══════════════════════════════════════════════\n",
        pct = (snap.memory_chars as f64 / snap.memory_limit.max(1) as f64) * 100.0,
        used = snap.memory_chars,
        limit = snap.memory_limit
    ));
    if snap.memory.is_empty() {
        out.push_str("(empty)\n");
    } else {
        out.push_str(
            &snap
                .memory
                .iter()
                .map(|e| e.content.as_str())
                .collect::<Vec<_>>()
                .join(" § "),
        );
        out.push('\n');
    }
    out
}

fn limit_for(cfg: &MemoryConfig, store: &str) -> usize {
    if store == "user" {
        cfg.user_char_limit
    } else {
        cfg.memory_char_limit
    }
}

pub fn apply_write(
    db: &Db,
    cfg: &MemoryConfig,
    store: &str,
    action: &str,
    content: Option<&str>,
    old_text: Option<&str>,
) -> Result<String, String> {
    if store != "user" && store != "memory" {
        return Err("store must be user|memory".into());
    }
    let entries = list_store(db, store)?;
    let limit = limit_for(cfg, store);
    let used = store_chars(&entries);

    match action {
        "add" => {
            let c = content.ok_or("content required for add")?;
            scan_memory_content(c)?;
            let add_len = c.chars().count();
            if used + add_len > limit {
                return Err(format!(
                    "Memory at {used}/{limit} chars. Adding this entry ({add_len} chars) would exceed the limit. Consolidate with replace/remove, then retry."
                ));
            }
            if entries.iter().any(|e| e.content == c) {
                return Ok("no duplicate added".into());
            }
            let conn = db.lock()?;
            let ts = now();
            conn.execute(
                "INSERT INTO memories (store, content, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
                params![store, c, ts, ts],
            )
            .map_err(|e| e.to_string())?;
            Ok("added".into())
        }
        "replace" => {
            let c = content.ok_or("content required for replace")?;
            let old = old_text.ok_or("old_text required for replace")?;
            scan_memory_content(c)?;
            let matches: Vec<_> = entries
                .iter()
                .filter(|e| e.content.contains(old))
                .collect();
            if matches.is_empty() {
                return Err("no entry matched old_text".into());
            }
            if matches.len() > 1 {
                return Err("old_text matched multiple entries; be more specific".into());
            }
            let target = matches[0];
            let new_used = used - target.content.chars().count() + c.chars().count();
            if new_used > limit {
                return Err(format!(
                    "Replace would exceed limit ({new_used}/{limit}). Shorten content or remove other entries."
                ));
            }
            let conn = db.lock()?;
            conn.execute(
                "UPDATE memories SET content = ?1, updated_at = ?2 WHERE id = ?3",
                params![c, now(), target.id],
            )
            .map_err(|e| e.to_string())?;
            Ok("replaced".into())
        }
        "remove" => {
            let old = old_text.ok_or("old_text required for remove")?;
            let matches: Vec<_> = entries
                .iter()
                .filter(|e| e.content.contains(old))
                .collect();
            if matches.is_empty() {
                return Err("no entry matched old_text".into());
            }
            if matches.len() > 1 {
                return Err("old_text matched multiple entries; be more specific".into());
            }
            let conn = db.lock()?;
            conn.execute("DELETE FROM memories WHERE id = ?1", params![matches[0].id])
                .map_err(|e| e.to_string())?;
            Ok("removed".into())
        }
        _ => Err("action must be add|replace|remove".into()),
    }
}

pub fn stage_write(
    db: &Db,
    store: &str,
    action: &str,
    content: Option<&str>,
    old_text: Option<&str>,
) -> Result<i64, String> {
    if store != "user" && store != "memory" {
        return Err("store must be user|memory".into());
    }
    if !matches!(action, "add" | "replace" | "remove") {
        return Err("action must be add|replace|remove".into());
    }
    if let Some(c) = content {
        scan_memory_content(c)?;
    }
    let conn = db.lock()?;
    conn.execute(
        "INSERT INTO memory_pending (store, action, content, old_text, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![store, action, content, old_text, now()],
    )
    .map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

/// Human-readable dump for "cosa sai di me" style questions.
pub fn render_human_summary(snap: &MemorySnapshot) -> String {
    let mut parts = Vec::new();
    if snap.user.is_empty() && snap.memory.is_empty() {
        return "Non ho ancora nulla in USER/MEMORY. Prova: `ricorda …` oppure `preferisco …`."
            .into();
    }
    if !snap.user.is_empty() {
        parts.push(format!(
            "Profilo (USER) — {}/{} caratteri:\n{}",
            snap.user_chars,
            snap.user_limit,
            snap.user
                .iter()
                .map(|e| format!("• {}", e.content))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    if !snap.memory.is_empty() {
        parts.push(format!(
            "Note (MEMORY) — {}/{} caratteri:\n{}",
            snap.memory_chars,
            snap.memory_limit,
            snap.memory
                .iter()
                .map(|e| format!("• {}", e.content))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    parts.join("\n\n")
}

pub fn list_pending(db: &Db) -> Result<Vec<PendingMemory>, String> {
    let conn = db.lock()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, store, action, content, old_text, created_at FROM memory_pending ORDER BY id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(PendingMemory {
                id: row.get(0)?,
                store: row.get(1)?,
                action: row.get(2)?,
                content: row.get(3)?,
                old_text: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn approve_pending(db: &Db, cfg: &MemoryConfig, id: i64) -> Result<String, String> {
    let pending = {
        let conn = db.lock()?;
        conn.query_row(
            "SELECT store, action, content, old_text FROM memory_pending WHERE id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "pending id not found".to_string())?
    };
    let result = apply_write(
        db,
        cfg,
        &pending.0,
        &pending.1,
        pending.2.as_deref(),
        pending.3.as_deref(),
    )?;
    let conn = db.lock()?;
    conn.execute("DELETE FROM memory_pending WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(result)
}

pub fn reject_pending(db: &Db, id: i64) -> Result<(), String> {
    let conn = db.lock()?;
    conn.execute("DELETE FROM memory_pending WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_entry(db: &Db, id: i64) -> Result<(), String> {
    let conn = db.lock()?;
    conn.execute("DELETE FROM memories WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn update_entry(db: &Db, cfg: &MemoryConfig, id: i64, content: &str) -> Result<(), String> {
    scan_memory_content(content)?;
    let (store, old_content): (String, String) = {
        let conn = db.lock()?;
        conn.query_row(
            "SELECT store, content FROM memories WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| e.to_string())?
    };
    let entries = list_store(db, &store)?;
    let used = store_chars(&entries);
    let limit = limit_for(cfg, &store);
    let new_used = used - old_content.chars().count() + content.chars().count();
    if new_used > limit {
        return Err(format!("Would exceed limit ({new_used}/{limit})"));
    }
    let conn = db.lock()?;
    conn.execute(
        "UPDATE memories SET content = ?1, updated_at = ?2 WHERE id = ?3",
        params![content, now(), id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
