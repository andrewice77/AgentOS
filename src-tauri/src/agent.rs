use crate::config::{session_badge, AppConfig};
use crate::web_search::{self, WebCommand};
use crate::db::{insert_ledger, Db};
use crate::llm::{self, ChatMessage};
use crate::memory::{self, MemorySnapshot};
use crate::mcp::McpHub;
use crate::organizer;
use crate::sessions;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    pub config: Arc<RwLock<AppConfig>>,
    pub mcp: Arc<McpHub>,
    pub frozen_memory: Arc<RwLock<Option<MemorySnapshot>>>,
    pub active_session: Arc<RwLock<Option<String>>>,
    pub last_web: Arc<RwLock<Option<crate::web_search::WebThread>>>,
    /// Remembered soft entities (name→id) for the active chat — generic MCP cache.
    pub zoho_ctx: Arc<crate::mcp::zoho_ctx::ZohoCtxStore>,
    /// Interactive multiple-choice question waiting for a click / typed answer.
    pub pending_question: Arc<RwLock<Option<PendingQuestion>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    pub message: String,
    pub session_id: Option<String>,
}

/// Structured choice prompt shown as clickable chips in the chat UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingQuestion {
    pub id: String,
    pub question: String,
    pub options: Vec<String>,
    #[serde(default)]
    pub allow_free_text: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub session_id: String,
    pub reply: String,
    pub badge: String,
    pub pending_memory: Vec<memory::PendingMemory>,
    /// MCP tool call waiting for Approva / Rifiuta in the chat UI.
    #[serde(default)]
    pub pending_mcp: Option<crate::mcp::PendingMcpCall>,
    /// Multiple-choice question waiting for a click.
    #[serde(default)]
    pub pending_question: Option<PendingQuestion>,
    /// True when Rust already streamed sentence TTS (piper/audio8) — UI must not re-speak.
    #[serde(default)]
    pub tts_streamed: bool,
}

fn make_chat_response(
    state: &AppState,
    session_id: String,
    reply: String,
    badge: String,
    tts_streamed: bool,
) -> Result<ChatResponse, String> {
    Ok(ChatResponse {
        session_id,
        reply,
        badge,
        pending_memory: memory::list_pending(&state.db)?,
        pending_mcp: state.mcp.peek_pending(),
        pending_question: state.pending_question.read().clone(),
        tts_streamed,
    })
}

/// Parse ```ask-question … ``` fence; returns (question, reply without fence).
pub fn extract_ask_question(text: &str) -> Option<(PendingQuestion, String)> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"(?is)```\s*ask-question\s*\n(.*?)```").expect("ask-question regex")
    });
    let caps = re.captures(text)?;
    let raw = caps.get(1)?.as_str().trim();
    let v: Value = serde_json::from_str(raw).ok()?;
    let question = v
        .get("question")
        .or_else(|| v.get("prompt"))
        .and_then(|x| x.as_str())?
        .trim()
        .to_string();
    if question.is_empty() {
        return None;
    }
    let options: Vec<String> = v
        .get("options")
        .or_else(|| v.get("choices"))
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|o| o.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    if options.len() < 2 {
        return None;
    }
    let allow_free_text = v
        .get("allow_free_text")
        .or_else(|| v.get("allowFreeText"))
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    let id = v
        .get("id")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("q-{}", chrono_lite_id()));
    let cleaned = re.replace(text, "").trim().to_string();
    let cleaned = if cleaned.is_empty() {
        question.clone()
    } else {
        cleaned
    };
    Some((
        PendingQuestion {
            id,
            question,
            options,
            allow_free_text,
        },
        cleaned,
    ))
}

fn chrono_lite_id() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// If the model asked a structured question, store it and rewrite the reply.
fn apply_ask_question(state: &AppState, reply: &mut String) {
    if let Some((q, cleaned)) = extract_ask_question(reply) {
        *state.pending_question.write() = Some(q);
        *reply = cleaned;
    }
}

#[cfg(test)]
mod ask_question_tests {
    use super::*;

    #[test]
    fn parses_ask_question_fence() {
        let text = r#"Scegli un’opzione:
```ask-question
{"question":"Quale ambiente?","options":["Dev","Prod"],"allow_free_text":false}
```"#;
        let (q, cleaned) = extract_ask_question(text).expect("parse");
        assert_eq!(q.question, "Quale ambiente?");
        assert_eq!(q.options, vec!["Dev", "Prod"]);
        assert!(!q.allow_free_text);
        assert!(cleaned.contains("Scegli"));
        assert!(!cleaned.contains("ask-question"));
    }
}

pub fn refresh_frozen_memory(state: &AppState) -> Result<(), String> {
    let cfg = state.config.read().clone();
    let snap = memory::snapshot(&state.db, &cfg.memory)?;
    *state.frozen_memory.write() = Some(snap);
    Ok(())
}

