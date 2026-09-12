use crate::db::{now, Db};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRow {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub message_id: i64,
    pub session_id: String,
    pub role: String,
    pub content: String,
}

pub fn create_session(db: &Db, provider: &str, model: &str, title: &str) -> Result<Session, String> {
    let id = Uuid::new_v4().to_string();
    let ts = now();
    let conn = db.lock()?;
    conn.execute(
        "INSERT INTO sessions (id, title, created_at, updated_at, provider, model)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, title, ts, ts, provider, model],
    )
    .map_err(|e| e.to_string())?;
    Ok(Session {
        id,
        title: title.into(),
        created_at: ts.clone(),
        updated_at: ts,
        provider: provider.into(),
        model: model.into(),
    })
}

pub fn list_sessions(db: &Db) -> Result<Vec<Session>, String> {
    let conn = db.lock()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, title, created_at, updated_at, provider, model FROM sessions ORDER BY updated_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Session {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                provider: row.get(4)?,
                model: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn add_message(db: &Db, session_id: &str, role: &str, content: &str) -> Result<i64, String> {
    let conn = db.lock()?;
    let ts = now();
    conn.execute(
        "INSERT INTO messages (session_id, role, content, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![session_id, role, content, ts],
    )
    .map_err(|e| e.to_string())?;
    let id = conn.last_insert_rowid();
    // FTS is best-effort — never block chat on index failure
    let _ = conn.execute(
        "INSERT INTO messages_fts (content, message_id, session_id, role) VALUES (?1, ?2, ?3, ?4)",
        params![content, id, session_id, role],
    );
    conn.execute(
        "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
        params![ts, session_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(id)
}

pub fn list_messages(db: &Db, session_id: &str) -> Result<Vec<MessageRow>, String> {
    let conn = db.lock()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, role, content, created_at FROM messages
             WHERE session_id = ?1 ORDER BY id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            Ok(MessageRow {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                created_at: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

/// Build a safe FTS5 MATCH query from free text (Italian/English tokens).
pub fn fts_query(raw: &str) -> String {
    let tokens: Vec<String> = raw
        .split_whitespace()
        .map(|t| {
            t.chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .collect::<String>()
        })
        .filter(|t| t.chars().count() >= 2)
        .map(|t| format!("\"{t}\""))
        .collect();
    if tokens.is_empty() {
        // last resort: quote whole cleaned string
        let cleaned: String = raw
            .chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace())
            .collect();
        let cleaned = cleaned.trim();
        if cleaned.is_empty() {
            return "\"__nomatch__\"".into();
        }
        return format!("\"{}\"", cleaned.replace('"', ""));
    }
    tokens.join(" OR ")
}

fn search_like(db: &Db, query: &str, limit: i64) -> Result<Vec<SearchHit>, String> {
    let conn = db.lock()?;
    let pattern = format!("%{}%", query.trim());
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, role, content FROM messages
             WHERE content LIKE ?1 ESCAPE '\\'
             ORDER BY id DESC LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![pattern, limit], |row| {
            Ok(SearchHit {
                message_id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn session_search(db: &Db, query: &str, limit: i64) -> Result<Vec<SearchHit>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(vec![]);
    }
    let match_q = fts_query(q);
    let fts_hits = {
        let conn = db.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT message_id, session_id, role, content FROM messages_fts
                 WHERE messages_fts MATCH ?1 ORDER BY rank LIMIT ?2",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![match_q, limit], |row| {
            Ok(SearchHit {
                message_id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
            })
        });
        match rows {
            Ok(iter) => iter.collect::<Result<Vec<_>, _>>().unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    };
    if !fts_hits.is_empty() {
        return Ok(fts_hits);
    }
    // Fallback: LIKE over messages (also covers FTS lag / special queries)
    search_like(db, q, limit)
}

/// Reindex FTS from messages table (idempotent best-effort).
pub fn rebuild_fts(db: &Db) -> Result<usize, String> {
    let conn = db.lock()?;
    let _ = conn.execute("DELETE FROM messages_fts", []);
    let items: Vec<(i64, String, String, String)> = {
        let mut stmt = conn
            .prepare("SELECT id, session_id, role, content FROM messages ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    let mut n = 0usize;
    for (id, sid, role, content) in items {
        conn.execute(
            "INSERT INTO messages_fts (content, message_id, session_id, role) VALUES (?1, ?2, ?3, ?4)",
            params![content, id, sid, role],
        )
        .map_err(|e| e.to_string())?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::fts_query;

    #[test]
    fn fts_query_tokenizes() {
        let q = fts_query("ciao AgentOS progetto!");
        assert!(q.contains("\"ciao\""));
        assert!(q.contains("\"AgentOS\"") || q.contains("\"agentos\"") || q.contains("AgentOS"));
        assert!(q.contains("OR"));
    }

    #[test]
    fn fts_query_empty_safe() {
        let q = fts_query("   ");
        assert!(!q.is_empty());
    }
}
