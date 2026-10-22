//! La bulle : une capsule de verre qui flotte au-dessus de tout, sur tous les
//! bureaux et par-dessus le plein écran, sans jamais prendre le focus.

use crate::config::Position;
use crate::AppState;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, PhysicalPosition, Theme, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// Capsule de 280 × 60 points, plus une marge pour la lueur du halo.
pub const WIDTH: f64 = 308.0;
pub const HEIGHT: f64 = 88.0;
pub const MARGIN: f64 = 14.0;
pub const RADIUS: f64 = 30.0;
const BOTTOM_GAP: f64 = 48.0;

pub fn theme_of(name: &str) -> Option<Theme> {
    match name {
        "light" => Some(Theme::Light),
        "dark" => Some(Theme::Dark),
        _ => None,
    }
}

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("bubble")
}

pub fn create(app: &AppHandle, theme: &str) -> tauri::Result<()> {
    let w = WebviewWindowBuilder::new(app, "bubble", WebviewUrl::App("bubble.html".into()))
        .title("Souffle")
        .inner_size(WIDTH, HEIGHT)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .focused(false)
        .focusable(false)
        .visible_on_all_workspaces(true)
        .accept_first_mouse(true)
        .theme(theme_of(theme))
        .build()?;
    // Sous Linux, une fenêtre encore jamais montrée n'a pas de surface : on
    // attend qu'elle s'affiche pour la rendre traversable. Sous Windows, la
    // bulle n'est jamais traversable (voir `set_interactive`).
    if cfg!(target_os = "macos") {
        let _ = w.set_ignore_cursor_events(true);
    }

    #[cfg(target_os = "macos")]
    {
        let w2 = w.clone();
        let _ = app.run_on_main_thread(move || {
            if let Ok(ns) = w2.ns_window() {
                unsafe { crate::platform::macos::setup_bubble(ns, MARGIN, RADIUS) };
            }
        });
    }
    Ok(())
}

pub fn set_theme(app: &AppHandle, theme: &str) {
    if let Some(w) = window(app) {
        let _ = w.set_theme(theme_of(theme));
    }
}

