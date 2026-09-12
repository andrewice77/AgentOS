use crate::agent::{self, AppState, ChatRequest};
use crate::config::{self, AppConfig, McpServerConfig};
use crate::llm;
use crate::memory;
use crate::organizer;
use crate::sessions;
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};

#[tauri::command]
pub async fn get_bootstrap(state: State<'_, AppState>) -> Result<Value, String> {
    let cfg = state.config.read().clone();
    let ollama_ok = llm::check_ollama(&cfg.ollama.base_url).await;
    let snap = memory::snapshot(&state.db, &cfg.memory)?;
    Ok(serde_json::json!({
        "config": cfg,
        "badge": config::provider_badge(&cfg.provider),
        "ollama_reachable": ollama_ok,
        "memory": snap,
        "pending_memory": memory::list_pending(&state.db)?,
        "pending_mcp": state.mcp.peek_pending(),
        "pending_question": state.pending_question.read().clone(),
        "tasks": organizer::list_tasks(&state.db)?,
        "reminders": organizer::list_reminders(&state.db)?,
        "labels": organizer::list_labels(&state.db)?,
        "sessions": sessions::list_sessions(&state.db)?,
        "active_session": state.active_session.read().clone(),
        "mcp_servers": state.mcp.list_configs(),
        "ledger": agent::ledger_list(&state.db, 50)?,
    }))
}

#[tauri::command]
pub async fn chat(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ChatRequest,
) -> Result<agent::ChatResponse, String> {
    // Clone before await — holding State across await can deadlock the IPC runtime.
    let state = state.inner().clone();
    agent::handle_chat(app, &state, request).await
}

#[tauri::command]
pub fn list_messages(state: State<'_, AppState>, session_id: String) -> Result<Vec<sessions::MessageRow>, String> {
    sessions::list_messages(&state.db, &session_id)
}

#[tauri::command]
pub fn list_sessions(state: State<'_, AppState>) -> Result<Vec<sessions::Session>, String> {
    sessions::list_sessions(&state.db)
}

#[tauri::command]
pub fn new_session(state: State<'_, AppState>) -> Result<sessions::Session, String> {
    let cfg = state.config.read().clone();
    let s = sessions::create_session(&state.db, &cfg.provider, &cfg.model, "Nuova chat")?;
    *state.active_session.write() = Some(s.id.clone());
    *state.last_web.write() = None;
    state.zoho_ctx.clear();
    agent::refresh_frozen_memory(&state)?;
    Ok(s)
}

#[tauri::command]
pub fn set_active_session(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
    let prev = state.active_session.read().clone();
    *state.active_session.write() = Some(session_id.clone());
    if prev.as_ref() != Some(&session_id) {
        *state.last_web.write() = None;
        state.zoho_ctx.clear();
    }
    Ok(())
}

#[tauri::command]
pub fn get_memory(state: State<'_, AppState>) -> Result<memory::MemorySnapshot, String> {
    let cfg = state.config.read().clone();
    memory::snapshot(&state.db, &cfg.memory)
}

#[tauri::command]
pub fn update_memory_entry(
    state: State<'_, AppState>,
    id: i64,
    content: String,
) -> Result<(), String> {
    let cfg = state.config.read().clone();
    memory::update_entry(&state.db, &cfg.memory, id, &content)?;
    agent::refresh_frozen_memory(&state)?;
    Ok(())
}

