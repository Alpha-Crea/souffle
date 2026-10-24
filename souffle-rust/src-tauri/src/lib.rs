//! Souffle — dictée vocale système.

pub mod journal;

pub mod ai;
pub mod audio;
pub mod bubble;
pub mod commands;
pub mod config;
pub mod desktop;
pub mod engine;
pub mod history;
pub mod inject;
pub mod pending;
pub mod platform;
pub mod settings_window;
pub mod shortcuts;
pub mod tray;

use config::{Config, Secrets};
use engine::{Engine, Settings};
use history::History;
use pending::Pending;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;
use tauri::Manager;

pub struct AppState {
    pub engine: Arc<Engine>,
    pub secrets: Arc<Mutex<Secrets>>,
    pub http: reqwest::Client,
    pub data_dir: PathBuf,
    pub shortcut_error: Mutex<Option<String>>,
    pub imported_from_electron: bool,
}

/// Premier lancement : réglages, historique et dictées en attente de la
/// version Electron sont repris tels quels. Les clés API, chiffrées par
/// Electron, sont à recoller une fois.
fn load_config(dir: &std::path::Path) -> (Config, bool) {
    let file = dir.join("config.json");
    if let Some(cfg) = Config::load(&file) {
        return (cfg, false);
    }
    if let Some(old) = config::electron_dir().and_then(|d| Config::load(&d.join("config.json"))) {
        let cfg = old.migrated_from_electron();
        let _ = cfg.save(&file);
        log!("réglages repris de la version Electron");
        return (cfg, true);
    }
    (Config::default(), false)
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let dir = config::data_dir();
    std::fs::create_dir_all(&dir)?;
    journal::init(&dir);
    log!("démarrage — Souffle {} · {} · {}", app.package_info().version, std::env::consts::OS, std::env::consts::ARCH);

    let (mut cfg, imported) = load_config(&dir);
    if cfg.upgrade() {
        let _ = cfg.save(&dir.join("config.json"));
    }
    let secrets = Arc::new(Mutex::new(Secrets::new(&dir, cfg!(any(target_os = "macos", target_os = "windows")))));
    let pending = Arc::new(Pending::new(dir.join("en-attente")));
    let mut history = History::open(&dir);
    if imported {
        if let Some(old) = config::electron_dir() {
            let n = history.import(&old.join("history.json"), cfg.history_limit);
            let m = pending.import_dir(&old.join("en-attente"));
            log!("historique repris : {n} entrée(s) · morceaux en attente repris : {m}");
        }
    }

    let http = ai::client();
    let engine_slot: Arc<OnceLock<Weak<Engine>>> = Arc::new(OnceLock::new());
    let slot = engine_slot.clone();
    let capture = audio::capture::Capture::spawn(
        pending.clone(),
        Arc::new(move |ev| {
            if let Some(e) = slot.get().and_then(Weak::upgrade) {
                e.on_capture(ev);
            }
        }),
    );

    let theme = cfg.theme.clone();
    let onboarded = cfg.onboarded;
    let launch_at_login = cfg.launch_at_login;
    let settings = Arc::new(Settings::new(cfg, Some(dir.join("config.json"))));
    let engine = Engine::new(
        settings,
        history,
        pending,
        Arc::new(desktop::RealAi { http: http.clone(), secrets: secrets.clone() }),
        Arc::new(desktop::TauriDesktop { app: app.handle().clone() }),
        Arc::new(desktop::CaptureRecorder(capture)),
        tauri::async_runtime::handle().inner().clone(),
    );
    let _ = engine_slot.set(Arc::downgrade(&engine));

    app.manage(AppState {
        engine: engine.clone(),
        secrets,
        http,
        data_dir: dir,
        shortcut_error: Mutex::new(None),
        imported_from_electron: imported,
    });

    // Application de barre de menus : pas d'icône dans le Dock.
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);

    let handle = app.handle().clone();
    bubble::create(&handle, &theme)?;
    tray::create(&handle)?;
    let _ = shortcuts::register_main(&handle);

    {
        use tauri_plugin_autostart::ManagerExt;
        let al = handle.autolaunch();
        if launch_at_login && !al.is_enabled().unwrap_or(false) {
            let _ = al.enable();
        }
    }

    // Sans l'Accessibilité, Souffle ne peut ni coller le texte ni entendre
    // Échap : on la demande dès le lancement plutôt qu'au premier échec.
    if !platform::accessibility_granted() {
        platform::ask_accessibility();
    }

    log!(
        "prêt — micro : {:?} · accessibilité : {}",
        platform::mic_permission(),
        platform::accessibility_granted()
    );

    if !onboarded {
        let has_key = handle.state::<AppState>().secrets.lock().map(|s| s.status().values().any(|v| *v)).unwrap_or(false);
        settings_window::open(&handle, Some(if has_key { "general" } else { "intelligence" }));
        engine.settings.update(|c| c.onboarded = true);
    }

    // Les dictées restées sur le disque (plantage, ordinateur éteint pendant
    // une transcription) sont retranscrites une fois le réseau prêt.
    let e = engine.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        e.recover_pending().await;
    });
    Ok(())
}

pub fn run() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            settings_window::open(app, None);
        }))
        .plugin(shortcuts::plugin())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .invoke_handler(commands::handler())
        .setup(setup)
        .build(tauri::generate_context!());
    match result {
        Ok(app) => app.run(|_app, event| {
            // Application de barre de menus : fermer les réglages ne quitte pas.
            if let tauri::RunEvent::ExitRequested { api, code: None, .. } = event {
                api.prevent_exit();
            }
        }),
        Err(e) => {
            log!("ERREUR au démarrage : {e}");
            std::process::exit(1);
        }
    }
}
