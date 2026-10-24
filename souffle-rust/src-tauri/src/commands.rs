//! Commandes appelées par les pages (réglages, bulle).

use crate::config::{key_name_for, SECRET_KEYS};
use crate::engine::State as Dictee;
use crate::history::Entry;
use crate::{ai, audio, bubble, inject, platform, settings_window, shortcuts, tray, AppState};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;

fn settings_view(app: &AppHandle, state: &AppState) -> Value {
    let cfg = state.engine.cfg();
    let mut v = serde_json::to_value(&cfg).unwrap_or_else(|_| json!({}));
    let secrets = state.secrets.lock().map(|s| s.status()).unwrap_or_default();
    v["secrets"] = json!(secrets);
    v["platform"] = json!(std::env::consts::OS);
    v["version"] = json!(app.package_info().version.to_string());
    v["accessibility"] = json!(platform::accessibility_granted());
    v["microphonePermission"] = json!(format!("{:?}", platform::mic_permission()).to_lowercase());
    v["shortcutPretty"] = json!(shortcuts::pretty(&cfg.shortcut));
    v["shortcutError"] = json!(state.shortcut_error.lock().ok().and_then(|e| e.clone()));
    v["dataDir"] = json!(state.data_dir.display().to_string());
    v["waitingOnDisk"] = json!(state.engine.snapshot().waiting_on_disk);
    v["importedFromElectron"] = json!(state.imported_from_electron);
    v
}

#[tauri::command]
pub fn get_settings(app: AppHandle, state: State<'_, AppState>) -> Value {
    settings_view(&app, &state)
}

#[tauri::command]
pub fn set_settings(app: AppHandle, state: State<'_, AppState>, patch: Value) -> Result<Value, String> {
    let before = state.engine.cfg();
    let next = before.patched(&patch)?;
    if next.shortcut != before.shortcut {
        shortcuts::validate(&next.shortcut)?;
    }
    state.engine.settings.update(|c| *c = next.clone());

    if next.shortcut != before.shortcut {
        if let Err(e) = shortcuts::register_main(&app) {
            // Raccourci refusé par le système : on revient à l'ancien, qui marchait.
            state.engine.settings.update(|c| c.shortcut = before.shortcut.clone());
            let _ = shortcuts::register_main(&app);
            return Err(format!("Ce raccourci est déjà pris ({e}). L'ancien est conservé."));
        }
    }
    if next.launch_at_login != before.launch_at_login {
        let al = app.autolaunch();
        let res = if next.launch_at_login { al.enable() } else { al.disable() };
        if let Err(e) = res {
            crate::log!("lancement au démarrage indisponible : {e}");
        }
    }
    if next.theme != before.theme {
        bubble::set_theme(&app, &next.theme);
        settings_window::set_theme(&app, &next.theme);
    }
    tray::refresh(&app);
    Ok(settings_view(&app, &state))
}

#[tauri::command]
pub fn set_secret(state: State<'_, AppState>, key: String, value: String) -> Value {
    let mut s = state.secrets.lock().unwrap_or_else(|e| e.into_inner());
    if SECRET_KEYS.contains(&key.as_str()) {
        s.set(&key, &value);
    }
    json!(s.status())
}

#[tauri::command]
pub async fn test_key(state: State<'_, AppState>, provider: String, key: String, custom_url: String) -> Result<Value, String> {
    let key = if key.trim().is_empty() {
        state.secrets.lock().map(|s| s.get(key_name_for(&provider))).unwrap_or_default()
    } else {
        key.trim().to_string()
    };
    Ok(match ai::test_key(&state.http, &provider, &key, &custom_url).await {
        Ok(n) => json!({ "ok": true, "models": n }),
        Err(e) => json!({ "ok": false, "error": ai::human_error(&e) }),
    })
}

#[tauri::command]
pub fn history_list(state: State<'_, AppState>) -> Vec<Entry> {
    state.engine.history.lock().map(|h| h.list().to_vec()).unwrap_or_default()
}

#[tauri::command]
pub fn history_clear(state: State<'_, AppState>) {
    if let Ok(mut h) = state.engine.history.lock() {
        h.clear();
    }
}

