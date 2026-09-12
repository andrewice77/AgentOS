mod agent;
mod briefing;
mod commands;
mod config;
mod db;
mod debuglog;
mod llm;
mod mcp;
mod memory;
mod organizer;
mod permissions;
mod sessions;
mod shell;
mod skills;
mod voice;
mod web_search;

use agent::AppState;
use mcp::McpHub;
use parking_lot::RwLock;
use std::sync::Arc;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .on_window_event(|window, event| {
            // Keep the process alive in the tray; companion windows are hide-only.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            let cfg = config::load_config().map_err(|e| {
                tracing::error!("config error: {e}");
                e
            })?;
            config::ensure_data_dirs()?;
            crate::debuglog::info(None, "AgentOS starting");
            crate::voice::set_app_handle(app.handle().clone());
            crate::voice::ensure_speech_worker();
            let database = Arc::new(db::Db::open()?);
            crate::debuglog::info(None, "database opened");
            let _ = crate::skills::ensure_skills_dir();
            let mcp = Arc::new(McpHub::new(cfg.mcp_servers.clone()));
            let main_theme = cfg.ui.theme.clone();
            let state = AppState {
                db: database,
                config: Arc::new(RwLock::new(cfg)),
                mcp: mcp.clone(),
                frozen_memory: Arc::new(RwLock::new(None)),
                active_session: Arc::new(RwLock::new(None)),
                last_web: Arc::new(RwLock::new(None)),
                zoho_ctx: Arc::new(crate::mcp::zoho_ctx::ZohoCtxStore::default()),
                pending_question: Arc::new(RwLock::new(None)),
            };
            agent::refresh_frozen_memory(&state)?;
            // Keep FTS roughly in sync if messages exist but index drifted
            let msg_n: i64 = state
                .db
                .lock()
                .ok()
                .and_then(|c| c.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0)).ok())
                .unwrap_or(0);
            let fts_n: i64 = state
                .db
                .lock()
                .ok()
                .and_then(|c| {
                    c.query_row("SELECT COUNT(*) FROM messages_fts", [], |r| r.get(0))
                        .ok()
                })
                .unwrap_or(0);
            if msg_n > 0 && msg_n != fts_n {
                match sessions::rebuild_fts(&state.db) {
                    Ok(n) => crate::debuglog::info(None, format!("fts:rebuilt {n} messages")),
                    Err(e) => crate::debuglog::warn(None, format!("fts:rebuild_skip {e}")),
                }
            }
            app.manage(state);

            let chat_i = MenuItem::with_id(app, "chat", "Apri / chiudi chat", true, None::<&str>)?;
            let avatar_i =
                MenuItem::with_id(app, "avatar", "Mostra avatar", true, None::<&str>)?;
            let console_i =
                MenuItem::with_id(app, "console", "Apri console", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Esci", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&chat_i, &avatar_i, &console_i, &quit_i])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .tooltip("AgentOS — click per chat")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => app.exit(0),
                    "chat" => {
                        let _ = shell::toggle_chat_panel(app.clone());
                    }
                    "avatar" => {
                        let _ = shell::show_avatar(app.clone());
                    }
                    "console" => {
                        let _ = shell::open_console(app.clone());
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        let _ = shell::toggle_chat_panel(app.clone());
                    }
                })
                .build(app)?;

            // Ensure companion shell is the visible face of the app
            if let Some(avatar) = app.get_webview_window("avatar") {
                let _ = avatar.set_background_color(Some(shell::avatar_window_color()));
                let _ = avatar.show();
            }
            shell::place_avatar_on_startup(app.handle());
            crate::debuglog::info(
                None,
                format!(
                    "webkit WEBKIT_DISABLE_DMABUF_RENDERER={:?} WEBKIT_DISABLE_COMPOSITING_MODE={:?}",
                    std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").ok(),
                    std::env::var("WEBKIT_DISABLE_COMPOSITING_MODE").ok()
                ),
            );
            if let Some(main) = app.get_webview_window("main") {
                shell::apply_main_window_theme(app.handle(), &main_theme);
                let _ = main.hide();
            }
            if let Some(chat) = app.get_webview_window("chat") {
                let _ = chat.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
                let _ = chat.hide();
            }

            // Best-effort MCP connect in background
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Some(state) = handle.try_state::<AppState>() {
                    let _ = state.mcp.refresh_connections().await;
                }
            });
            crate::mcp::watch::spawn(app.handle().clone());

            // Reminder poller — works even if the UI tab is idle
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                use tauri::Emitter;
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    let Some(state) = handle.try_state::<AppState>() else {
                        continue;
                    };
                    let due = match organizer::due_reminders(&state.db) {
                        Ok(d) => d,
                        Err(e) => {
                            crate::debuglog::warn(None, format!("reminder:poll_err {e}"));
                            continue;
                        }
                    };
                    for r in due {
                        if let Err(e) = organizer::mark_fired_or_advance(&state.db, r.id) {
                            crate::debuglog::warn(None, format!("reminder:mark_fired {e}"));
                            continue;
                        }
                        crate::debuglog::info(
                            None,
                            format!("reminder:fired #{} {}", r.id, r.message),
                        );
                        let _ = notify_rust::Notification::new()
                            .summary("AgentOS — promemoria")
                            .body(&r.message)
                            .appname("AgentOS")
                            .sound_name("message-new-instant")
                            .show();
                        let _ = handle.emit("reminder-due", &r);
                    }
                }
            });

            // DevTools only when explicitly requested (avoids noise in normal use)
            if std::env::var_os("AGENTOS_DEVTOOLS").is_some() {
                for label in ["avatar", "chat", "main"] {
                    if let Some(window) = app.get_webview_window(label) {
                        window.open_devtools();
                    }
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            shell::toggle_chat_panel,
            shell::show_chat_panel,
            shell::hide_chat_panel,
            shell::open_console,
            shell::show_avatar,
            shell::reposition_chat,
            shell::get_avatar_pos,
            shell::move_avatar_by,
            shell::set_avatar_pos,
            shell::begin_avatar_drag,
            shell::get_avatar_desktop,
            shell::sync_avatar_window,
            shell::set_avatar_overlay,
            shell::set_avatar_sprite_pos,
            shell::set_avatar_clickthrough,
            shell::avatar_display_profile,
            shell::apply_main_window_theme_cmd,
            commands::get_bootstrap,
            commands::chat,
            commands::list_messages,
            commands::list_sessions,
            commands::new_session,
            commands::set_active_session,
            commands::get_memory,
            commands::update_memory_entry,
            commands::delete_memory_entry,
            commands::list_pending_memory,
            commands::propose_memory,
            commands::approve_memory,
            commands::reject_memory,
            commands::session_search,
            commands::rebuild_session_fts,
            commands::list_tasks,
            commands::create_task,
            commands::set_task_status,
            commands::update_task,
            commands::delete_task,
            commands::list_reminders,
            commands::create_reminder,
            commands::update_reminder,
            commands::delete_reminder,
            commands::snooze_reminder,
            commands::snooze_reminder_tomorrow,
            commands::reschedule_reminder,
            commands::list_labels,
            commands::create_label,
            commands::update_label_color,
            commands::recolor_generic_labels,
            commands::delete_label,
            commands::set_task_labels,
            commands::get_ledger,
            commands::get_config,
            commands::save_app_config,
            commands::list_local_models,
            commands::set_provider_key,
            commands::clear_provider_key,
            commands::refresh_mcp,
            commands::list_mcp_servers,
            commands::mcp_status,
            commands::install_mcp_echo,
            commands::remove_mcp_server,
            commands::get_pending_mcp,
            commands::call_mcp_tool,
            commands::approve_pending_mcp,
            commands::approve_and_resume_mcp,
            commands::reject_pending_mcp,
            commands::start_mcp_oauth,
            commands::mcp_auth_status,
            commands::logout_mcp_oauth,
            commands::desktop_notify,
            commands::poll_due_reminders,
            commands::get_debug_logs,
            commands::clear_debug_logs,
            commands::push_debug_log,
            commands::request_avatar_probe,
            commands::run_diagnostics,
            commands::voice_status,
            commands::speak_text,
            commands::stop_speech,
            commands::listen_transcribe,
            commands::get_morning_briefing,
            commands::briefing_should_offer,
            commands::list_skills,
            commands::read_skill,
            commands::write_skill,
            commands::delete_skill,
            commands::run_skills_review,
            commands::latest_skills_review,
            commands::apply_skills_review,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
