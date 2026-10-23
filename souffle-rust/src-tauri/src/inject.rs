//! Insertion du texte là où l'on écrit : presse-papier, puis ⌘V / Ctrl+V.
//!
//! Le texte reste ensuite dans le presse-papier : c'est le filet quand le
//! collage rate. Rendre l'ancien contenu est une option, désactivée par défaut.

use crate::engine::{PasteOpts, PasteOutcome};
use crate::platform;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Exécute une action sur le fil principal et attend sa fin. Sur macOS, la
/// frappe doit lire la disposition du clavier, ce que le système n'autorise
/// que depuis le fil principal.
pub type OnMain<'a> = &'a dyn Fn(Box<dyn FnOnce() + Send>);

pub fn write_clipboard(text: &str) -> bool {
    for attempt in 0..3 {
        if let Ok(mut c) = arboard::Clipboard::new() {
            if c.set_text(text.to_string()).is_ok() {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(40 * (attempt + 1)));
    }
    false
}

pub fn read_clipboard() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

fn send_paste_keystroke() -> Result<(), String> {
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
    let modifier = if cfg!(target_os = "macos") { Key::Meta } else { Key::Control };
    // Windows reconnaît Ctrl+V à la touche virtuelle V, quelle que soit la
    // disposition du clavier. macOS, lui, cherche la touche qui produit « v ».
    let v = if cfg!(target_os = "windows") { Key::Other(0x56) } else { Key::Unicode('v') };
    enigo.key(modifier, Direction::Press).map_err(|e| e.to_string())?;
    let res = enigo.key(v, Direction::Click).map_err(|e| e.to_string());
    // Le modificateur est relâché quoi qu'il arrive : jamais de ⌘ ou de Ctrl coincé.
    let _ = enigo.key(modifier, Direction::Release);
    res
}

/// Appelé hors du fil principal : il patiente (focus, frappe).
pub fn paste(text: &str, opts: &PasteOpts, on_main: OnMain) -> PasteOutcome {
    let previous = if opts.restore_clipboard { read_clipboard() } else { None };
    if !write_clipboard(text) {
        crate::log!("presse-papier inaccessible");
        return PasteOutcome::KeystrokeFailed;
    }
    if !opts.auto_paste {
        return PasteOutcome::ClipboardOnly;
    }
    if !platform::accessibility_granted() {
        return PasteOutcome::NeedsAccessibility;
    }

    // Si l'on a changé de fenêtre pendant la transcription, on revient sur
    // celle où l'on a commencé à parler : c'est là qu'on attend le texte.
    if opts.refocus && opts.target.pid != 0 {
        let now = platform::frontmost();
        if now.pid != opts.target.pid && platform::focus(&opts.target) {
            crate::log!("focus rendu à « {} »", opts.target.name);
            std::thread::sleep(Duration::from_millis(220));
        }
        // Et, dans cette fenêtre, au champ où l'on avait commencé à parler.
        if platform::restore_field(&opts.target) {
            crate::log!("focus rendu au champ de départ");
            std::thread::sleep(Duration::from_millis(80));
        }
    }
    // Laisse le focus se stabiliser avant d'envoyer la frappe.
    std::thread::sleep(Duration::from_millis(120));

    let (tx, rx) = std::sync::mpsc::channel();
    // Une frappe arrivée trop tard (après « Copié ») collerait au mauvais moment.
    let expired = Arc::new(AtomicBool::new(false));
    let late = expired.clone();
    on_main(Box::new(move || {
        if late.load(Ordering::SeqCst) {
            return;
        }
        let _ = tx.send(send_paste_keystroke());
    }));
    match rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            crate::log!("collage impossible : {e}");
            return PasteOutcome::KeystrokeFailed;
        }
        Err(_) => {
            expired.store(true, Ordering::SeqCst);
            crate::log!("collage impossible : pas de réponse du fil principal");
            return PasteOutcome::KeystrokeFailed;
        }
    }

    // Une frappe envoyée ne prouve pas que l'application l'a reçue : restaurer
    // l'ancien presse-papier peut effacer la dictée pour de bon. On ne le fait
    // que sur demande explicite.
    if let Some(prev) = previous {
        let text = text.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1200));
            if read_clipboard().as_deref() == Some(text.as_str()) {
                write_clipboard(&prev);
            }
        });
    }
    PasteOutcome::Pasted
}