fn system_prompt(snap: &MemorySnapshot, cfg: &AppConfig) -> String {
    let mut base = format!(
        "You are AgentOS, a local-first personal desktop assistant on Linux.\n\
         Be concise, practical, and honest about uncertainty.\n\
         You can help with organization, memory, and tools.\n\
         The USER/MEMORY block below is authoritative durable memory already approved by the user.\n\
         Prefer it over guesses when answering personal questions.\n\
         Built-in commands the app handles outside the model: \
         `ricorda …`, `preferisco …`, `dimentica …`, `cosa sai di me`, `cerca: …`, \
         `task: …`, `salva un task: …`, `ricorda di …`, `lista task`, `completa task N`, \
         `in corso task N`, `cosa ho fatto` / `task completati`, \
         `aggiungi al cliente Rossi: …`, `task per Rossi`, `lista etichette`, \
         `ricordami tra 90 secondi di …`, `ricordami tra 5 minuti di …`, \
         `ricordami domani alle 9 di …`, \
         `promemoria: YYYY-MM-DD HH:MM | testo`, \
         `mcp list`, `mcp call server/tool {{…}}`, `approva mcp`.\n\
         When MCP tools are listed below, you may call them yourself with an mcp-call block — \
         do not tell the user to type mcp call unless they ask how.\n\
         When you need the user to pick among a few clear options (not open-ended prose), \
         end your reply with an ask-question fence so the UI shows clickable choices:\n\
         ```ask-question\n\
         {{\"question\":\"…\",\"options\":[\"A\",\"B\",\"C\"],\"allow_free_text\":true}}\n\
         ```\n\
         Use short option labels. Prefer this over listing A/B/C only in prose.\n\
         Never claim you created a reminder, task, or memory entry. \
         Those are applied only by the app; if a request reached you, it was not saved — \
         tell the user a supported phrasing instead of pretending.\n\
         If the user prefixes a question with `cerca online:` / `cerca sul web:`, \
         the app injects a WEB EVIDENCE block from the live web — use it and cite links.\n\
         Follow-up turns in the same chat may also include a fresh WEB EVIDENCE block.\n\
         Public LinkedIn/GitHub/site URLs in WEB EVIDENCE must be shared when asked; \
         never invent them, never refuse them as 'private'.\n\n\
         {}",
        memory::render_frozen_prompt(snap)
    );
    if cfg.skills.enabled && cfg.skills.inject_prompt {
        base.push_str(&crate::skills::prompt_block());
    }
    base
}