#[tauri::command]
pub fn delete_memory_entry(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<(), String> {
    memory::delete_entry(&state.db, id)?;
    agent::refresh_frozen_memory(&state)?;
    let _ = app.emit("memory-changed", id);
    Ok(())
}

#[tauri::command]
pub fn list_pending_memory(state: State<'_, AppState>) -> Result<Vec<memory::PendingMemory>, String> {
    memory::list_pending(&state.db)
}

#[tauri::command]
pub fn propose_memory(
    app: AppHandle,
    state: State<'_, AppState>,
    store: String,
    content: String,
) -> Result<Value, String> {
    let cfg = state.config.read().clone();
    let store = store.trim();
    let content = content.trim();
    if content.is_empty() {
        return Err("content vuoto".into());
    }
    memory::scan_memory_content(content)?;
    if cfg.memory.write_approval {
        let id = memory::stage_write(&state.db, store, "add", Some(content), None)?;
        let _ = app.emit("memory-changed", id);
        Ok(serde_json::json!({"pending_id": id, "applied": false}))
    } else {
        memory::apply_write(&state.db, &cfg.memory, store, "add", Some(content), None)?;
        agent::refresh_frozen_memory(&state)?;
        let _ = app.emit("memory-changed", &store);
        Ok(serde_json::json!({"pending_id": null, "applied": true}))
    }
}

#[tauri::command]
pub fn approve_memory(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<String, String> {
    let cfg = state.config.read().clone();
    let r = memory::approve_pending(&state.db, &cfg.memory, id)?;
    agent::refresh_frozen_memory(&state)?;
    let _ = app.emit("memory-changed", id);
    Ok(r)
}

#[tauri::command]
pub fn reject_memory(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<(), String> {
    memory::reject_pending(&state.db, id)?;
    let _ = app.emit("memory-changed", id);
    Ok(())
}

#[tauri::command]
pub fn session_search(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<sessions::SearchHit>, String> {
    sessions::session_search(&state.db, &query, 20)
}

#[tauri::command]
pub fn rebuild_session_fts(state: State<'_, AppState>) -> Result<usize, String> {
    sessions::rebuild_fts(&state.db)
}

#[tauri::command]
pub fn list_tasks(state: State<'_, AppState>) -> Result<Vec<organizer::Task>, String> {
    organizer::list_tasks(&state.db)
}

#[tauri::command]
pub fn create_task(
    app: AppHandle,
    state: State<'_, AppState>,
    title: String,
    description: String,
    priority: i64,
    deadline: Option<String>,
    label_ids: Option<Vec<i64>>,
) -> Result<i64, String> {
    let ids = label_ids.unwrap_or_default();
    let id = organizer::create_task(
        &state.db,
        &title,
        &description,
        priority,
        deadline.as_deref(),
        &ids,
        &[],
    )?;
    let _ = app.emit("tasks-changed", id);
    Ok(id)
}

#[tauri::command]
pub fn set_task_status(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    status: String,
) -> Result<(), String> {
    organizer::set_task_status(&state.db, id, &status)?;
    let _ = app.emit("tasks-changed", id);
    Ok(())
}

#[tauri::command]
pub fn update_task(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    title: Option<String>,
    description: Option<String>,
    priority: Option<i64>,
    deadline: Option<String>,
    touch_deadline: Option<bool>,
    status: Option<String>,
) -> Result<(), String> {
    organizer::update_task(
        &state.db,
        id,
        title.as_deref(),
        description.as_deref(),
        priority,
        deadline.as_deref(),
        touch_deadline.unwrap_or(false),
        status.as_deref(),
    )?;
    let _ = app.emit("tasks-changed", id);
    Ok(())
}

#[tauri::command]
pub fn delete_task(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    organizer::delete_task(&state.db, id)?;
    let _ = app.emit("tasks-changed", id);
    Ok(())
}

#[tauri::command]
pub fn list_reminders(state: State<'_, AppState>) -> Result<Vec<organizer::Reminder>, String> {
    organizer::list_reminders(&state.db)
}

#[tauri::command]
pub fn create_reminder(
    app: AppHandle,
    state: State<'_, AppState>,
    schedule: String,
    message: String,
    task_id: Option<i64>,
    recurrence: Option<String>,
) -> Result<i64, String> {
    let id = organizer::create_reminder(
        &state.db,
        &schedule,
        &message,
        task_id,
        recurrence.as_deref(),
    )?;
    let _ = app.emit("reminders-changed", id);
    Ok(id)
}

#[tauri::command]
pub fn update_reminder(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    schedule: Option<String>,
    message: Option<String>,
    task_id: Option<i64>,
    touch_task_id: Option<bool>,
    recurrence: Option<String>,
) -> Result<(), String> {
    organizer::update_reminder(
        &state.db,
        id,
        schedule.as_deref(),
        message.as_deref(),
        task_id,
        touch_task_id.unwrap_or(false),
        recurrence.as_deref(),
    )?;
    let _ = app.emit("reminders-changed", id);
    Ok(())
}

#[tauri::command]
pub fn delete_reminder(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    organizer::delete_reminder(&state.db, id)?;
    let _ = app.emit("reminders-changed", id);
    Ok(())
}

#[tauri::command]
pub fn snooze_reminder(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    minutes: i64,
) -> Result<String, String> {
    let rfc = organizer::snooze_reminder(&state.db, id, minutes)?;
    let _ = app.emit("reminders-changed", id);
    Ok(rfc)
}

#[tauri::command]
pub fn snooze_reminder_tomorrow(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    hour: Option<u32>,
    minute: Option<u32>,
) -> Result<String, String> {
    let rfc = organizer::snooze_reminder_until_tomorrow(
        &state.db,
        id,
        hour.unwrap_or(9),
        minute.unwrap_or(0),
    )?;
    let _ = app.emit("reminders-changed", id);
    Ok(rfc)
}

#[tauri::command]
pub fn reschedule_reminder(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    schedule: String,
) -> Result<String, String> {
    let rfc = organizer::reschedule_reminder(&state.db, id, &schedule)?;
    let _ = app.emit("reminders-changed", id);
    Ok(rfc)
}

#[tauri::command]
pub fn list_labels(state: State<'_, AppState>) -> Result<Vec<organizer::Label>, String> {
    organizer::list_labels(&state.db)
}

#[tauri::command]
pub fn create_label(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    kind: String,
    color: Option<String>,
) -> Result<i64, String> {
    let id = organizer::create_label(&state.db, &name, &kind, color.as_deref())?;
    let _ = app.emit("labels-changed", id);
    let _ = app.emit("tasks-changed", id);
    Ok(id)
}

#[tauri::command]
pub fn update_label_color(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    color: String,
) -> Result<(), String> {
    organizer::update_label_color(&state.db, id, &color)?;
    let _ = app.emit("labels-changed", id);
    let _ = app.emit("tasks-changed", id);
    Ok(())
}

#[tauri::command]
pub fn recolor_generic_labels(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let n = organizer::recolor_generic_labels(&state.db)?;
    let _ = app.emit("labels-changed", n as i64);
    let _ = app.emit("tasks-changed", n as i64);
    Ok(n)
}

#[tauri::command]
pub fn delete_label(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    organizer::delete_label(&state.db, id)?;
    let _ = app.emit("labels-changed", id);
    let _ = app.emit("tasks-changed", id);
    Ok(())
}

#[tauri::command]
pub fn set_task_labels(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: i64,
    label_ids: Vec<i64>,
) -> Result<(), String> {
    organizer::set_task_labels(&state.db, task_id, &label_ids)?;
    let _ = app.emit("tasks-changed", task_id);
    Ok(())
}

#[tauri::command]
pub fn get_ledger(state: State<'_, AppState>) -> Result<Vec<Value>, String> {
    agent::ledger_list(&state.db, 100)
}

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    Ok(state.config.read().clone())
}

#[tauri::command]
pub fn save_app_config(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    cfg: AppConfig,
) -> Result<(), String> {
    config::save_config(&cfg)?;
    state.mcp.update_configs(cfg.mcp_servers.clone());
    *state.config.write() = cfg.clone();
    agent::refresh_frozen_memory(&state)?;
    crate::shell::apply_avatar_window_size(&app);
    crate::shell::apply_main_window_theme(&app, &cfg.ui.theme);
    let _ = app.emit(
        "config-updated",
        serde_json::json!({
            "avatar": cfg.avatar,
            "avatar_roam": cfg.avatar_roam,
            "provider": cfg.provider,
            "ui": cfg.ui,
        }),
    );
    Ok(())
}

#[tauri::command]
pub async fn list_local_models(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let cfg = state.config.read().clone();
    llm::list_ollama_models(&cfg.ollama.base_url).await
}

#[tauri::command]
pub fn set_provider_key(provider: String, key: String) -> Result<(), String> {
    llm::set_api_key(&provider, &key)
}

#[tauri::command]
pub fn clear_provider_key(provider: String) -> Result<(), String> {
    llm::clear_api_key(&provider)
}

#[tauri::command]
pub async fn refresh_mcp(state: State<'_, AppState>) -> Result<Vec<crate::mcp::McpToolInfo>, String> {
    let state = state.inner().clone();
    state.mcp.refresh_connections().await?;
    state.mcp.list_tools().await
}

#[tauri::command]
pub fn list_mcp_servers(state: State<'_, AppState>) -> Result<Vec<McpServerConfig>, String> {
    Ok(state.mcp.list_configs())
}

#[tauri::command]
pub async fn mcp_status(state: State<'_, AppState>) -> Result<crate::mcp::McpStatus, String> {
    let state = state.inner().clone();
    state.mcp.status().await
}

#[tauri::command]
pub fn install_mcp_echo(state: State<'_, AppState>) -> Result<McpServerConfig, String> {
    let echo = crate::mcp::echo_server_config();
    let mut cfg = state.config.read().clone();
    cfg.mcp_servers.retain(|s| s.name != echo.name);
    cfg.mcp_servers.push(echo.clone());
    config::save_config(&cfg)?;
    state.mcp.update_configs(cfg.mcp_servers.clone());
    *state.config.write() = cfg;
    Ok(echo)
}

#[tauri::command]
pub fn remove_mcp_server(state: State<'_, AppState>, name: String) -> Result<(), String> {
    let mut cfg = state.config.read().clone();
    cfg.mcp_servers.retain(|s| s.name != name);
    config::save_config(&cfg)?;
    state.mcp.update_configs(cfg.mcp_servers.clone());
    *state.config.write() = cfg;
    Ok(())
}

#[tauri::command]
pub fn get_pending_mcp(
    state: State<'_, AppState>,
) -> Result<Option<crate::mcp::PendingMcpCall>, String> {
    Ok(state.mcp.peek_pending())
}

#[tauri::command]
pub async fn call_mcp_tool(
    state: State<'_, AppState>,
    server: String,
    tool: String,
    arguments: Value,
    approved: bool,
) -> Result<Value, String> {
    let state = state.inner().clone();
    let cfg = state.config.read().clone();
    state
        .mcp
        .call_tool(
            &state.db,
            &cfg.permissions.default_mcp,
            &server,
            &tool,
            arguments,
            approved,
        )
        .await
}

#[tauri::command]
pub async fn approve_pending_mcp(app: AppHandle, state: State<'_, AppState>) -> Result<Value, String> {
    let state = state.inner().clone();
    let pending = state
        .mcp
        .take_pending()
        .ok_or_else(|| "Nessuna chiamata MCP in attesa".to_string())?;
    let _ = app.emit("mcp-pending", Option::<crate::mcp::PendingMcpCall>::None);
    let cfg = state.config.read().clone();
    state
        .mcp
        .call_tool(
            &state.db,
            &cfg.permissions.default_mcp,
            &pending.server,
            &pending.tool,
            pending.arguments,
            true,
        )
        .await
}

/// Approve the pending MCP call and resume the agent loop for the active session.
#[tauri::command]
pub async fn approve_and_resume_mcp(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<agent::ChatResponse, String> {
    let state = state.inner().clone();
    let session_id = state
        .active_session
        .read()
        .clone()
        .ok_or_else(|| "Nessuna sessione attiva".to_string())?;
    // Reuse the same chat path as typing "approva mcp"
    agent::handle_chat(
        app,
        &state,
        agent::ChatRequest {
            message: "approva mcp".into(),
            session_id: Some(session_id),
        },
    )
    .await
}

#[tauri::command]
pub fn reject_pending_mcp(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let _ = state.mcp.take_pending();
    let _ = app.emit("mcp-pending", Option::<crate::mcp::PendingMcpCall>::None);
    Ok(())
}

#[tauri::command]
pub async fn start_mcp_oauth(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<String, String> {
    let state = state.inner().clone();
    crate::mcp::start_oauth(&app, &state.mcp, &name).await
}

#[tauri::command]
pub fn mcp_auth_status(state: State<'_, AppState>) -> Result<Vec<crate::mcp::McpAuthStatus>, String> {
    Ok(state.mcp.auth_status())
}

#[tauri::command]
pub fn logout_mcp_oauth(state: State<'_, AppState>, name: String) -> Result<String, String> {
    crate::mcp::logout(&state.mcp, &name)
}

#[tauri::command]
pub fn desktop_notify(title: String, body: String) -> Result<(), String> {
    notify_rust::Notification::new()
        .summary(&title)
        .body(&body)
        .appname("AgentOS")
        .sound_name("message-new-instant")
        .show()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn poll_due_reminders(state: State<'_, AppState>) -> Result<Vec<organizer::Reminder>, String> {
    // Prefer the background poller for notify; this command remains for manual/UI checks.
    let due = organizer::due_reminders(&state.db)?;
    for r in &due {
        organizer::mark_fired_or_advance(&state.db, r.id)?;
    }
    Ok(due)
}

#[tauri::command]
pub fn get_debug_logs() -> Result<Vec<crate::debuglog::DebugLine>, String> {
    Ok(crate::debuglog::snapshot())
}

#[tauri::command]
pub fn clear_debug_logs() -> Result<(), String> {
    crate::debuglog::clear();
    Ok(())
}

#[tauri::command]
pub fn push_debug_log(app: tauri::AppHandle, level: String, message: String) -> Result<(), String> {
    let lvl = match level.as_str() {
        "error" => "error",
        "warn" => "warn",
        _ => "info",
    };
    crate::debuglog::log(Some(&app), lvl, message);
    Ok(())
}

#[tauri::command]
pub fn request_avatar_probe(app: tauri::AppHandle) -> Result<(), String> {
    crate::debuglog::info(Some(&app), "avatar:probe_requested from debug tab");
    let _ = app.emit("avatar-debug-probe", serde_json::json!({ "source": "debug-tab" }));
    Ok(())
}

#[tauri::command]
pub async fn run_diagnostics(state: State<'_, AppState>) -> Result<Value, String> {
    let cfg = state.config.read().clone();
    crate::debuglog::info(None, "diagnostics:start");
    let ollama_ok = llm::check_ollama(&cfg.ollama.base_url).await;
    crate::debuglog::info(None, format!("diagnostics:ollama_reachable={ollama_ok}"));

    let mut models = Vec::new();
    let mut sample = String::new();
    let mut sample_err = String::new();
    if ollama_ok {
        models = llm::list_ollama_models(&cfg.ollama.base_url)
            .await
            .unwrap_or_default();
        crate::debuglog::info(None, format!("diagnostics:models={models:?}"));
        let probe = vec![crate::llm::ChatMessage {
            role: "user".into(),
            content: "Rispondi solo con: PONG".into(),
        }];
        match llm::complete(&cfg, &probe).await {
            Ok(r) => {
                sample = r.chars().take(200).collect();
                crate::debuglog::info(None, format!("diagnostics:sample_ok={sample}"));
            }
            Err(e) => {
                sample_err = e.clone();
                crate::debuglog::error(None, format!("diagnostics:sample_err={e}"));
            }
        }
    }

    Ok(serde_json::json!({
        "provider": cfg.provider,
        "model": cfg.model,
        "ollama_base_url": cfg.ollama.base_url,
        "ollama_reachable": ollama_ok,
        "models": models,
        "sample_reply": sample,
        "sample_error": sample_err,
        "data_dir": crate::config::data_dir().display().to_string(),
        "log_file": crate::config::data_dir().join("logs/agentos.log").display().to_string(),
        "voice": crate::voice::status(&cfg.voice),
        "webkit_disable_dmabuf": std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").ok(),
        "webkit_disable_compositing": std::env::var("WEBKIT_DISABLE_COMPOSITING_MODE").ok(),
    }))
}

#[tauri::command]
pub fn voice_status(state: State<'_, AppState>) -> Result<crate::voice::VoiceStatus, String> {
    let cfg = state.config.read().clone();
    Ok(crate::voice::status(&cfg.voice))
}

#[tauri::command]
pub async fn speak_text(state: State<'_, AppState>, text: String) -> Result<(), String> {
    let cfg = state.config.read().clone();
    if cfg.voice.engine == "system" {
        return Err("engine=system: usa Web Speech nel frontend".into());
    }
    crate::voice::speak_engine(cfg.voice.clone(), text).await
}

#[tauri::command]
pub fn stop_speech() -> Result<(), String> {
    crate::voice::stop_speech()
}

#[tauri::command]
pub async fn listen_transcribe(
    state: State<'_, AppState>,
    seconds: Option<u32>,
) -> Result<String, String> {
    let cfg = state.config.read().clone();
    crate::voice::record_and_transcribe(cfg.voice, seconds.unwrap_or(5)).await
}

#[tauri::command]
pub async fn get_morning_briefing(
    state: State<'_, AppState>,
) -> Result<crate::briefing::MorningBriefing, String> {
    let cfg = state.config.read().clone();
    crate::briefing::build_briefing(&cfg, &state.db).await
}

#[tauri::command]
pub fn briefing_should_offer(state: State<'_, AppState>) -> Result<bool, String> {
    let cfg = state.config.read().clone();
    Ok(crate::briefing::should_offer_now(&cfg))
}

#[tauri::command]
pub fn list_skills() -> Result<Vec<crate::skills::SkillMeta>, String> {
    crate::skills::list_skills()
}

#[tauri::command]
pub fn read_skill(id: String) -> Result<String, String> {
    crate::skills::read_skill(&id)
}

#[tauri::command]
pub fn write_skill(id: String, markdown: String) -> Result<(), String> {
    crate::skills::write_skill(&id, &markdown)
}

#[tauri::command]
pub fn delete_skill(id: String) -> Result<(), String> {
    crate::skills::delete_skill(&id)
}

#[tauri::command]
pub async fn run_skills_review(
    state: State<'_, AppState>,
) -> Result<crate::skills::SkillReviewProposal, String> {
    let cfg = state.config.read().clone();
    crate::skills::run_background_review(&cfg, &state.db).await
}

#[tauri::command]
pub fn latest_skills_review() -> Result<Option<crate::skills::SkillReviewProposal>, String> {
    crate::skills::latest_review()
}

#[tauri::command]
pub fn apply_skills_review(id: String, markdown: String) -> Result<(), String> {
    crate::skills::write_skill(&id, &markdown)
}
