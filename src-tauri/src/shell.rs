//! Companion shell: floating avatar + chat panel window management.

use parking_lot::Mutex;
use serde::Serialize;
use std::sync::OnceLock;
use tauri::{AppHandle, LogicalSize, Manager, PhysicalPosition, PhysicalSize, Size, WebviewWindow};

/// Linux WebKitGTK + compositor cannot damage alpha-0 pixels (clone trails).
/// Windows (WebView2) and macOS (WKWebView) typically can, so those builds
/// use a transparent "glass" avatar window. Overlay roam is unused.
pub fn avatar_uses_glass() -> bool {
    !cfg!(target_os = "linux")
}

pub fn avatar_window_color() -> tauri::window::Color {
    if avatar_uses_glass() {
        tauri::window::Color(0, 0, 0, 0)
    } else {
        tauri::window::Color(14, 21, 18, 255)
    }
}

/// Opaque background for the main console window (WebKitGTK defaults to white).
pub fn main_window_color(theme: &str) -> tauri::window::Color {
    match theme {
        "light" => tauri::window::Color(238, 241, 246, 255),
        "slate" => tauri::window::Color(20, 21, 25, 255),
        "ocean" => tauri::window::Color(10, 20, 32, 255),
        "warm" => tauri::window::Color(20, 18, 16, 255),
        _ => tauri::window::Color(18, 24, 32, 255), // midnight
    }
}

pub fn apply_main_window_theme(app: &AppHandle, theme: &str) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.set_background_color(Some(main_window_color(theme)));
    }
}

#[tauri::command]
pub fn apply_main_window_theme_cmd(app: AppHandle, theme: String) -> Result<(), String> {
    apply_main_window_theme(&app, &theme);
    Ok(())
}

#[derive(Serialize)]
pub struct AvatarDisplayProfile {
    pub os: String,
    pub glass: bool,
}

#[tauri::command]
pub fn avatar_display_profile() -> AvatarDisplayProfile {
    AvatarDisplayProfile {
        os: std::env::consts::OS.to_string(),
        glass: avatar_uses_glass(),
    }
}

/// Sprite state for the unused fullscreen overlay path.
struct OverlayState {
    enabled: bool,
    sprite_x: i32,
    sprite_y: i32,
    sprite_w: u32,
    sprite_h: u32,
}

fn overlay() -> &'static Mutex<OverlayState> {
    static S: OnceLock<Mutex<OverlayState>> = OnceLock::new();
    S.get_or_init(|| {
        Mutex::new(OverlayState {
            enabled: false,
            sprite_x: 0,
            sprite_y: 0,
            sprite_w: AVATAR_W,
            sprite_h: AVATAR_H,
        })
    })
}

fn apply_monitor_overlay(win: &WebviewWindow) -> Result<(), String> {
    let mon = win
        .current_monitor()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "monitor assente".to_string())?;
    let p = mon.position();
    let s = mon.size();
    win.set_position(PhysicalPosition::new(p.x, p.y))
        .map_err(|e| format!("overlay position: {e}"))?;
    win.set_size(tauri::Size::Physical(PhysicalSize::new(s.width, s.height)))
        .map_err(|e| format!("overlay size: {e}"))?;
    let _ = win.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
    Ok(())
}

fn clamp_sprite_pos(win: &WebviewWindow, x: i32, y: i32, sw: u32, sh: u32) -> (i32, i32) {
    let Ok(Some(mon)) = win.current_monitor() else {
        return (x, y);
    };
    let mpos = mon.position();
    let msize = mon.size();
    let margin = 8i32;
    let min_x = mpos.x + margin;
    let min_y = mpos.y + margin;
    let max_x = mpos.x + msize.width as i32 - sw as i32 - margin;
    let max_y = mpos.y + msize.height as i32 - sh as i32 - margin;
    (
        x.clamp(min_x.min(max_x), max_x.max(min_x)),
        y.clamp(min_y.min(max_y), max_y.max(min_y)),
    )
}

