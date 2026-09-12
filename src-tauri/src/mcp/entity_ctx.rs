//! Generic session-scoped entity cache (name ↔ id) from MCP tool results.
//! Domain-agnostic: Zoho projects, finance accounts, CRM deals, etc.

use crate::db::{now, Db};
use parking_lot::RwLock;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub kind: String,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub server: Option<String>,
}

impl Entity {
    pub fn display(&self) -> &str {
        self.name
            .as_deref()
            .or(self.label.as_deref())
            .unwrap_or(self.id.as_str())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EntityCtx {
    pub session_id: String,
    #[serde(default)]
    pub entities: Vec<Entity>,
    /// Convenience mirrors for common Zoho-style follow-ups.
    pub portal_id: Option<String>,
    pub project_id: Option<String>,
}

impl EntityCtx {
    pub fn remember_ids(&mut self, portal_id: Option<&str>, project_id: Option<&str>) {
        if let Some(p) = portal_id.filter(|s| !s.is_empty()) {
            self.portal_id = Some(p.to_string());
            self.upsert_entity(Entity {
                kind: "portal".into(),
                id: p.to_string(),
                name: None,
                label: None,
                server: None,
            });
        }
        if let Some(p) = project_id.filter(|s| !s.is_empty()) {
            self.project_id = Some(p.to_string());
            self.upsert_entity(Entity {
                kind: "project".into(),
                id: p.to_string(),
                name: None,
                label: None,
                server: None,
            });
        }
    }

    pub fn upsert_entity(&mut self, ent: Entity) {
        if ent.id.trim().is_empty() {
            return;
        }
        if let Some(existing) = self
            .entities
            .iter_mut()
            .find(|e| e.kind == ent.kind && e.id == ent.id)
        {
            if ent.name.is_some() {
                existing.name = ent.name;
            }
            if ent.label.is_some() {
                existing.label = ent.label;
            }
            if ent.server.is_some() {
                existing.server = ent.server;
            }
        } else {
            // Cap growth
            if self.entities.len() >= 200 {
                self.entities.remove(0);
            }
            self.entities.push(ent);
        }
    }

    pub fn upsert_many(&mut self, ents: impl IntoIterator<Item = Entity>) {
        for e in ents {
            self.upsert_entity(e);
        }
    }

    /// Soft-match: find entities whose name/label appears in the user text.
    pub fn soft_match(&self, user_text: &str) -> Vec<&Entity> {
        let lower = user_text.to_lowercase();
        let mut hits: Vec<(&Entity, usize)> = Vec::new();
        for e in &self.entities {
            let label = e.display().to_lowercase();
            if label.chars().count() < 2 {
                continue;
            }
            if lower.contains(&label) {
                hits.push((e, label.chars().count()));
            }
        }
        // Prefer longer name matches (more specific).
        hits.sort_by(|a, b| b.1.cmp(&a.1));
        hits.into_iter().map(|(e, _)| e).collect()
    }

    pub fn resolve_id_by_kind(&self, kind: &str, user_text: &str) -> Option<String> {
        self.soft_match(user_text)
            .into_iter()
            .find(|e| e.kind.eq_ignore_ascii_case(kind))
            .map(|e| e.id.clone())
    }

    pub fn resolve_any_named(&self, user_text: &str) -> Option<&Entity> {
        self.soft_match(user_text).into_iter().next()
    }

    /// Prompt snippet listing remembered named entities.
    pub fn prompt_hint(&self) -> String {
        let named: Vec<_> = self
            .entities
            .iter()
            .filter(|e| e.name.as_ref().map(|n| n.len() >= 2).unwrap_or(false))
            .take(24)
            .collect();
        if named.is_empty() {
            return String::new();
        }
        let mut out = String::from(
            "\n\n## Remembered entities (this chat)\n\
             Prefer these IDs when the user refers to a name — do not ask for numeric IDs:\n",
        );
        for e in named {
            out.push_str(&format!(
                "- {} «{}» → id `{}`\n",
                e.kind,
                e.display(),
                e.id
            ));
        }
        out
    }
}

#[derive(Default)]
pub struct EntityCtxStore {
    inner: RwLock<Option<EntityCtx>>,
}

impl EntityCtxStore {
    pub fn get_for_session(&self, db: &Db, session_id: &str) -> Option<EntityCtx> {
        {
            let g = self.inner.read();
            if let Some(c) = g.as_ref().filter(|c| c.session_id == session_id) {
                return Some(c.clone());
            }
        }
        // Cold start / switch: load from SQLite
        if let Ok(Some(ctx)) = load_from_db(db, session_id) {
            *self.inner.write() = Some(ctx.clone());
            return Some(ctx);
        }
        None
    }

    pub fn upsert_ids(
        &self,
        db: &Db,
        session_id: &str,
        portal_id: Option<&str>,
        project_id: Option<&str>,
    ) {
        let mut g = self.inner.write();
        let mut ctx = g
            .take()
            .filter(|c| c.session_id == session_id)
            .unwrap_or(EntityCtx {
                session_id: session_id.to_string(),
                ..Default::default()
            });
        ctx.session_id = session_id.to_string();
        ctx.remember_ids(portal_id, project_id);
        let _ = save_to_db(db, &ctx);
        *g = Some(ctx);
    }

    pub fn ingest(
        &self,
        db: &Db,
        session_id: &str,
        server: Option<&str>,
        result: &Value,
    ) {
        let ents = extract_entities(result, server);
        if ents.is_empty() {
            return;
        }
        let mut g = self.inner.write();
        let mut ctx = g
            .take()
            .filter(|c| c.session_id == session_id)
            .unwrap_or_else(|| {
                load_from_db(db, session_id)
                    .ok()
                    .flatten()
                    .unwrap_or(EntityCtx {
                        session_id: session_id.to_string(),
                        ..Default::default()
                    })
            });
        ctx.session_id = session_id.to_string();
        ctx.upsert_many(ents);
        // Promote latest portal/project if present
        if let Some(p) = ctx
            .entities
            .iter()
            .rev()
            .find(|e| e.kind == "portal")
            .map(|e| e.id.clone())
        {
            ctx.portal_id = Some(p);
        }
        if let Some(p) = ctx
            .entities
            .iter()
            .rev()
            .find(|e| e.kind == "project")
            .map(|e| e.id.clone())
        {
            ctx.project_id = Some(p);
        }
        let _ = save_to_db(db, &ctx);
        *g = Some(ctx);
    }

    pub fn clear(&self) {
        *self.inner.write() = None;
    }
}

fn load_from_db(db: &Db, session_id: &str) -> Result<Option<EntityCtx>, String> {
    let conn = db.lock()?;
    let mut stmt = conn
        .prepare("SELECT payload FROM session_entity_ctx WHERE session_id = ?1")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query(params![session_id])
        .map_err(|e| e.to_string())?;
    if let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let payload: String = row.get(0).map_err(|e| e.to_string())?;
        let mut ctx: EntityCtx =
            serde_json::from_str(&payload).map_err(|e| e.to_string())?;
        ctx.session_id = session_id.to_string();
        Ok(Some(ctx))
    } else {
        Ok(None)
    }
}

fn save_to_db(db: &Db, ctx: &EntityCtx) -> Result<(), String> {
    let payload = serde_json::to_string(ctx).map_err(|e| e.to_string())?;
    let conn = db.lock()?;
    conn.execute(
        "INSERT INTO session_entity_ctx (session_id, payload, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(session_id) DO UPDATE SET payload = excluded.payload, updated_at = excluded.updated_at",
        params![ctx.session_id, payload, now()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Walk arbitrary MCP JSON and pull objects that look like {id, name|title|label}.
pub fn extract_entities(value: &Value, server: Option<&str>) -> Vec<Entity> {
    let mut out = Vec::new();
    walk(value, server, &mut out, 0);
    // Dedup by kind+id keeping the richest name
    let mut map: std::collections::HashMap<(String, String), Entity> =
        std::collections::HashMap::new();
    for e in out {
        let key = (e.kind.clone(), e.id.clone());
        map.entry(key)
            .and_modify(|old| {
                if old.name.is_none() && e.name.is_some() {
                    old.name = e.name.clone();
                }
                if old.label.is_none() && e.label.is_some() {
                    old.label = e.label.clone();
                }
            })
            .or_insert(e);
    }
    map.into_values().collect()
}

fn walk(value: &Value, server: Option<&str>, out: &mut Vec<Entity>, depth: usize) {
    walk_hinted(value, server, None, out, depth);
}

fn walk_hinted(
    value: &Value,
    server: Option<&str>,
    kind_hint: Option<&str>,
    out: &mut Vec<Entity>,
    depth: usize,
) {
    if depth > 8 || out.len() > 120 {
        return;
    }
    match value {
        Value::Array(arr) => {
            for item in arr {
                walk_hinted(item, server, kind_hint, out, depth + 1);
            }
        }
        Value::Object(map) => {
            if let Some(ent) = try_entity_from_object(map, server, kind_hint) {
                out.push(ent);
            }
            for (k, v) in map {
                if v.is_array() || v.is_object() {
                    let hint = kind_from_key(k).or(kind_hint);
                    walk_hinted(v, server, hint, out, depth + 1);
                }
            }
        }
        _ => {}
    }
}

fn kind_from_key(key: &str) -> Option<&str> {
    let k = key.to_lowercase();
    for (needle, kind) in [
        ("portal", "portal"),
        ("project", "project"),
        ("issue", "issue"),
        ("bug", "issue"),
        ("task", "task"),
        ("account", "account"),
        ("deal", "deal"),
        ("campaign", "campaign"),
        ("contact", "contact"),
        ("customer", "customer"),
    ] {
        if k.contains(needle) {
            return Some(kind);
        }
    }
    None
}

fn try_entity_from_object(
    map: &serde_json::Map<String, Value>,
    server: Option<&str>,
    kind_hint: Option<&str>,
) -> Option<Entity> {
    let id = first_str(
        map,
        &[
            "id",
            "ID",
            "project_id",
            "portal_id",
            "issue_id",
            "task_id",
            "account_id",
            "deal_id",
            "campaign_id",
        ],
    )?;
    // Skip very short / non-identifier noise
    if id.chars().count() < 2 {
        return None;
    }
    let name = first_str(
        map,
        &[
            "name",
            "NAME",
            "title",
            "TITLE",
            "project_name",
            "portal_name",
            "display_name",
            "label",
            "subject",
        ],
    );
    let label = first_str(map, &["key", "code", "prefix", "number"]);
    let kind = kind_hint
        .map(|s| s.to_string())
        .unwrap_or_else(|| infer_kind(map, name.as_deref(), &id));
    Some(Entity {
        kind,
        id,
        name,
        label,
        server: server.map(|s| s.to_string()),
    })
}

fn first_str(map: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(v) = map.get(*k) {
            if let Some(s) = v.as_str().map(|s| s.trim().to_string()) {
                if !s.is_empty() {
                    return Some(s);
                }
            }
            if let Some(n) = v.as_i64() {
                return Some(n.to_string());
            }
            if let Some(n) = v.as_u64() {
                return Some(n.to_string());
            }
        }
    }
    None
}

fn infer_kind(
    map: &serde_json::Map<String, Value>,
    name: Option<&str>,
    id: &str,
) -> String {
    let type_hint = first_str(map, &["type", "entity_type", "module", "kind"])
        .unwrap_or_default()
        .to_lowercase();
    for (needle, kind) in [
        ("portal", "portal"),
        ("project", "project"),
        ("issue", "issue"),
        ("bug", "issue"),
        ("task", "task"),
        ("account", "account"),
        ("deal", "deal"),
        ("campaign", "campaign"),
        ("contact", "contact"),
        ("customer", "customer"),
    ] {
        if type_hint.contains(needle) {
            return kind.into();
        }
    }
    if map.contains_key("portal_id") && !map.contains_key("project_id") && name.is_some() {
        return "portal".into();
    }
    if map.contains_key("project_id") || map.contains_key("project_name") {
        return "project".into();
    }
    if map.contains_key("issue_id") {
        return "issue".into();
    }
    if map.contains_key("task_id") {
        return "task".into();
    }
    // Heuristic: long numeric ids from Zoho-style lists without type → project if name-like
    if id.chars().all(|c| c.is_ascii_digit()) && id.len() >= 8 && name.is_some() {
        return "project".into();
    }
    "entity".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_named_projects() {
        let v = json!({
            "projects": [
                {"id": "111", "name": "Alpha"},
                {"id": "222", "name": "Beta Login"}
            ]
        });
        let ents = extract_entities(&v, Some("zoho"));
        assert!(ents.iter().any(|e| e.name.as_deref() == Some("Alpha")));
        let ctx = EntityCtx {
            session_id: "s".into(),
            entities: ents,
            ..Default::default()
        };
        let id = ctx.resolve_id_by_kind("project", "apri i task di Alpha");
        assert_eq!(id.as_deref(), Some("111"));
    }
}
