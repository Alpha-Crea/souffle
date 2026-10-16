//! Le moteur branché sur le vrai système : Tauri, le micro, les fournisseurs.

use crate::ai::{self, AiError};
use crate::audio::beep::{self, Beep};
use crate::audio::capture::{Capture, Cmd};
use crate::config::{Config, Secrets};
use crate::engine::{Ai, Desktop, MicPermission, PasteOpts, PasteOutcome, Recorder, Target, View};
use crate::pending::Audio;
use crate::{bubble, inject, platform, tray, AppState};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

pub struct RealAi {
    pub http: reqwest::Client,
    pub secrets: Arc<Mutex<Secrets>>,
}

impl RealAi {
    fn keys(&self) -> impl Fn(&str) -> String + Send + Sync + '_ {
        move |name: &str| self.secrets.lock().map(|s| s.get(name)).unwrap_or_default()
    }
}

#[async_trait]
impl Ai for RealAi {
    async fn transcribe(&self, cfg: &Config, audio: &Audio, context: &str) -> Result<String, AiError> {
        ai::transcribe(&self.http, cfg, &self.keys(), audio, context).await
    }

    async fn format(&self, cfg: &Config, text: &str, app: &str, context: &str) -> String {
        ai::format(&self.http, cfg, &self.keys(), text, app, context).await
    }
}

pub struct CaptureRecorder(pub Capture);

impl Recorder for CaptureRecorder {
    fn send(&self, cmd: Cmd) {
        self.0.send(cmd);
    }
}

pub struct TauriDesktop {
    pub app: AppHandle,
}

impl TauriDesktop {
    fn refocus_enabled(&self) -> bool {
        self.app.try_state::<AppState>().map(|s| s.engine.cfg().refocus_target).unwrap_or(true)
    }
}

#[async_trait]
impl Desktop for TauriDesktop {
    fn view(&self, view: &View) {
        let _ = self.app.emit_to("bubble", "bubble:view", view);
    }

    fn level(&self, level: f32) {
        let _ = self.app.emit_to("bubble", "bubble:level", level);
    }

    fn show_bubble(&self) {
        bubble::show(&self.app);
    }

    fn hide_bubble(&self) {
        bubble::hide(&self.app);
    }

    fn set_interactive(&self, on: bool) {
        bubble::set_interactive(&self.app, on);
    }

    fn tray_changed(&self) {
        tray::refresh(&self.app);
    }

    fn frontmost(&self) -> Target {
        platform::frontmost()
    }

    async fn paste(&self, text: String, opts: PasteOpts) -> PasteOutcome {
        let app = self.app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let on_main = |f: Box<dyn FnOnce() + Send>| {
                if app.run_on_main_thread(f).is_err() {
                    crate::log!("fil principal indisponible");
                }
            };
            inject::paste(&text, &opts, &on_main)
        })
        .await
        .unwrap_or(PasteOutcome::KeystrokeFailed)
    }

    fn copy(&self, text: &str) {
        inject::write_clipboard(text);
    }

    fn beep(&self, b: Beep) {
        beep::play(b);
    }

    fn mic_permission(&self) -> MicPermission {
        platform::mic_permission()
    }

    async fn request_mic(&self) -> bool {
        let (tx, rx) = tokio::sync::oneshot::channel();
        platform::request_mic(Box::new(move |granted| {
            let _ = tx.send(granted);
        }));
        rx.await.unwrap_or(false)
    }

    fn open_mic_settings(&self) {
        platform::open_mic_settings();
    }

    fn open_accessibility_settings(&self) {
        platform::prompt_accessibility();
    }

    fn escape_watch(&self, on: bool) {
        crate::shortcuts::escape_watch(&self.app, on);
    }

    fn restore_focus(&self, target: &Target) {
        if !self.refocus_enabled() || target.pid == 0 {
            return;
        }
        // Le clic sur la bulle a pu faire passer Souffle au premier plan :
        // on rend aussitôt la main à l'application où l'on écrit.
        if platform::frontmost().pid == std::process::id() as i64 {
            let t = target.clone();
            std::thread::spawn(move || {
                platform::focus(&t);
            });
        }
    }
}