fn avatar_anchor(win: &WebviewWindow) -> Result<(i32, i32, u32, u32), String> {
    let st = overlay().lock();
    if st.enabled {
        return Ok((st.sprite_x, st.sprite_y, st.sprite_w, st.sprite_h));
    }
    drop(st);
    let pos = win.outer_position().map_err(|e| e.to_string())?;
    let size = win.outer_size().map_err(|e| e.to_string())?;
    Ok((pos.x, pos.y, size.width, size.height))
}

fn avatar_win(app: &AppHandle) -> Result<WebviewWindow, String> {
    app.get_webview_window("avatar")
        .ok_or_else(|| "finestra avatar assente".into())
}

fn chat_win(app: &AppHandle) -> Result<WebviewWindow, String> {
    app.get_webview_window("chat")
        .ok_or_else(|| "finestra chat assente".into())
}

fn main_win(app: &AppHandle) -> Result<WebviewWindow, String> {
    app.get_webview_window("main")
        .ok_or_else(|| "finestra console assente".into())
}

/// Place the chat panel beside the avatar (right if space, else left).
pub fn position_chat_near_avatar(app: &AppHandle) -> Result<(), String> {
    let avatar = avatar_win(app)?;
    let chat = chat_win(app)?;

    let (av_x, av_y, av_w, av_h) = avatar_anchor(&avatar)?;
    let av_pos = PhysicalPosition::new(av_x, av_y);
    let av_size = PhysicalSize::new(av_w, av_h);
    let chat_size = chat
        .outer_size()
        .unwrap_or(PhysicalSize::new(400, 560));

    // Prefer monitor that contains the avatar
    let gap = 10i32;
    let mut x = av_pos.x + av_size.width as i32 + gap;
    let y = av_pos.y;

    if let Ok(Some(monitor)) = avatar.current_monitor() {
        let mpos = monitor.position();
        let msize = monitor.size();
        let right_edge = mpos.x + msize.width as i32;
        if x + chat_size.width as i32 > right_edge - 8 {
            x = av_pos.x - gap - chat_size.width as i32;
        }
        if x < mpos.x + 8 {
            x = mpos.x + 8;
        }
    }

    chat
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn toggle_chat_panel(app: AppHandle) -> Result<bool, String> {
    let chat = chat_win(&app)?;
    let visible = chat.is_visible().unwrap_or(false);
    if visible {
        let _ = chat.hide();
        Ok(false)
    } else {
        let _ = position_chat_near_avatar(&app);
        let _ = chat.show();
        let _ = chat.set_focus();
        Ok(true)
    }
}

#[tauri::command]
pub fn show_chat_panel(app: AppHandle) -> Result<(), String> {
    let chat = chat_win(&app)?;
    let _ = position_chat_near_avatar(&app);
    let _ = chat.show();
    let _ = chat.set_focus();
    Ok(())
}

#[tauri::command]
pub fn hide_chat_panel(app: AppHandle) -> Result<(), String> {
    let chat = chat_win(&app)?;
    let _ = chat.hide();
    Ok(())
}

#[tauri::command]
pub fn open_console(app: AppHandle) -> Result<(), String> {
    let main = main_win(&app)?;
    let theme = app
        .try_state::<crate::agent::AppState>()
        .map(|s| s.config.read().ui.theme.clone())
        .unwrap_or_else(|| "midnight".into());
    apply_main_window_theme(&app, &theme);
    let _ = main.show();
    let _ = main.set_focus();
    Ok(())
}

#[tauri::command]
pub fn show_avatar(app: AppHandle) -> Result<(), String> {
    let avatar = avatar_win(&app)?;
    let _ = avatar.show();
    Ok(())
}

#[tauri::command]
pub fn reposition_chat(app: AppHandle) -> Result<(), String> {
    let chat = chat_win(&app)?;
    if chat.is_visible().unwrap_or(false) {
        position_chat_near_avatar(&app)?;
    }
    Ok(())
}

#[tauri::command]
pub fn get_avatar_pos(app: AppHandle) -> Result<(i32, i32), String> {
    let win = avatar_win(&app)?;
    {
        let st = overlay().lock();
        if st.enabled {
            return Ok((st.sprite_x, st.sprite_y));
        }
    }
    let pos = win.outer_position().map_err(|e| e.to_string())?;
    Ok((pos.x, pos.y))
}

/// Move the floating avatar by a delta in physical pixels.
/// During roam overlay the OS window stays put — only the sprite origin moves.
#[tauri::command]
pub fn move_avatar_by(app: AppHandle, dx: i32, dy: i32) -> Result<(i32, i32), String> {
    if dx == 0 && dy == 0 {
        return get_avatar_pos(app);
    }
    let win = avatar_win(&app)?;
    {
        let mut st = overlay().lock();
        if st.enabled {
            let x = st.sprite_x.saturating_add(dx);
            let y = st.sprite_y.saturating_add(dy);
            let (x, y) = clamp_sprite_pos(&win, x, y, st.sprite_w, st.sprite_h);
            st.sprite_x = x;
            st.sprite_y = y;
            return Ok((x, y));
        }
    }
    let pos = win.outer_position().map_err(|e| e.to_string())?;
    let x = pos.x.saturating_add(dx);
    let y = pos.y.saturating_add(dy);
    let (x, y) = clamp_win_pos(&win, x, y);
    win.set_position(PhysicalPosition::new(x, y))
        .map_err(|e| format!("set_position: {e}"))?;
    Ok((x, y))
}

fn clamp_win_pos(win: &WebviewWindow, x: i32, y: i32) -> (i32, i32) {
    let Ok(Some(mon)) = win.current_monitor() else {
        return (x, y);
    };
    let Ok(size) = win.outer_size() else {
        return (x, y);
    };
    let mpos = mon.position();
    let msize = mon.size();
    let margin = 8i32;
    let min_x = mpos.x + margin;
    let min_y = mpos.y + margin;
    let max_x = mpos.x + msize.width as i32 - size.width as i32 - margin;
    let max_y = mpos.y + msize.height as i32 - size.height as i32 - margin;
    (
        x.clamp(min_x.min(max_x), max_x.max(min_x)),
        y.clamp(min_y.min(max_y), max_y.max(min_y)),
    )
}

#[tauri::command]
pub fn set_avatar_pos(app: AppHandle, x: i32, y: i32) -> Result<(), String> {
    let win = avatar_win(&app)?;
    {
        let mut st = overlay().lock();
        if st.enabled {
            let (x, y) = clamp_sprite_pos(&win, x, y, st.sprite_w, st.sprite_h);
            st.sprite_x = x;
            st.sprite_y = y;
            return Ok(());
        }
    }
    let (x, y) = clamp_win_pos(&win, x, y);
    win.set_position(PhysicalPosition::new(x, y))
        .map_err(|e| format!("set_position: {e}"))
}

const AVATAR_W: u32 = 168;
const AVATAR_H: u32 = 252;

pub fn avatar_pixel_size() -> (u32, u32) {
    (AVATAR_W, AVATAR_H)
}

pub fn apply_avatar_window_size(app: &AppHandle) {
    let Ok(win) = avatar_win(app) else {
        return;
    };
    let (w, h) = avatar_pixel_size();
    {
        let mut st = overlay().lock();
        st.sprite_w = w;
        st.sprite_h = h;
        if st.enabled {
            drop(st);
            let _ = apply_monitor_overlay(&win);
            return;
        }
    }
    let _ = win.set_size(Size::Logical(LogicalSize::new(w as f64, h as f64)));
    let _ = win.set_background_color(Some(avatar_window_color()));
}

#[tauri::command]
pub fn sync_avatar_window(app: AppHandle) -> Result<(), String> {
    apply_avatar_window_size(&app);
    Ok(())
}

#[tauri::command]
pub fn set_avatar_overlay(app: AppHandle, enabled: bool) -> Result<AvatarDesktop, String> {
    let win = avatar_win(&app)?;
    if enabled {
        {
            let mut st = overlay().lock();
            if !st.enabled {
                if let Ok(pos) = win.outer_position() {
                    st.sprite_x = pos.x;
                    st.sprite_y = pos.y;
                }
                if let Ok(size) = win.outer_size() {
                    if size.width > 80 && size.height > 80 && size.width < 900 {
                        st.sprite_w = size.width;
                        st.sprite_h = size.height;
                    }
                }
                st.enabled = true;
            }
        }
        apply_monitor_overlay(&win)?;
    } else {
        let (x, y, w, h) = {
            let mut st = overlay().lock();
            if !st.enabled {
                drop(st);
                return get_avatar_desktop(app);
            }
            st.enabled = false;
            (st.sprite_x, st.sprite_y, st.sprite_w, st.sprite_h)
        };
        let _ = win.set_ignore_cursor_events(false);
        let _ = win.set_size(Size::Logical(LogicalSize::new(w as f64, h as f64)));
        let (x, y) = clamp_win_pos(&win, x, y);
        let _ = win.set_position(PhysicalPosition::new(x, y));
        let _ = win.set_background_color(Some(avatar_window_color()));
    }
    get_avatar_desktop(app)
}

#[tauri::command]
pub fn set_avatar_sprite_pos(app: AppHandle, x: i32, y: i32) -> Result<(), String> {
    let win = avatar_win(&app)?;
    let mut st = overlay().lock();
    let (x, y) = clamp_sprite_pos(&win, x, y, st.sprite_w, st.sprite_h);
    st.sprite_x = x;
    st.sprite_y = y;
    Ok(())
}

#[tauri::command]
pub fn set_avatar_clickthrough(app: AppHandle, ignore: bool) -> Result<(), String> {
    avatar_win(&app)?
        .set_ignore_cursor_events(ignore)
        .map_err(|e| e.to_string())
}

/// Size the floating window for the 3D buddy and dock it if it was dumped at (0,0).
pub fn place_avatar_on_startup(app: &AppHandle) {
    let Ok(win) = avatar_win(app) else {
        return;
    };
    apply_avatar_window_size(app);
    if let Ok(size) = win.outer_size() {
        crate::debuglog::info(
            None,
            format!("avatar window sized to {}x{} (target {}x{})", size.width, size.height, AVATAR_W, AVATAR_H),
        );
    }
    let Ok(pos) = win.outer_position() else {
        return;
    };
    let Ok(Some(mon)) = win.current_monitor() else {
        return;
    };
    let mpos = mon.position();
    let msize = mon.size();
    let Ok(size) = win.outer_size() else {
        return;
    };
    let win_w = size.width as i32;
    let win_h = size.height as i32;
    let offscreen = pos.x + win_w < mpos.x + 8
        || pos.y + win_h < mpos.y + 8
        || pos.x > mpos.x + msize.width as i32 - 8
        || pos.y > mpos.y + msize.height as i32 - 8;
    let at_origin = pos.x.abs() < 16 && pos.y.abs() < 16;
    if offscreen || at_origin {
        let margin = 28i32;
        let x = mpos.x + msize.width as i32 - win_w - margin;
        let y = mpos.y + msize.height as i32 - win_h - margin - 40;
        let (x, y) = clamp_win_pos(&win, x, y);
        let _ = win.set_position(PhysicalPosition::new(x, y));
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AvatarDesktop {
    pub cursor_x: i32,
    pub cursor_y: i32,
    pub win_x: i32,
    pub win_y: i32,
    pub win_w: u32,
    pub win_h: u32,
    pub mon_x: i32,
    pub mon_y: i32,
    pub mon_w: u32,
    pub mon_h: u32,
    pub overlay: bool,
    pub sprite_x: i32,
    pub sprite_y: i32,
    pub sprite_w: u32,
    pub sprite_h: u32,
}

#[tauri::command]
pub fn get_avatar_desktop(app: AppHandle) -> Result<AvatarDesktop, String> {
    use device_query::{DeviceQuery, DeviceState};

    let win = avatar_win(&app)?;
    let pos = win.outer_position().map_err(|e| e.to_string())?;
    let size = win.outer_size().map_err(|e| e.to_string())?;
    let (mon_x, mon_y, mon_w, mon_h) = match win.current_monitor() {
        Ok(Some(mon)) => {
            let p = mon.position();
            let s = mon.size();
            (p.x, p.y, s.width, s.height)
        }
        _ => (pos.x, pos.y, size.width, size.height),
    };
    let st = overlay().lock();
    let (sprite_x, sprite_y, sprite_w, sprite_h, overlay_on) = if st.enabled {
        (st.sprite_x, st.sprite_y, st.sprite_w, st.sprite_h, true)
    } else {
        (pos.x, pos.y, size.width, size.height, false)
    };
    drop(st);
    let mouse = DeviceState::new().get_mouse();
    Ok(AvatarDesktop {
        cursor_x: mouse.coords.0,
        cursor_y: mouse.coords.1,
        win_x: pos.x,
        win_y: pos.y,
        win_w: size.width,
        win_h: size.height,
        mon_x,
        mon_y,
        mon_w,
        mon_h,
        overlay: overlay_on,
        sprite_x,
        sprite_y,
        sprite_w,
        sprite_h,
    })
}

/// OS-level drag: poll global mouse until left button is released.
/// Needed because the avatar webview is tiny — browser pointer events stop
/// as soon as the cursor leaves the transparent window.
#[tauri::command]
pub fn begin_avatar_drag(app: AppHandle) -> Result<(), String> {
    use device_query::{DeviceQuery, DeviceState};

    let win = avatar_win(&app)?;
    if overlay().lock().enabled {
        return Ok(());
    }
    let start_pos = win.outer_position().map_err(|e| e.to_string())?;

    let device = DeviceState::new();
    let mouse = device.get_mouse();
    let start_mouse = mouse.coords;

    std::thread::spawn(move || {
        use device_query::{DeviceQuery, DeviceState};
        use std::time::{Duration, Instant};

        let device = DeviceState::new();
        let mut last_set = Instant::now() - Duration::from_millis(50);
        let mut ever_pressed = false;
        let started = Instant::now();

        loop {
            let mouse = device.get_mouse();
            let pressed = mouse_left_pressed(&mouse.button_pressed);

            if pressed {
                ever_pressed = true;
                let (mx, my) = mouse.coords;
                let x = start_pos.x + (mx - start_mouse.0);
                let y = start_pos.y + (my - start_mouse.1);
                if last_set.elapsed() >= Duration::from_millis(8) {
                    if let Some(w) = app.get_webview_window("avatar") {
                        let _ = w.set_position(PhysicalPosition::new(x, y));
                    }
                    last_set = Instant::now();
                }
            } else if ever_pressed {
                break;
            } else if started.elapsed() > Duration::from_millis(600) {
                // Never saw a press — abort
                break;
            }

            std::thread::sleep(Duration::from_millis(8));
        }

        let mouse = device.get_mouse();
        let (mx, my) = mouse.coords;
        let x = start_pos.x + (mx - start_mouse.0);
        let y = start_pos.y + (my - start_mouse.1);
        if let Some(w) = app.get_webview_window("avatar") {
            let _ = w.set_position(PhysicalPosition::new(x, y));
        }
        let _ = position_chat_near_avatar(&app);
    });

    Ok(())
}

fn mouse_left_pressed(buttons: &[bool]) -> bool {
    // device_query: index 1 is typically left; some backends use 0.
    buttons.get(1).copied().unwrap_or(false) || buttons.get(0).copied().unwrap_or(false)
}