/// Heuristic tool routing for MVP (works reliably on 7–8B models).
async fn maybe_handle_tools(
    app: &AppHandle,
    state: &AppState,
    user_message: &str,
) -> Result<Option<String>, String> {
    let lower = user_message.to_lowercase();
    let cfg = state.config.read().clone();

    // --- Recall: what do you know about me ---
    if lower == "cosa sai di me"
        || lower == "cosa sai di me?"
        || lower == "cosa ricordi"
        || lower == "cosa ricordi?"
        || lower.starts_with("cosa sai di me")
        || lower.starts_with("cosa ricordi")
        || lower.starts_with("what do you know about me")
        || lower.starts_with("show memory")
    {
        let snap = memory::snapshot(&state.db, &cfg.memory)?;
        return Ok(Some(memory::render_human_summary(&snap)));
    }

    // --- Remember / preferisco → USER or MEMORY ---
    let remember = !organizer::looks_like_reminder_intent(user_message)
        && !organizer::looks_like_task_create(user_message)
        && (lower.starts_with("ricorda ")
            || lower.starts_with("remember ")
            || lower.starts_with("preferisco ")
            || lower.starts_with("i prefer ")
            || lower.starts_with("profilo: ")
            || lower.contains("salva nel profilo")
            || lower.contains("aggiungi al profilo")
            || lower.contains("nelle mie preferenze")
            || (lower.contains("preferisco")
                && (lower.contains("ricord") || lower.contains("salva"))));
    if remember {
        let content = if lower.starts_with("profilo: ") {
            user_message.split_once(':').map(|(_, r)| r.trim()).unwrap_or("").to_string()
        } else {
            user_message
                .split_once(' ')
                .map(|(_, rest)| rest.trim())
                .unwrap_or("")
                .to_string()
        };
        if content.is_empty() {
            return Ok(Some("Cosa vuoi che ricordi?".into()));
        }
        memory::scan_memory_content(&content)?;
        let store = if lower.starts_with("preferisco ")
            || lower.starts_with("i prefer ")
            || lower.starts_with("profilo: ")
            || lower.contains("prefer")
            || lower.contains("preferisc")
            || lower.contains("nel profilo")
            || lower.contains("al profilo")
        {
            "user"
        } else {
            "memory"
        };
        {
            let conn = state.db.lock()?;
            insert_ledger(
                &conn,
                "memory",
                "safe",
                "proposed",
                "memory write",
                &json!({"store": store, "action": "add", "content": content}),
            )?;
        }
        if cfg.memory.write_approval {
            let id = memory::stage_write(&state.db, store, "add", Some(&content), None)?;
            let _ = app.emit("memory-changed", id);
            return Ok(Some(format!(
                "Ho preparato un salvataggio in {store}. Approva la richiesta #{id} nel pannello Memoria, oppure usa il pulsante Approva in chat."
            )));
        } else {
            memory::apply_write(&state.db, &cfg.memory, store, "add", Some(&content), None)?;
            refresh_frozen_memory(state)?;
            let _ = app.emit("memory-changed", json!({"store": store}));
            return Ok(Some(format!("Salvato in {store}.")));
        }
    }

    // --- Forget ---
    if lower.starts_with("dimentica ") || lower.starts_with("forget ") {
        let needle = user_message
            .split_once(' ')
            .map(|(_, rest)| rest.trim())
            .unwrap_or("")
            .to_string();
        if needle.is_empty() {
            return Ok(Some("Cosa vuoi che dimentichi? Indica una parte del testo in memoria.".into()));
        }
        {
            let conn = state.db.lock()?;
            insert_ledger(
                &conn,
                "memory",
                "safe",
                "proposed",
                "memory remove",
                &json!({"action": "remove", "old_text": needle}),
            )?;
        }
        // Try MEMORY first, then USER
        let store = {
            let snap = memory::snapshot(&state.db, &cfg.memory)?;
            if snap.memory.iter().any(|e| e.content.contains(&needle)) {
                "memory"
            } else if snap.user.iter().any(|e| e.content.contains(&needle)) {
                "user"
            } else {
                return Ok(Some(format!(
                    "Nessuna entry contiene «{needle}». Controlla il pannello Memoria."
                )));
            }
        };
        if cfg.memory.write_approval {
            let id = memory::stage_write(&state.db, store, "remove", None, Some(&needle))?;
            let _ = app.emit("memory-changed", id);
            return Ok(Some(format!(
                "Ho preparato la rimozione da {store}. Approva #{id} con il pulsante in chat o nel pannello Memoria."
            )));
        } else {
            memory::apply_write(&state.db, &cfg.memory, store, "remove", None, Some(&needle))?;
            refresh_frozen_memory(state)?;
            let _ = app.emit("memory-changed", json!({"store": store}));
            return Ok(Some(format!("Rimosso da {store}.")));
        }
    }

    // --- Approve pending by id from chat ---
    if lower.starts_with("approva memoria ") || lower.starts_with("approve memory ") {
        let id_str = user_message
            .split_whitespace()
            .last()
            .unwrap_or("");
        let id: i64 = id_str
            .parse()
            .map_err(|_| "Uso: approva memoria <id>".to_string())?;
        let r = memory::approve_pending(&state.db, &cfg.memory, id)?;
        refresh_frozen_memory(state)?;
        let _ = app.emit("memory-changed", id);
        return Ok(Some(format!("Memoria approvata (#{id}): {r}.")));
    }
    if lower.starts_with("rifiuta memoria ") || lower.starts_with("reject memory ") {
        let id_str = user_message.split_whitespace().last().unwrap_or("");
        let id: i64 = id_str
            .parse()
            .map_err(|_| "Uso: rifiuta memoria <id>".to_string())?;
        memory::reject_pending(&state.db, id)?;
        let _ = app.emit("memory-changed", id);
        return Ok(Some(format!("Richiesta memoria #{id} rifiutata.")));
    }

    // --- Session search ---
    if web_search::parse_command(user_message).is_some() {
        return Ok(None);
    }
    let search_prefixes = [
        "cerca nelle sessioni:",
        "cerca nelle conversazioni:",
        "cerca:",
        "search sessions:",
        "search:",
    ];
    let is_search = search_prefixes.iter().any(|p| lower.starts_with(p))
        || lower.contains("cerca nelle sessioni")
        || lower.contains("cerca nelle conversazioni");
    if is_search {
        let q = if let Some((_, rest)) = user_message.split_once(':') {
            rest.trim().to_string()
        } else if let Some((_, rest)) = lower.split_once("sessions ") {
            // keep original casing from message roughly
            user_message
                .get(user_message.len().saturating_sub(rest.len())..)
                .unwrap_or(rest)
                .trim()
                .to_string()
        } else {
            user_message.to_string()
        };
        let q = q.trim();
        if q.is_empty() || q.eq_ignore_ascii_case("cerca nelle sessioni") {
            return Ok(Some("Uso: `cerca: parola` oppure `cerca nelle sessioni: tema`.".into()));
        }
        let hits = sessions::session_search(&state.db, q, 8)?;
        if hits.is_empty() {
            return Ok(Some(format!("Nessun risultato per «{q}».")));
        }
        let body = hits
            .iter()
            .map(|h| {
                format!(
                    "- [{}] {}",
                    h.role,
                    h.content.chars().take(180).collect::<String>()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Ok(Some(format!("Risultati session search per «{q}»:\n{body}")));
    }

    // --- Tasks ---
    if lower == "lista task"
        || lower == "lista tasks"
        || lower == "i miei task"
        || lower == "cosa ho da fare"
        || lower == "todo list"
        || lower.starts_with("lista task")
    {
        return Ok(Some(organizer::render_task_list(&state.db)?));
    }

    if lower.starts_with("task per ")
        || lower.starts_with("task del cliente ")
        || lower.starts_with("task del progetto ")
        || lower.starts_with("task con ")
        || lower.starts_with("mostra task ")
    {
        let q = lower
            .replacen("task per ", "", 1)
            .replacen("task del cliente ", "", 1)
            .replacen("task del progetto ", "", 1)
            .replacen("task con ", "", 1)
            .replacen("mostra task ", "", 1)
            .trim()
            .to_string();
        if q.is_empty() {
            return Ok(Some(
                "Specifica etichetta/cliente/progetto. Es: `task per Rossi` o `task del progetto sito`."
                    .into(),
            ));
        }
        return Ok(Some(organizer::tasks_for_label_query(&state.db, &q)?));
    }

    if lower == "lista etichette"
        || lower == "lista label"
        || lower == "le mie etichette"
        || lower.starts_with("lista etichette")
    {
        let labs = organizer::list_labels(&state.db)?;
        if labs.is_empty() {
            return Ok(Some(
                "Nessuna etichetta ancora. Creane una con `task: … cliente Rossi` o dal pannello Organizer."
                    .into(),
            ));
        }
        let body = labs
            .iter()
            .map(|l| format!("#{} · {}:{}", l.id, l.kind, l.name))
            .collect::<Vec<_>>()
            .join("\n");
        return Ok(Some(format!("Etichette:\n{body}")));
    }

    if lower == "cosa ho fatto"
        || lower == "cosa ho completato"
        || lower.starts_with("cosa ho fatto")
        || lower.starts_with("task completati")
        || lower.starts_with("lista completati")
        || lower.starts_with("completati")
    {
        let days = if lower.contains("settimana") {
            7
        } else if lower.contains("mese") || lower.contains("scorso mese") {
            30
        } else if lower.contains("anno") {
            365
        } else {
            30
        };
        return Ok(Some(organizer::render_completed_tasks(&state.db, days)?));
    }

    if lower.starts_with("completa task ")
        || lower.starts_with("fatto task ")
        || lower.starts_with("completa #")
        || lower.starts_with("done task ")
    {
        let id_str = user_message
            .split_whitespace()
            .last()
            .unwrap_or("")
            .trim_start_matches('#');
        let id: i64 = id_str
            .parse()
            .map_err(|_| "Uso: completa task <id>".to_string())?;
        organizer::set_task_status(&state.db, id, "done")?;
        let conn = state.db.lock()?;
        insert_ledger(
            &conn,
            "task",
            "safe",
            "executed",
            &format!("completed task #{id}"),
            &json!({"id": id}),
        )?;
        let _ = app.emit("tasks-changed", id);
        return Ok(Some(format!("Task #{id} completato.")));
    }

    if lower.starts_with("in corso task ")
        || lower.starts_with("task in corso ")
        || lower.starts_with("doing task ")
    {
        let id_str = user_message
            .split_whitespace()
            .last()
            .unwrap_or("")
            .trim_start_matches('#');
        let id: i64 = id_str
            .parse()
            .map_err(|_| "Uso: in corso task <id>".to_string())?;
        organizer::set_task_status(&state.db, id, "doing")?;
        let _ = app.emit("tasks-changed", id);
        return Ok(Some(format!("Task #{id} impostato in corso.")));
    }

    if lower.starts_with("elimina task ") || lower.starts_with("delete task ") {
        let id_str = user_message
            .split_whitespace()
            .last()
            .unwrap_or("")
            .trim_start_matches('#');
        let id: i64 = id_str
            .parse()
            .map_err(|_| "Uso: elimina task <id>".to_string())?;
        organizer::delete_task(&state.db, id)?;
        let _ = app.emit("tasks-changed", id);
        return Ok(Some(format!("Task #{id} eliminato.")));
    }

    match organizer::parse_task_create(user_message) {
        organizer::TaskCreate::NeedTitle => {
            return Ok(Some(
                "Cosa vuoi salvare come task? Es: `salva un task: comprare il latte` oppure `task: titolo`."
                    .into(),
            ));
        }
        organizer::TaskCreate::Ready {
            title,
            priority,
            deadline,
            labels,
        } => {
            let id = organizer::create_task(
                &state.db,
                &title,
                "",
                priority,
                deadline.as_deref(),
                &[],
                &labels,
            )?;
            let conn = state.db.lock()?;
            insert_ledger(
                &conn,
                "task",
                "safe",
                "executed",
                &format!("created task #{id}"),
                &json!({
                    "title": title,
                    "priority": priority,
                    "deadline": deadline,
                    "labels": labels.iter().map(|l| format!("{}:{}", l.kind, l.name)).collect::<Vec<_>>(),
                }),
            )?;
            let _ = app.emit("tasks-changed", id);
            let due = deadline
                .as_deref()
                .map(|d| format!(" · entro {}", organizer::format_schedule_human(d)))
                .unwrap_or_default();
            let labs = if labels.is_empty() {
                String::new()
            } else {
                let body = labels
                    .iter()
                    .map(|l| format!("{}:{}", l.kind, l.name))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(" · [{body}]")
            };
            return Ok(Some(format!(
                "Task #{id} creato: {title}{due}{labs}. Lo trovi nel pannello Organizer — non serve approvazione."
            )));
        }
        organizer::TaskCreate::NotATask => {}
    }

    // --- Reminders (NL, including conversational phrasing) ---
    if organizer::looks_like_reminder_intent(user_message) {
        match organizer::parse_reminder_nl(user_message) {
            Ok(parsed) => {
                let id = organizer::create_reminder_parsed(&state.db, &parsed)?;
                let conn = state.db.lock()?;
                insert_ledger(
                    &conn,
                    "reminder",
                    "safe",
                    "executed",
                    &format!("created reminder #{id}"),
                    &json!({
                        "schedule": parsed.schedule_rfc3339,
                        "human": parsed.human,
                        "message": parsed.message
                    }),
                )?;
                let _ = app.emit("reminders-changed", id);
                return Ok(Some(format!(
                    "Reminder #{id} creato per {} — «{}». Ti avviserò a tempo debito.",
                    parsed.human, parsed.message
                )));
            }
            Err(e) => return Ok(Some(e)),
        }
    }

    if lower == "lista reminder"
        || lower == "lista promemoria"
        || lower == "i miei promemoria"
        || lower.starts_with("lista reminder")
        || lower.starts_with("lista promemoria")
    {
        let all = organizer::list_reminders(&state.db)?;
        let pending: Vec<_> = all.iter().filter(|r| !r.fired && r.enabled).collect();
        if pending.is_empty() {
            return Ok(Some(
                "Nessun promemoria in programma. Prova: `ricordami tra 2 minuti di test`.".into(),
            ));
        }
        let body = pending
            .iter()
            .map(|r| {
                format!(
                    "#{id} · {when} — {msg}",
                    id = r.id,
                    when = organizer::format_schedule_human(&r.schedule),
                    msg = r.message
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Ok(Some(format!("Promemoria attivi:\n{body}")));
    }

    if lower.starts_with("elimina reminder ")
        || lower.starts_with("elimina promemoria ")
        || lower.starts_with("delete reminder ")
    {
        let id_str = user_message
            .split_whitespace()
            .last()
            .unwrap_or("")
            .trim_start_matches('#');
        let id: i64 = id_str
            .parse()
            .map_err(|_| "Uso: elimina promemoria <id>".to_string())?;
        organizer::delete_reminder(&state.db, id)?;
        let _ = app.emit("reminders-changed", id);
        return Ok(Some(format!("Promemoria #{id} eliminato.")));
    }

    // --- MCP ---
    if lower == "mcp list"
        || lower == "lista mcp"
        || lower == "lista tool mcp"
        || lower.starts_with("mcp status")
    {
        let status = state.mcp.status().await?;
        if status.servers.is_empty() {
            return Ok(Some(
                "Nessun server MCP. In tab MCP premi «Installa echo smoke» oppure configura un server stdio."
                    .into(),
            ));
        }
        let mut lines = Vec::new();
        for s in &status.servers {
            let st = if s.connected {
                "connected"
            } else if let Some(e) = &s.error {
                e.as_str()
            } else {
                "offline"
            };
            lines.push(format!("• {} [{}] — {}", s.name, st, s.command));
        }
        if status.tools.is_empty() {
            lines.push("Tool: (nessuno — prova refresh)".into());
        } else {
            lines.push("Tool:".into());
            for t in &status.tools {
                lines.push(format!("  - {}/{} — {}", t.server, t.name, t.description));
            }
        }
        if let Some(p) = state.mcp.peek_pending() {
            lines.push(format!(
                "In attesa approvazione: {}/{} (livello {})",
                p.server, p.tool, p.level
            ));
        }
        return Ok(Some(lines.join("\n")));
    }

    if lower == "approva mcp" || lower == "approve mcp" {
        // Handled in handle_chat (resume agent loop). Keep a fallback if called alone.
        return Ok(None);
    }

    if lower == "rifiuta mcp" || lower == "reject mcp" {
        let _ = state.mcp.take_pending();
        return Ok(Some("Chiamata MCP rifiutata.".into()));
    }

    if lower.starts_with("mcp call ") || lower.starts_with("usa mcp ") {
        // Formats:
        // mcp call server/tool {"a":1}
        // mcp call server tool {"a":1}
        let rest = if lower.starts_with("mcp call ") {
            user_message.get("mcp call ".len()..).unwrap_or("").trim()
        } else {
            user_message.get("usa mcp ".len()..).unwrap_or("").trim()
        };
        if rest.is_empty() {
            return Ok(Some(
                "Uso: `mcp call echo/echo {\"text\":\"ciao\"}`".into(),
            ));
        }
        let (server, tool, args_raw) = if let Some((left, jsonish)) = rest.split_once('{') {
            let json = format!("{{{jsonish}");
            let left = left.trim();
            if let Some((s, t)) = left.split_once('/') {
                (s.trim().to_string(), t.trim().to_string(), json)
            } else {
                let mut parts = left.split_whitespace();
                let s = parts.next().unwrap_or("").to_string();
                let t = parts.next().unwrap_or("").to_string();
                (s, t, json)
            }
        } else if let Some((s, rest)) = rest.split_once('/') {
            // Prefer longest form: zoho-projects/ZohoProjects_get_portals
            // Avoid parsing literal "server/tool …" from help text examples.
            let mut parts = rest.split_whitespace();
            let tool = parts.next().unwrap_or("").to_string();
            let args = parts.collect::<Vec<_>>().join(" ");
            let server = s.trim().to_string();
            if server.eq_ignore_ascii_case("server") && tool.eq_ignore_ascii_case("tool") {
                // User typed the template: mcp call server/tool zoho-projects/RealTool
                let rem = args.trim();
                if let Some((real_s, real_rest)) = rem.split_once('/') {
                    let mut rp = real_rest.split_whitespace();
                    let real_t = rp.next().unwrap_or("").to_string();
                    let real_args = rp.collect::<Vec<_>>().join(" ");
                    (real_s.trim().to_string(), real_t, real_args)
                } else {
                    (server, tool, args)
                }
            } else {
                (server, tool, args)
            }
        } else {
            let mut parts = rest.split_whitespace();
            let s = parts.next().unwrap_or("").to_string();
            let t = parts.next().unwrap_or("").to_string();
            let args = parts.collect::<Vec<_>>().join(" ");
            (s, t, args)
        };
        if server.is_empty() || tool.is_empty() {
            return Ok(Some(
                "Uso: `mcp call echo/echo {\"text\":\"ciao\"}` oppure `mcp call echo echo {\"text\":\"ciao\"}`."
                    .into(),
            ));
        }
        let arguments = if args_raw.is_empty() {
            json!({})
        } else if let Ok(v) = serde_json::from_str::<Value>(&args_raw) {
            v
        } else {
            json!({ "text": args_raw })
        };
        let cfg = state.config.read().clone();
        match state
            .mcp
            .call_tool(
                &state.db,
                &cfg.permissions.default_mcp,
                &server,
                &tool,
                arguments,
                false,
            )
            .await
        {
            Ok(result) => {
                return Ok(Some(format!(
                    "MCP {server}/{tool}:\n{}",
                    serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string())
                )));
            }
            Err(e) if e.starts_with("APPROVAL_REQUIRED:") => {
                return Ok(Some(format!(
                    "Chiamata MCP {server}/{tool} richiede approvazione (permission {}). Premi Approva in chat oppure digita `approva mcp`.",
                    cfg.permissions.default_mcp
                )));
            }
            Err(e) => return Ok(Some(format!("Errore MCP: {e}"))),
        }
    }

    Ok(None)
}

async fn execute_web_search(
    app: &AppHandle,
    state: &AppState,
    query: &str,
) -> Result<web_search::WebEvidence, String> {
    let _ = app.emit("chat-status", "Cerco sul web…");
    crate::debuglog::info(
        Some(app),
        format!("chat:web_search q_len={}", query.chars().count()),
    );
    let evidence = web_search::search_and_fetch(query).await;
    let pages = evidence
        .hits
        .iter()
        .filter(|h| h.page_text.is_some())
        .count();
    {
        let conn = state.db.lock()?;
        insert_ledger(
            &conn,
            "web_search",
            "read",
            if evidence.error.is_some() && evidence.hits.is_empty() {
                "error"
            } else {
                "executed"
            },
            &format!("web search «{query}»"),
            &json!({
                "query": query,
                "hits": evidence.hits.iter().map(|h| json!({
                    "title": h.title,
                    "url": h.url,
                    "has_page": h.page_text.is_some(),
                })).collect::<Vec<_>>(),
                "error": evidence.error,
            }),
        )?;
    }
    crate::debuglog::info(
        Some(app),
        format!(
            "chat:web_search hits={} pages={} err={:?}",
            evidence.hits.len(),
            pages,
            evidence.error
        ),
    );
    if pages > 0 {
        let _ = app.emit("chat-status", "Leggo le fonti…");
    }
    Ok(evidence)
}

pub async fn handle_chat(
    app: AppHandle,
    state: &AppState,
    req: ChatRequest,
) -> Result<ChatResponse, String> {
    let started = std::time::Instant::now();
    let app_log = app.clone();
    let log = move |msg: String| {
        crate::debuglog::info(Some(&app_log), msg);
    };

    log(format!(
        "chat:start msg_len={} session={:?}",
        req.message.chars().count(),
        req.session_id
    ));

    let cfg = state.config.read().clone();
    let mut used_web = false;
    log(format!(
        "chat:provider={} model={} ollama={}",
        cfg.provider, cfg.model, cfg.ollama.base_url
    ));

    log("chat:resolve_session".into());
    let session_id = if let Some(id) = req.session_id.clone() {
        log(format!("chat:reuse_request_session {id}"));
        id
    } else {
        // IMPORTANT: drop the read guard before any write().
        // Temporaries in `else if let x = lock.read()...` live for the whole if-else
        // and deadlock parking_lot RwLock on upgrade.
        let existing = state.active_session.read().clone();
        if let Some(id) = existing {
            log(format!("chat:reuse_active_session {id}"));
            id
        } else {
            log("chat:create_session_begin".into());
            let title = req.message.chars().take(48).collect::<String>();
            let s = sessions::create_session(&state.db, &cfg.provider, &cfg.model, &title)?;
            *state.active_session.write() = Some(s.id.clone());
            log(format!("chat:new_session {}", s.id));
            s.id
        }
    };

    log("chat:add_user_message".into());
    sessions::add_message(&state.db, &session_id, "user", &req.message)?;
    // User replied (typed or clicked a choice) — clear interactive question.
    *state.pending_question.write() = None;
    let _ = app.emit("avatar-state", "thinking");

    let mut web_block = String::new();
    let mut web_evidence: Option<web_search::WebEvidence> = None;
    match web_search::parse_command(&req.message) {
        Some(WebCommand::Help) => {
            let tool_reply = web_search::help_text();
            sessions::add_message(&state.db, &session_id, "assistant", &tool_reply)?;
            let _ = app.emit("avatar-state", "speaking");
            let _ = app.emit("chat-token", &tool_reply);
            return Ok(make_chat_response(
                state,
                session_id,
                tool_reply,
                session_badge(&cfg.provider, false),
                false,
            )?);
        }
        Some(WebCommand::Query(q)) => {
            used_web = true;
            let evidence = execute_web_search(&app, state, &q).await?;
            *state.last_web.write() = Some(web_search::WebThread {
                session_id: session_id.clone(),
                subject: web_search::strip_question_shell(&q),
                constraints: String::new(),
            });
            web_block = web_search::render_prompt_block(&evidence);
            web_evidence = Some(evidence);
        }
        None => {
            let follow = {
                let guard = state.last_web.read();
                guard
                    .as_ref()
                    .filter(|t| t.session_id == session_id && web_search::is_web_followup(&req.message))
                    .cloned()
            };
            if let Some(mut thread) = follow {
                used_web = true;
                web_search::apply_followup_constraint(&mut thread, &req.message);
                let q = web_search::compose_followup_query(&thread, &req.message);
                let evidence = execute_web_search(&app, state, &q).await?;
                *state.last_web.write() = Some(thread);
                web_block = web_search::render_prompt_block(&evidence);
                web_evidence = Some(evidence);
            }
        }
    }

    log("chat:tools_check".into());
    let lower_msg = req.message.trim().to_lowercase();
    if web_block.is_empty() && (lower_msg == "approva mcp" || lower_msg == "approve mcp") {
        log("chat:mcp_approve_resume".into());
        let snap = {
            let guard = state.frozen_memory.read();
            match guard.clone() {
                Some(s) => s,
                None => {
                    drop(guard);
                    refresh_frozen_memory(state)?;
                    state
                        .frozen_memory
                        .read()
                        .clone()
                        .ok_or_else(|| "memory snapshot missing".to_string())?
                }
            }
        };
        let mcp_tools = state.mcp.list_tools().await.unwrap_or_default();
        if mcp_tools.is_empty() {
            return Err("Nessun tool MCP connesso. Apri MCP → Riconnetti.".into());
        }
        if state.mcp.peek_pending().is_none() {
            let reply = "Nessuna chiamata MCP in attesa.".to_string();
            sessions::add_message(&state.db, &session_id, "assistant", &reply)?;
            let _ = app.emit("avatar-state", "speaking");
            let _ = app.emit("chat-token", &reply);
            return Ok(make_chat_response(
                state,
                session_id,
                reply,
                session_badge(&cfg.provider, false),
                false,
            )?);
        }
        let system = system_prompt(&snap, &cfg);
        let mut reply = crate::mcp::agent::resume_after_approval(
            &app,
            state,
            &cfg,
            &session_id,
            &mcp_tools,
            system,
        )
        .await?;
        apply_ask_question(state, &mut reply);
        sessions::add_message(&state.db, &session_id, "assistant", &reply)?;
        let _ = app.emit("chat-status", "");
        let _ = app.emit("avatar-state", "speaking");
        let _ = app.emit("chat-token", &reply);
        let mut tts_streamed = false;
        if crate::voice::local_tts_enabled(&cfg.voice) {
            let gen = crate::voice::begin_reply_speech();
            crate::voice::enqueue_sentence(&cfg.voice, &reply, gen);
            tts_streamed = true;
        } else {
            let _ = app.emit("avatar-state", "idle");
        }
        log(format!(
            "chat:mcp_approve_done chars={} ms={}",
            reply.chars().count(),
            started.elapsed().as_millis()
        ));
        let pending_mcp = state.mcp.peek_pending();
        let _ = app.emit("mcp-pending", pending_mcp.clone());
        return Ok(make_chat_response(
            state,
            session_id,
            reply,
            session_badge(&cfg.provider, false),
            tts_streamed,
        )?);
    }

    if web_block.is_empty() {
        if let Some(tool_reply) = maybe_handle_tools(&app, state, &req.message).await? {
            log("chat:handled_by_builtin_tool".into());
            let mut tool_reply = tool_reply;
            apply_ask_question(state, &mut tool_reply);
            sessions::add_message(&state.db, &session_id, "assistant", &tool_reply)?;
            let _ = app.emit("avatar-state", "speaking");
            let _ = app.emit("chat-token", &tool_reply);
            let mut tts_streamed = false;
            if crate::voice::local_tts_enabled(&cfg.voice) {
                let gen = crate::voice::begin_reply_speech();
                crate::voice::enqueue_sentence(&cfg.voice, &tool_reply, gen);
                tts_streamed = true;
            } else {
                let _ = app.emit("avatar-state", "idle");
            }
            let pending_mcp = state.mcp.peek_pending();
            if pending_mcp.is_some() {
                let _ = app.emit("mcp-pending", pending_mcp.clone());
            }
            return Ok(make_chat_response(
                state,
                session_id,
                tool_reply,
                session_badge(&cfg.provider, false),
                tts_streamed,
            )?);
        }
    }

    log("chat:memory_snapshot".into());
    let snap = {
        let guard = state.frozen_memory.read();
        match guard.clone() {
            Some(s) => s,
            None => {
                drop(guard);
                log("chat:memory_snapshot_missing — refreshing".into());
                refresh_frozen_memory(state)?;
                state
                    .frozen_memory
                    .read()
                    .clone()
                    .ok_or_else(|| "memory snapshot missing".to_string())?
            }
        }
    };

    log("chat:load_history".into());
    let history = sessions::list_messages(&state.db, &session_id)?;
    let mcp_tools = state.mcp.list_tools().await.unwrap_or_default();
    let use_mcp_agent = !mcp_tools.is_empty() && web_block.is_empty();

    let mut system = system_prompt(&snap, &cfg);
    if use_mcp_agent {
        system.push_str(&crate::mcp::agent::tools_system_block(&mcp_tools));
    }
    if !web_block.is_empty() {
        system.push_str(&web_block);
    }
    let mut messages = vec![ChatMessage {
        role: "system".into(),
        content: system,
    }];
    let history_take = if used_web { 12 } else { 16 };
    for m in history
        .iter()
        .rev()
        .take(history_take)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        messages.push(ChatMessage {
            role: m.role.clone(),
            content: crate::mcp::agent::sanitize_history_content(&m.content),
        });
    }

    // Generic MCP agent loop: model may call any discovered tool, then answer.
    if use_mcp_agent {
        log(format!(
            "chat:mcp_agent tools={} messages={}",
            mcp_tools.len(),
            messages.len()
        ));
        match crate::mcp::agent::run_loop(
            &app,
            state,
            &cfg,
            messages.clone(),
            &mcp_tools,
            &session_id,
            false,
        )
            .await
        {
            Ok(crate::mcp::agent::LoopOutcome::NeedsApproval { server, tool, level }) => {
                let reply = format!(
                    "Per rispondere mi serve il tool `{server}/{tool}` (permission {level}). \
                     Premi Approva qui sotto (o digita `approva mcp`): dopo l’approvazione continuo \
                     automaticamente con i passi successivi."
                );
                sessions::add_message(&state.db, &session_id, "assistant", &reply)?;
                let _ = app.emit("chat-status", "");
                let _ = app.emit("avatar-state", "speaking");
                let _ = app.emit("chat-token", &reply);
                let pending_mcp = state.mcp.peek_pending();
                let _ = app.emit("mcp-pending", pending_mcp.clone());
                let mut tts_streamed = false;
                if crate::voice::local_tts_enabled(&cfg.voice) {
                    let gen = crate::voice::begin_reply_speech();
                    crate::voice::enqueue_sentence(&cfg.voice, &reply, gen);
                    tts_streamed = true;
                } else {
                    let _ = app.emit("avatar-state", "idle");
                }
                return Ok(make_chat_response(
                    state,
                    session_id,
                    reply,
                    session_badge(&cfg.provider, false),
                    tts_streamed,
                )?);
            }
            Ok(crate::mcp::agent::LoopOutcome::Answer(reply)) => {
                // Fall through to streaming path only if empty; else emit like a tool reply
                if !reply.trim().is_empty() {
                    let mut reply = reply;
                    apply_ask_question(state, &mut reply);
                    if let Some(ev) = web_evidence.as_ref() {
                        let footer = web_search::render_sources_footer(ev);
                        if !footer.is_empty() {
                            reply.push_str(&footer);
                        }
                    }
                    sessions::add_message(&state.db, &session_id, "assistant", &reply)?;
                    let _ = app.emit("chat-status", "");
                    let _ = app.emit("avatar-state", "speaking");
                    let _ = app.emit("chat-token", &reply);
                    let mut tts_streamed = false;
                    if crate::voice::local_tts_enabled(&cfg.voice) {
                        let gen = crate::voice::begin_reply_speech();
                        crate::voice::enqueue_sentence(&cfg.voice, &reply, gen);
                        tts_streamed = true;
                    } else {
                        let _ = app.emit("avatar-state", "idle");
                    }
                    log(format!(
                        "chat:mcp_agent_done chars={} ms={}",
                        reply.chars().count(),
                        started.elapsed().as_millis()
                    ));
                    return Ok(make_chat_response(
                        state,
                        session_id,
                        reply,
                        session_badge(&cfg.provider, used_web),
                        tts_streamed,
                    )?);
                }
            }
            Err(e) => {
                crate::debuglog::warn(Some(&app), format!("chat:mcp_agent_err {e} — fallback stream"));
                // fall through to normal streaming
            }
        }
    }

    let _ = app.emit("chat-status", "");
    log(format!(
        "chat:calling_llm messages={} system_chars={}",
        messages.len(),
        messages.first().map(|m| m.content.len()).unwrap_or(0)
    ));

    let app2 = app.clone();
    let token_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let token_count2 = token_count.clone();
    let speaking_emitted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let speaking_emitted2 = speaking_emitted.clone();
    let stream_tts = crate::voice::local_tts_enabled(&cfg.voice);
    let voice_cfg = cfg.voice.clone();
    let tts_gen = if stream_tts {
        crate::voice::begin_reply_speech()
    } else {
        0
    };
    let sentence_buf = std::sync::Arc::new(parking_lot::Mutex::new(
        crate::voice::SentenceBuffer::default(),
    ));
    let sentence_buf2 = sentence_buf.clone();

    let mut reply = match llm::stream_chat(&cfg, &messages, move |token| {
        let n = token_count2.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if n == 0
            && !speaking_emitted2.swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            let _ = app2.emit("avatar-state", "speaking");
        }
        let _ = app2.emit("chat-token", &token);
        if stream_tts {
            let sentences = sentence_buf2.lock().push(&token);
            for s in sentences {
                crate::voice::enqueue_sentence(&voice_cfg, &s, tts_gen);
            }
        }
    })
    .await
    {
        Ok(r) => r,
        Err(e) => {
            crate::debuglog::error(Some(&app), format!("chat:llm_error {e}"));
            let _ = app.emit("chat-status", "");
            let _ = app.emit("avatar-state", "error");
            return Err(e);
        }
    };

    if reply.trim().is_empty() {
        let err = "Ollama ha restituito una risposta vuota. Riprova o cambia modello.".to_string();
        crate::debuglog::error(Some(&app), format!("chat:llm_error {err}"));
        let _ = app.emit("chat-status", "");
        let _ = app.emit("avatar-state", "error");
        return Err(err);
    }

    let mut tts_streamed = false;
    if stream_tts {
        if let Some(rest) = sentence_buf.lock().flush() {
            crate::voice::enqueue_sentence(&cfg.voice, &rest, tts_gen);
        }
        tts_streamed = true;
        // Do NOT wait here: the UI must show the reply immediately while
        // remaining sentences keep synthesizing/playing in the background.
    }

    if let Some(ev) = web_evidence.as_ref() {
        let footer = web_search::render_sources_footer(ev);
        if !footer.is_empty() {
            let _ = app.emit("chat-token", &footer);
            reply.push_str(&footer);
        }
    }

    log(format!(
        "chat:done chars={} tokens_approx={} ms={} tts_streamed={tts_streamed}",
        reply.chars().count(),
        token_count.load(std::sync::atomic::Ordering::Relaxed),
        started.elapsed().as_millis()
    ));

    apply_ask_question(state, &mut reply);
    sessions::add_message(&state.db, &session_id, "assistant", &reply)?;
    if !tts_streamed {
        let _ = app.emit("avatar-state", "idle");
    }
    // If tts_streamed, speech worker emits idle when the queue drains.

    Ok(make_chat_response(
        state,
        session_id,
        reply,
        session_badge(&cfg.provider, used_web),
        tts_streamed,
    )?)
}

pub fn ledger_list(db: &Db, limit: i64) -> Result<Vec<Value>, String> {
    let conn = db.lock()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, timestamp, tool, permission, status, summary, detail_json
             FROM action_ledger ORDER BY id DESC LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([limit], |row| {
            Ok(json!({
                "id": row.get::<_, i64>(0)?,
                "timestamp": row.get::<_, String>(1)?,
                "tool": row.get::<_, String>(2)?,
                "permission": row.get::<_, String>(3)?,
                "status": row.get::<_, String>(4)?,
                "summary": row.get::<_, String>(5)?,
                "detail": row.get::<_, String>(6)?,
            }))
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}
