//! Barre de menus (macOS) / zone de notification (Windows).

use crate::engine::State;
use crate::{settings_window, shortcuts, AppState};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

const SPOKEN_LANGUAGES: [(&str, &str); 7] = [
    ("auto", "Détection automatique"),
    ("fr", "Français"),
    ("en", "English"),
    ("de", "Deutsch"),
    ("es", "Español"),
    ("it", "Italiano"),
    ("pt", "Português"),
];

fn icon() -> Image<'static> {
    #[cfg(target_os = "macos")]
    let bytes: &[u8] = include_bytes!("../icons/tray-template@2x.png");
    #[cfg(not(target_os = "macos"))]
    let bytes: &[u8] = include_bytes!("../icons/tray.png");
    Image::from_bytes(bytes).expect("icône de la barre de menus")
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    TrayIconBuilder::with_id("main")
        .icon(icon())
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("Souffle — prêt")
        .menu(&menu)
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(on_menu)
        .on_tray_icon_event(|tray, event| {
            // Windows : un clic gauche ouvre les réglages, le clic droit le menu.
            if cfg!(not(target_os = "macos")) {
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                    settings_window::open(tray.app_handle(), None);
                }
            }
        })
        .build(app)?;
    Ok(())
}

pub fn refresh(app: &AppHandle) {
    let app2 = app.clone();
    // Les menus se reconstruisent sur le fil principal.
    let _ = app.run_on_main_thread(move || {
        let Some(tray) = app2.tray_by_id("main") else { return };
        if let Ok(menu) = build_menu(&app2) {
            let _ = tray.set_menu(Some(menu));
        }
        let state = app2.try_state::<AppState>().map(|s| s.engine.state()).unwrap_or_default();
        let tip = match state {
            State::Idle => "Souffle — prêt",
            State::Recording => "Souffle — écoute…",
            State::Paused => "Souffle — en pause",
            State::Working => "Souffle — transcription…",
            State::Done => "Souffle — inséré",
            State::Error => "Souffle — erreur",
        };
        let _ = tray.set_tooltip(Some(tip));
    });
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let state = app.state::<AppState>();
    let engine = &state.engine;
    let snap = engine.snapshot();
    let cfg = engine.cfg();
    let recording = matches!(snap.state, State::Recording | State::Paused);

    let menu = Menu::new(app)?;
    menu.append(&MenuItem::with_id(app, "dictate", if recording { "Terminer la dictée" } else { "Dicter maintenant" }, true, None::<&str>)?)?;
    if recording {
        let label = if snap.state == State::Paused { "Reprendre" } else { "Mettre en pause" };
        menu.append(&MenuItem::with_id(app, "pause", label, true, None::<&str>)?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "info-shortcut", format!("Raccourci : {}", shortcuts::pretty(&cfg.shortcut)), false, None::<&str>)?)?;
    let saved = (cfg.stats.words as f64 / 45.0 - cfg.stats.seconds as f64 / 60.0).max(0.0).round() as u64;
    menu.append(&MenuItem::with_id(
        app,
        "info-stats",
        format!("{} mots dictés · ~{} min gagnées", group_thousands(cfg.stats.words), saved),
        false,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;

    // Whisper ne détecte qu'une langue par enregistrement : forcer la bonne avant
    // de parler vaut mieux qu'espérer qu'il devine. Ce menu rend le changement instantané.
    let lang = Submenu::with_id(app, "lang", "Langue parlée", true)?;
    for (code, label) in SPOKEN_LANGUAGES {
        lang.append(&CheckMenuItem::with_id(app, format!("lang:{code}"), label, true, cfg.language == code, None::<&str>)?)?;
    }
    menu.append(&lang)?;
    menu.append(&CheckMenuItem::with_id(app, "live", "Mode direct", true, cfg.live_mode, None::<&str>)?)?;
    menu.append(&CheckMenuItem::with_id(app, "privacy", "Mode confidentialité", true, cfg.privacy_mode, None::<&str>)?)?;

    if snap.has_failed || snap.waiting_on_disk > 0 {
        menu.append(&PredefinedMenuItem::separator(app)?)?;
        if snap.has_failed {
            menu.append(&MenuItem::with_id(app, "retry", "Réessayer la dernière dictée", true, None::<&str>)?)?;
        }
        if snap.waiting_on_disk > 0 {
            let n = snap.waiting_on_disk;
            let s = if n > 1 { "s" } else { "" };
            menu.append(&MenuItem::with_id(app, "recover", format!("Retranscrire {n} dictée{s} en attente"), true, None::<&str>)?)?;
        }
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "recenter", "Replacer la bulle au centre", cfg.overlay_position.is_some(), None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "settings", "Réglages…", true, Some("CmdOrCtrl+,"))?)?;
    menu.append(&MenuItem::with_id(app, "history", "Historique…", true, None::<&str>)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "quit", "Quitter Souffle", true, None::<&str>)?)?;
    Ok(menu)
}

fn group_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push('\u{202f}');
        }
        out.push(c);
    }
    out
}

fn on_menu(app: &AppHandle, event: MenuEvent) {
    let Some(state) = app.try_state::<AppState>() else { return };
    let engine = state.engine.clone();
    let id = event.id().as_ref().to_string();
    match id.as_str() {
        "dictate" => engine.toggle(),
        "pause" => engine.toggle_pause(),
        "cancel" => engine.cancel(),
        "live" => {
            engine.settings.update(|c| c.live_mode = !c.live_mode);
            settings_window::reload(app);
        }
        "privacy" => {
            engine.settings.update(|c| c.privacy_mode = !c.privacy_mode);
            settings_window::reload(app);
        }
        "retry" => engine.retry_last(),
        "recover" => engine.recover_pending_in_background(),
        "recenter" => {
            engine.settings.update(|c| c.overlay_position = None);
        }
        "settings" => settings_window::open(app, None),
        "history" => settings_window::open(app, Some("history")),
        "quit" => app.exit(0),
        other => {
            if let Some(code) = other.strip_prefix("lang:") {
                let code = code.to_string();
                crate::log!("langue parlée → {code}");
                engine.settings.update(|c| c.language = code);
                settings_window::reload(app);
            }
        }
    }
    refresh(app);
}

#[cfg(test)]
mod tests {
    #[test]
    fn thousands() {
        assert_eq!(super::group_thousands(1234567), "1\u{202f}234\u{202f}567");
        assert_eq!(super::group_thousands(12), "12");
    }
}
