//! Fenêtre de réglages.

use crate::bubble::theme_of;
use crate::AppState;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub fn open(app: &AppHandle, tab: Option<&str>) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        if let Some(t) = tab {
            let _ = w.emit("settings:tab", t);
        }
        return;
    }
    let theme = app.try_state::<AppState>().map(|s| s.engine.cfg().theme).unwrap_or_default();
    let url = match tab {
        Some(t) => format!("settings.html#{t}"),
        None => "settings.html".into(),
    };
    let builder = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App(url.into()))
        .title("Souffle — Réglages")
        .inner_size(940.0, 700.0)
        .min_inner_size(760.0, 560.0)
        .center()
        .theme(theme_of(&theme));
    #[cfg(target_os = "macos")]
    let builder = builder.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true);
    match builder.build() {
        Ok(w) => {
            let _ = w.set_focus();
        }
        Err(e) => crate::log!("réglages : ouverture impossible : {e}"),
    }
}

/// Le menu a changé un réglage : la fenêtre ouverte se met à jour.
pub fn reload(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.emit("settings:changed", ());
    }
}

pub fn set_theme(app: &AppHandle, theme: &str) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.set_theme(theme_of(theme));
        let _ = w.emit("settings:theme", theme);
    }
}
