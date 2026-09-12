use crate::config::data_dir;
use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use std::time::Duration;

pub struct Db {
    pub conn: Mutex<Connection>,
}

impl Db {
    #[allow(dead_code)]
    pub fn open_in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        conn.busy_timeout(Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        // Reuse schema via a throwaway open path is awkward; inline same batch.
        Self::init_schema(conn)
    }

    fn init_schema(conn: Connection) -> Result<Self, String> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode=WAL;
            PRAGMA foreign_keys=ON;
            PRAGMA synchronous=NORMAL;

            CREATE TABLE IF NOT EXISTS meta (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS memories (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              store TEXT NOT NULL CHECK(store IN ('user','memory')),
              content TEXT NOT NULL,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS memory_pending (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              store TEXT NOT NULL,
              action TEXT NOT NULL,
              content TEXT,
              old_text TEXT,
              created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS sessions (
              id TEXT PRIMARY KEY,
              title TEXT NOT NULL,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL,
              provider TEXT NOT NULL,
              model TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS messages (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              session_id TEXT NOT NULL,
              role TEXT NOT NULL,
              content TEXT NOT NULL,
              created_at TEXT NOT NULL,
              FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
              content,
              message_id UNINDEXED,
              session_id UNINDEXED,
              role UNINDEXED
            );

            CREATE TABLE IF NOT EXISTS tasks (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              title TEXT NOT NULL,
              description TEXT NOT NULL DEFAULT '',
              status TEXT NOT NULL DEFAULT 'open',
              priority INTEGER NOT NULL DEFAULT 2,
              deadline TEXT,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS events (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              type TEXT NOT NULL,
              schedule TEXT NOT NULL,
              message TEXT NOT NULL,
              enabled INTEGER NOT NULL DEFAULT 1,
              fired INTEGER NOT NULL DEFAULT 0,
              created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS action_ledger (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              timestamp TEXT NOT NULL,
              tool TEXT NOT NULL,
              permission TEXT NOT NULL,
              status TEXT NOT NULL,
              summary TEXT NOT NULL,
              detail_json TEXT NOT NULL DEFAULT '{}'
            );

            CREATE TABLE IF NOT EXISTS session_entity_ctx (
              session_id TEXT PRIMARY KEY,
              payload TEXT NOT NULL,
              updated_at TEXT NOT NULL,
              FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS labels (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              name TEXT NOT NULL,
              kind TEXT NOT NULL DEFAULT 'tag'
                CHECK(kind IN ('project','client','area','tag')),
              color TEXT NOT NULL DEFAULT '#5b8def',
              created_at TEXT NOT NULL,
              UNIQUE(name, kind)
            );

            CREATE TABLE IF NOT EXISTS task_labels (
              task_id INTEGER NOT NULL,
              label_id INTEGER NOT NULL,
              PRIMARY KEY (task_id, label_id),
              FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE,
              FOREIGN KEY(label_id) REFERENCES labels(id) ON DELETE CASCADE
            );
            "#,
        )
        .map_err(|e| e.to_string())?;
        migrate_organizer_columns(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn open() -> Result<Self, String> {
        let path = data_dir().join("database.sqlite");
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.busy_timeout(Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        Self::init_schema(conn)
    }

    pub fn lock(&self) -> Result<parking_lot::MutexGuard<'_, Connection>, String> {
        self.conn
            .try_lock_for(Duration::from_secs(5))
            .ok_or_else(|| "database lock timeout (5s)".to_string())
    }

    #[allow(dead_code)]
    pub fn with_conn<F, T>(&self, f: F) -> Result<T, String>
    where
        F: FnOnce(&Connection) -> Result<T, String>,
    {
        let conn = self.lock()?;
        f(&conn)
    }
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?;
    for r in rows {
        if r.map_err(|e| e.to_string())? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn migrate_organizer_columns(conn: &Connection) -> Result<(), String> {
    if !table_has_column(conn, "events", "task_id")? {
        conn.execute("ALTER TABLE events ADD COLUMN task_id INTEGER", [])
            .map_err(|e| e.to_string())?;
    }
    if !table_has_column(conn, "events", "recurrence")? {
        conn.execute(
            "ALTER TABLE events ADD COLUMN recurrence TEXT NOT NULL DEFAULT 'none'",
            [],
        )
        .map_err(|e| e.to_string())?;
    }
    // Ensure entity ctx table exists for DBs created before this feature.
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS session_entity_ctx (
          session_id TEXT PRIMARY KEY,
          payload TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
        );
        "#,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn now() -> String {
    Utc::now().to_rfc3339()
}

pub fn insert_ledger(
    conn: &Connection,
    tool: &str,
    permission: &str,
    status: &str,
    summary: &str,
    detail: &serde_json::Value,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO action_ledger (timestamp, tool, permission, status, summary, detail_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            now(),
            tool,
            permission,
            status,
            summary,
            detail.to_string()
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