/// Le presse-papier du système écrit toujours, même quand la page n'a pas le focus.
#[tauri::command]
pub fn copy_text(text: String) -> bool {
    !text.is_empty() && inject::write_clipboard(&text)
}

#[tauri::command]
pub fn open_accessibility() {
    platform::prompt_accessibility();
}

#[tauri::command]
pub async fn request_microphone(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.engine.desk().request_mic().await)
}

#[tauri::command]
pub fn open_microphone_settings() {
    platform::open_mic_settings();
}

#[tauri::command]
pub fn open_data_folder(state: State<'_, AppState>) {
    platform::open_url(&state.data_dir.display().to_string());
}

#[tauri::command]
pub fn open_link(url: String) {
    if url.starts_with("https://") {
        platform::open_url(&url);
    }
}

#[tauri::command]
pub fn microphones() -> Vec<String> {
    audio::capture::microphones()
}

#[tauri::command]
pub fn dictate(state: State<'_, AppState>) {
    state.engine.toggle();
}

#[tauri::command]
pub fn retry_pending(state: State<'_, AppState>) {
    state.engine.recover_pending_in_background();
}

#[tauri::command]
pub fn validate_shortcut(accel: String) -> Result<String, String> {
    shortcuts::validate(&accel).map(|_| shortcuts::pretty(&accel))
}

#[tauri::command]
pub fn shortcut_capture(app: AppHandle, on: bool) {
    shortcuts::capture(&app, on);
}

/* ---------- Bulle ---------- */

#[tauri::command]
pub fn bubble_click(state: State<'_, AppState>) {
    state.engine.bubble_clicked();
}

/// Clic droit : un petit menu plutôt qu'une annulation immédiate. Un clic
/// droit malencontreux ne doit jamais faire perdre une dictée.
#[tauri::command]
pub fn bubble_right_click(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    let s = state.engine.state();
    if !matches!(s, Dictee::Recording | Dictee::Paused) {
        return Ok(());
    }
    let e = |err: tauri::Error| err.to_string();
    let menu = Menu::new(&app).map_err(e)?;
    menu.append(&MenuItem::with_id(&app, "dictate", "Terminer et écrire", true, None::<&str>).map_err(e)?).map_err(e)?;
    let pause = if s == Dictee::Paused { "Reprendre" } else { "Mettre en pause" };
    menu.append(&MenuItem::with_id(&app, "pause", pause, true, None::<&str>).map_err(e)?).map_err(e)?;
    menu.append(&PredefinedMenuItem::separator(&app).map_err(e)?).map_err(e)?;
    menu.append(&MenuItem::with_id(&app, "cancel", "Annuler la dictée", true, None::<&str>).map_err(e)?).map_err(e)?;
    if let Some(w) = app.get_webview_window("bubble") {
        w.popup_menu(&menu).map_err(e)?;
    }
    Ok(())
}

#[tauri::command]
pub fn bubble_drag_start(app: AppHandle) {
    bubble::drag_start(&app);
}

#[tauri::command]
pub fn bubble_drag_move(app: AppHandle, dx: f64, dy: f64) {
    bubble::drag_move(&app, dx, dy);
}

#[tauri::command]
pub fn bubble_drag_end(app: AppHandle) {
    bubble::drag_end(&app);
}

#[tauri::command]
pub fn bubble_hello(app: AppHandle) -> Value {
    let platform = std::env::consts::OS;
    let sounds = app.try_state::<AppState>().map(|s| s.engine.cfg().play_sounds).unwrap_or(true);
    json!({ "platform": platform, "sounds": sounds, "glass": cfg!(target_os = "macos") })
}

pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        get_settings,
        set_settings,
        set_secret,
        test_key,
        history_list,
        history_clear,
        copy_text,
        open_accessibility,
        request_microphone,
        open_microphone_settings,
        open_data_folder,
        open_link,
        microphones,
        dictate,
        retry_pending,
        validate_shortcut,
        shortcut_capture,
        bubble_click,
        bubble_right_click,
        bubble_drag_start,
        bubble_drag_move,
        bubble_drag_end,
        bubble_hello,
    ]
}