/// Position : la dernière où on l'a posée (en proportion de l'écran), sinon en
/// bas au centre, sur l'écran où se trouve le pointeur. Renvoie le cadre en
/// pixels : x, y, largeur, hauteur.
fn frame(app: &AppHandle) -> Option<(i32, i32, i32, i32)> {
    let state = app.try_state::<AppState>()?;
    let saved = state.engine.cfg().overlay_position;
    let cursor = app.cursor_position().ok();
    let monitor = cursor
        .and_then(|c| app.monitor_from_point(c.x, c.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten())?;
    let scale = monitor.scale_factor();
    let wa = monitor.work_area();
    let (ww, wh) = ((WIDTH * scale) as i32, (HEIGHT * scale) as i32);
    let (ax, ay, aw, ah) = (wa.position.x, wa.position.y, wa.size.width as i32, wa.size.height as i32);
    let (x, y) = match saved {
        Some(Position { fx, fy }) if fx.is_finite() && fy.is_finite() => (
            ax + (fx.clamp(0.0, 1.0) * (aw - ww).max(0) as f64) as i32,
            ay + (fy.clamp(0.0, 1.0) * (ah - wh).max(0) as f64) as i32,
        ),
        _ => (ax + (aw - ww) / 2, ay + ah - wh - (BOTTOM_GAP * scale) as i32 + (MARGIN * scale) as i32),
    };
    Some((x, y, ww, wh))
}

/// Sous Windows, la bulle ne passe jamais par les fonctions de fenêtre de
/// Tauri : montrée sans être activée (pour ne pas voler le focus), elle est
/// crue cachée par Tauri, qui la recacherait au moindre redimensionnement ou
/// déplacement. Tout se fait directement, sur le fil principal, dans l'ordre.
#[cfg(target_os = "windows")]
fn on_main_hwnd(app: &AppHandle, w: &WebviewWindow, f: impl FnOnce(isize) + Send + 'static) {
    if let Ok(h) = w.hwnd() {
        let hwnd = h.0 as isize;
        let _ = app.run_on_main_thread(move || f(hwnd));
    }
}

pub fn show(app: &AppHandle) {
    let Some(w) = window(app) else { return };
    let frame = frame(app);
    #[cfg(target_os = "windows")]
    {
        on_main_hwnd(app, &w, move |h| crate::platform::windows::show_at(h, frame));
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some((x, y, _, _)) = frame {
            let _ = w.set_size(tauri::LogicalSize::new(WIDTH, HEIGHT));
            let _ = w.set_position(PhysicalPosition::new(x, y));
        }
    }
    #[cfg(target_os = "macos")]
    {
        let w2 = w.clone();
        let _ = app.run_on_main_thread(move || {
            if let Ok(ns) = w2.ns_window() {
                unsafe { crate::platform::macos::order_front(ns) };
            }
        });
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = w.show();
    }
}

pub fn hide(app: &AppHandle) {
    let Some(w) = window(app) else { return };
    #[cfg(target_os = "windows")]
    {
        on_main_hwnd(app, &w, crate::platform::windows::hide);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = w.hide();
    }
}

/// Bulle cliquable (dictée en cours, erreur rattrapable) ou traversée par la
/// souris. Sous Windows, une fenêtre traversable doit être « superposée »
/// (WS_EX_LAYERED), ce qui fige l'affichage de WebView2 : la bulle y reste
/// donc toujours cliquable, et ignore elle-même les clics hors dictée.
pub fn set_interactive(app: &AppHandle, on: bool) {
    if cfg!(target_os = "windows") {
        return;
    }
    if let Some(w) = window(app) {
        if cfg!(target_os = "linux") && !w.is_visible().unwrap_or(false) {
            return;
        }
        let _ = w.set_ignore_cursor_events(!on);
    }
}

/* ---------- Déplacement ---------- */

static DRAG: Mutex<Option<(PhysicalPosition<i32>, f64)>> = Mutex::new(None);

pub fn drag_start(app: &AppHandle) {
    let Some(w) = window(app) else { return };
    if let (Ok(pos), Ok(scale)) = (w.outer_position(), w.scale_factor()) {
        *DRAG.lock().unwrap_or_else(|e| e.into_inner()) = Some((pos, scale));
    }
}

/// `dx`, `dy` : déplacement du pointeur depuis le début du glisser, en points.
pub fn drag_move(app: &AppHandle, dx: f64, dy: f64) {
    let Some((origin, scale)) = *DRAG.lock().unwrap_or_else(|e| e.into_inner()) else { return };
    let Some(w) = window(app) else { return };
    let (x, y) = (origin.x + (dx * scale) as i32, origin.y + (dy * scale) as i32);
    #[cfg(target_os = "windows")]
    {
        on_main_hwnd(app, &w, move |h| crate::platform::windows::move_to(h, x, y));
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = w.set_position(PhysicalPosition::new(x, y));
    }
}

/// Position gardée en proportion de l'écran : la bulle revient au même endroit
/// relatif, quel que soit l'écran où commence la dictée suivante.
pub fn drag_end(app: &AppHandle) {
    if DRAG.lock().unwrap_or_else(|e| e.into_inner()).take().is_none() {
        return;
    }
    let Some(w) = window(app) else { return };
    let (Ok(pos), Ok(size)) = (w.outer_position(), w.outer_size()) else { return };
    let center = (pos.x as f64 + size.width as f64 / 2.0, pos.y as f64 + size.height as f64 / 2.0);
    let Some(m) = app.monitor_from_point(center.0, center.1).ok().flatten().or_else(|| w.current_monitor().ok().flatten())
    else {
        return;
    };
    let wa = m.work_area();
    let span_x = (wa.size.width as f64 - size.width as f64).max(1.0);
    let span_y = (wa.size.height as f64 - size.height as f64).max(1.0);
    let p = Position {
        fx: ((pos.x - wa.position.x) as f64 / span_x).clamp(0.0, 1.0),
        fy: ((pos.y - wa.position.y) as f64 / span_y).clamp(0.0, 1.0),
    };
    if let Some(state) = app.try_state::<AppState>() {
        state.engine.settings.update(|c| c.overlay_position = Some(p));
        crate::tray::refresh(app);
    }
    crate::log!("bulle déplacée et mémorisée");
}
