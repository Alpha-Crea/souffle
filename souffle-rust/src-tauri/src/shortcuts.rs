//! Raccourci global (bascule ou maintien) et Échap pendant la dictée.

use crate::AppState;
use std::str::FromStr;
use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

const ESCAPE: &str = "Escape";

pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app: &AppHandle<R>, shortcut: &Shortcut, event: ShortcutEvent| {
            let Some(state) = app.try_state::<AppState>() else { return };
            let engine = state.engine.clone();
            if Shortcut::from_str(ESCAPE).is_ok_and(|e| &e == shortcut) {
                if event.state == ShortcutState::Pressed {
                    engine.escape_pressed();
                }
                return;
            }
            match event.state {
                ShortcutState::Pressed => engine.shortcut_pressed(),
                ShortcutState::Released => engine.shortcut_released(),
            }
        })
        .build()
}

/// Vérifie qu'un raccourci est utilisable, sans l'enregistrer.
pub fn validate(accel: &str) -> Result<(), String> {
    let s = Shortcut::from_str(accel).map_err(|e| format!("raccourci illisible : {e}"))?;
    if s.mods.is_empty() && !matches!(accel.to_lowercase().as_str(), s if s.starts_with('f') && s[1..].parse::<u8>().is_ok()) {
        // Une touche nue (lettre, Espace…) se déclencherait à chaque frappe.
        return Err("ajoutez au moins une touche de modification (⌥, ⌃, ⇧ ou ⌘)".into());
    }
    Ok(())
}

/// (Ré)enregistre le raccourci principal. Le raccourci doit survivre à tout :
/// en cas d'échec, l'erreur est gardée pour les réglages.
pub fn register_main(app: &AppHandle) -> Result<(), String> {
    let Some(state) = app.try_state::<AppState>() else { return Err("application pas prête".into()) };
    let accel = state.engine.cfg().shortcut;
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let res = validate(&accel).and_then(|_| gs.register(accel.as_str()).map_err(|e| e.to_string()));
    match &res {
        Ok(()) => {
            crate::log!("raccourci « {accel} » enregistré");
            *state.shortcut_error.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
        Err(e) => {
            crate::log!("raccourci « {accel} » refusé : {e}");
            *state.shortcut_error.lock().unwrap_or_else(|e| e.into_inner()) =
                Some(format!("Le raccourci « {accel} » est déjà pris ou refusé par le système. Choisissez-en un autre."));
        }
    }
    res
}

/// Échap n'est écouté que pendant une dictée : le reste du temps, rien.
pub fn escape_watch(app: &AppHandle, on: bool) {
    #[cfg(target_os = "macos")]
    {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let h = handle.clone();
            crate::platform::macos::escape_monitor(
                on,
                Arc::new(move || {
                    if let Some(s) = h.try_state::<AppState>() {
                        s.engine.escape_pressed();
                    }
                }),
            );
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Jamais depuis l'intérieur d'un rappel de raccourci : on repousse.
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let gs = app.global_shortcut();
            if on {
                if !gs.is_registered(ESCAPE) {
                    if let Err(e) = gs.register(ESCAPE) {
                        crate::log!("Échap indisponible : {e}");
                    }
                }
            } else if gs.is_registered(ESCAPE) {
                let _ = gs.unregister(ESCAPE);
            }
        });
        let _ = Arc::new(());
    }
}

/// Affichage lisible : « ⌥ Espace » sur Mac, « Ctrl + Maj + Espace » ailleurs.
pub fn pretty(accel: &str) -> String {
    let mac = cfg!(target_os = "macos");
    accel
        .split('+')
        .map(|p| {
            let p = p.trim();
            match p.to_lowercase().as_str() {
                "alt" | "option" => if mac { "⌥" } else { "Alt" }.to_string(),
                "control" | "ctrl" => if mac { "⌃" } else { "Ctrl" }.to_string(),
                "shift" => if mac { "⇧" } else { "Maj" }.to_string(),
                "command" | "cmd" | "super" | "meta" => if mac { "⌘" } else { "Win" }.to_string(),
                "commandorcontrol" | "cmdorctrl" => if mac { "⌘" } else { "Ctrl" }.to_string(),
                "space" => "Espace".to_string(),
                "escape" | "esc" => "Échap".to_string(),
                _ => p.to_uppercase(),
            }
        })
        .collect::<Vec<_>>()
        .join(if mac { " " } else { " + " })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts() {
        assert!(validate("Alt+Space").is_ok());
        assert!(validate("Control+Shift+Space").is_ok());
        assert!(validate("CommandOrControl+Shift+D").is_ok());
        assert!(validate("F8").is_ok());
        assert!(validate("Space").is_err());
        assert!(validate("A").is_err());
        assert!(validate("Hyper+Truc").is_err());
        let p = pretty("Control+Shift+Space");
        assert!(p.contains("Espace"));
    }
}
