//! Machine à états et pipeline : audio -> texte -> insertion.
//!
//! Le moteur ne connaît ni Tauri ni le système : il parle à trois interfaces
//! (`Ai`, `Desktop`, `Recorder`). C'est ce qui permet de le tester de bout en
//! bout, pause, réessai et mode direct compris, sans micro ni fenêtre.

use crate::ai::{self, AiError};
use crate::audio::beep::Beep;
use crate::audio::capture::{Cmd, Event, Part};
use crate::config::Config;
use crate::history::{Entry, History};
use crate::pending::{Audio, Pending};
use async_trait::async_trait;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, RwLock, Weak};
use std::time::Duration;
use tokio::time::Instant;
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/* ------------------------------------------------------------------ */
/* Interfaces                                                          */
/* ------------------------------------------------------------------ */

/// Application qui avait le focus au début de la dictée.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Target {
    pub name: String,
    pub pid: i64,
    /// Fenêtre (Windows) : c'est elle qui retrouve le focus, pas seulement l'application.
    pub window: isize,
}

#[derive(Clone, Debug)]
pub struct PasteOpts {
    pub auto_paste: bool,
    pub refocus: bool,
    pub restore_clipboard: bool,
    pub target: Target,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PasteOutcome {
    Pasted,
    ClipboardOnly,
    NeedsAccessibility,
    KeystrokeFailed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MicPermission {
    Granted,
    Denied,
    Unknown,
}

#[async_trait]
pub trait Ai: Send + Sync {
    async fn transcribe(&self, cfg: &Config, audio: &Audio, context: &str) -> Result<String, AiError>;
    async fn format(&self, cfg: &Config, text: &str, app: &str, context: &str) -> String;
    /// Prépare la connexion pendant que l'on parle.
    async fn prewarm(&self, _cfg: &Config) {}
}

#[async_trait]
pub trait Desktop: Send + Sync {
    fn view(&self, view: &View);
    fn level(&self, level: f32);
    fn show_bubble(&self);
    fn hide_bubble(&self);
    /// La bulle reçoit les clics (dictée, erreur rattrapable) ou les laisse passer.
    fn set_interactive(&self, on: bool);
    fn tray_changed(&self);
    fn frontmost(&self) -> Target;
    async fn paste(&self, text: String, opts: PasteOpts) -> PasteOutcome;
    fn copy(&self, text: &str);
    fn beep(&self, beep: Beep);
    fn mic_permission(&self) -> MicPermission;
    async fn request_mic(&self) -> bool;
    fn open_mic_settings(&self);
    fn open_accessibility_settings(&self);
    fn escape_watch(&self, on: bool);
    /// Rend le focus à l'application visée (après un clic sur la bulle).
    fn restore_focus(&self, target: &Target);
}

pub trait Recorder: Send + Sync {
    fn send(&self, cmd: Cmd);
}

/* ------------------------------------------------------------------ */
/* Réglages partagés                                                   */
/* ------------------------------------------------------------------ */

pub struct Settings {
    cfg: RwLock<Config>,
    file: Option<PathBuf>,
}

impl Settings {
    pub fn new(cfg: Config, file: Option<PathBuf>) -> Settings {
        Settings { cfg: RwLock::new(cfg), file }
    }

    pub fn get(&self) -> Config {
        self.cfg.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn update(&self, f: impl FnOnce(&mut Config)) -> Config {
        let mut c = self.cfg.write().unwrap_or_else(|e| e.into_inner());
        f(&mut c);
        if let Some(file) = &self.file {
            if let Err(e) = c.save(file) {
                crate::log!("réglages : écriture impossible : {e}");
            }
        }
        c.clone()
    }
}

/* ------------------------------------------------------------------ */
/* État                                                                */
/* ------------------------------------------------------------------ */

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum State {
    #[default]
    Idle,
    Recording,
    Paused,
    Working,
    Done,
    Error,
}

/// Ce que la bulle affiche.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub state: State,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub words: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pasted: Option<bool>,
    pub live: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segments: Option<usize>,
    pub retry: bool,
    pub resumed: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub state: State,
    pub has_failed: bool,
    pub waiting_on_disk: usize,
}

enum Job {
    Transcribe(usize, Audio),
    SavePartial,
    Finish,
    Deliver(usize, JoinHandle<Option<(String, String)>>),
    FinishLive,
}

#[derive(Default)]
struct DState {
    raws: BTreeMap<usize, String>,
    parts: usize,
    duration_ms: u64,
    failed: Option<AiError>,
    failed_parts: Vec<usize>,
    live_text: String,
    live_context: String,
    segments: usize,
    finished: bool,
    cancelled: bool,
    recovered: bool,
}

/// Une dictée : un ou plusieurs morceaux d'audio. Un par pause (ou toutes les
/// quatre à six minutes) en mode classique, un par phrase en mode direct.
pub struct Dictation {
    pub id: String,
    pub live: bool,
    target: Mutex<Target>,
    st: Mutex<DState>,
    cancel: Mutex<CancellationToken>,
    jobs: Mutex<Option<mpsc::UnboundedSender<Job>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Dictation {
    fn new(id: String, live: bool, target: Target) -> Arc<Dictation> {
        Arc::new(Dictation {
            id,
            live,
            target: Mutex::new(target),
            st: Mutex::new(DState::default()),
            cancel: Mutex::new(CancellationToken::new()),
            jobs: Mutex::new(None),
        })
    }

    fn app(&self) -> String {
        lock(&self.target).name.clone()
    }

    fn token(&self) -> CancellationToken {
        lock(&self.cancel).clone()
    }

    fn raw_text(&self) -> String {
        lock(&self.st).raws.values().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" ").trim().to_string()
    }

    fn is_cancelled(&self) -> bool {
        lock(&self.st).cancelled
    }
}

pub fn new_id() -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let ms = crate::history::now_ms();
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let salt = radix36((std::process::id() as u64).wrapping_mul(2_654_435_761) ^ n.wrapping_mul(40_503));
    format!("d{}{}", radix36(ms), &salt[salt.len().saturating_sub(4)..])
}

fn radix36(mut n: u64) -> String {
    const D: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut s = vec![];
    loop {
        s.push(D[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

pub fn count_words(t: &str) -> usize {
    t.split_whitespace().count()
}

/// Phrases que Whisper invente sur un silence ou un souffle (héritées des
/// sous-titres qui ont servi à l'entraîner). Personne ne les dicte telles quelles.
pub fn is_hallucination(text: &str) -> bool {
    let t: String = text
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    const KNOWN: [&str; 9] = [
        "sous-titrage st 501",
        "soustitrage st 501",
        "sous-titres réalisés par la communauté damaraorg",
        "soustitres réalisés par la communauté damaraorg",
        "merci davoir regardé",
        "merci davoir regardé cette vidéo",
        "thank you for watching",
        "thanks for watching",
        "sous-titres par jérémy diaz",
    ];
    KNOWN.iter().any(|k| t == k.replace('-', "")) || KNOWN.contains(&t.as_str())
}

/// Message de la bulle après un échec. Une panne de réseau ne perd rien :
/// on le dit, puisque Souffle réessaiera tout seul.
fn failure_message(e: &AiError) -> String {
    match e {
        AiError::Network(_) => "Pas de réseau — audio gardé".into(),
        _ => ai::human_error(e),
    }
}

/* ------------------------------------------------------------------ */
/* Moteur                                                              */
/* ------------------------------------------------------------------ */

#[derive(Default)]
struct Hold {
    down: bool,
    pressed_at: Option<Instant>,
    started_here: bool,
}

struct Inner {
    state: State,
    starting: bool,
    dictation: Option<Arc<Dictation>>,
    failed: Option<Arc<Dictation>>,
    spoken_ms: u64,
    run_started: Option<Instant>,
    capture_started: bool,
    error_seq: u64,
    watchdog_gen: u64,
    max_gen: u64,
    hold: Hold,
}

pub struct Engine {
    inner: Mutex<Inner>,
    pub settings: Arc<Settings>,
    pub history: Mutex<History>,
    pub pending: Arc<Pending>,
    ai: Arc<dyn Ai>,
    desk: Arc<dyn Desktop>,
    rec: Arc<dyn Recorder>,
    rt: Handle,
    me: Weak<Engine>,
}

/// Délai sans le moindre signe de vie de la capture.
const CAPTURE_WATCHDOG_MS: u64 = 4500;
/// Toute requête est bornée dans le temps : passé ce délai après l'arrêt, une
/// dictée encore « en cours de transcription » est bloquée. On rend la main.
const WORKING_TIMEOUT_MS: u64 = 180_000;
/// Au-delà de cette durée d'audio, une transcription vide n'efface rien.
const EMPTY_KEEP_MS: u64 = 4000;
/// Après une panne de réseau : nouveaux essais automatiques, en secondes.
const AUTO_RETRY_S: [u64; 4] = [20, 60, 180, 600];
/// En mode « maintenir », un appui plus court est un simple appui : la dictée continue.
const HOLD_MIN_MS: u64 = 400;

impl Engine {
    pub fn new(
        settings: Arc<Settings>,
        history: History,
        pending: Arc<Pending>,
        ai: Arc<dyn Ai>,
        desk: Arc<dyn Desktop>,
        rec: Arc<dyn Recorder>,
        rt: Handle,
    ) -> Arc<Engine> {
        Arc::new_cyclic(|me| Engine {
            inner: Mutex::new(Inner {
                state: State::Idle,
                starting: false,
                dictation: None,
                failed: None,
                spoken_ms: 0,
                run_started: None,
                capture_started: false,
                error_seq: 0,
                watchdog_gen: 0,
                max_gen: 0,
                hold: Hold::default(),
            }),
            settings,
            history: Mutex::new(history),
            pending,
            ai,
            desk,
            rec,
            rt,
            me: me.clone(),
        })
    }

    fn arc(&self) -> Arc<Engine> {
        self.me.upgrade().expect("moteur vivant")
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        lock(&self.inner)
    }

    pub fn state(&self) -> State {
        self.inner().state
    }

    pub fn cfg(&self) -> Config {
        self.settings.get()
    }

    fn keeps_history(&self) -> bool {
        !self.cfg().privacy_mode
    }

    fn current(&self) -> Option<Arc<Dictation>> {
        self.inner().dictation.clone()
    }

    fn is_current(&self, d: &Arc<Dictation>) -> bool {
        self.inner().dictation.as_ref().is_some_and(|c| Arc::ptr_eq(c, d))
    }

    pub fn snapshot(&self) -> Snapshot {
        let (state, failed, current) = {
            let i = self.inner();
            (i.state, i.failed.as_ref().map(|d| d.id.clone()), i.dictation.as_ref().map(|d| d.id.clone()))
        };
        let waiting = self
            .pending
            .dictations_on_disk()
            .into_iter()
            .filter(|m| Some(&m.id) != failed.as_ref() && Some(&m.id) != current.as_ref())
            .count();
        Snapshot { state, has_failed: failed.is_some(), waiting_on_disk: waiting }
    }

    fn set_state(&self, next: State, mut view: View) {
        self.inner().state = next;
        view.state = next;
        self.desk.view(&view);
        self.desk.tray_changed();
    }

    fn beep(&self, b: Beep) {
        if self.cfg().play_sounds {
            self.desk.beep(b);
        }
    }

    fn after(&self, ms: u64, f: impl FnOnce(Arc<Engine>) + Send + 'static) {
        let me = self.arc();
        self.rt.spawn(async move {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            f(me);
        });
    }

    fn hide_later(&self, ms: u64) {
        self.after(ms, |e| {
            if e.state() == State::Idle {
                e.desk.hide_bubble();
            }
        });
    }

    /* ---------- Commandes ---------- */

    /// Raccourci (mode bascule), menu « Dicter maintenant », bouton des réglages.
    pub fn toggle(&self) {
        let state = self.state();
        crate::log!("raccourci reçu (état = {state:?})");
        if matches!(state, State::Recording | State::Paused) {
            self.stop();
        } else {
            self.start();
        }
    }

    pub fn shortcut_pressed(&self) {
        if self.cfg().mode != "hold" {
            self.toggle();
            return;
        }
        let state = {
            let mut i = self.inner();
            if i.hold.down {
                return; // répétition automatique de la touche
            }
            i.hold.down = true;
            i.hold.pressed_at = Some(Instant::now());
            i.hold.started_here = !matches!(i.state, State::Recording | State::Paused);
            i.state
        };
        if matches!(state, State::Recording | State::Paused) {
            self.stop();
        } else {
            self.start();
        }
    }

    pub fn shortcut_released(&self) {
        let held = {
            let mut i = self.inner();
            if !i.hold.down {
                return;
            }
            i.hold.down = false;
            let held = i.hold.started_here && i.hold.pressed_at.is_some_and(|t| t.elapsed() >= Duration::from_millis(HOLD_MIN_MS));
            i.hold.started_here = false;
            held
        };
        if held && self.cfg().mode == "hold" && self.state() == State::Recording {
            self.stop();
        }
    }

    pub fn escape_pressed(&self) {
        let action = self.cfg().escape_action;
        crate::log!("Échap pendant la dictée → {action}");
        match action.as_str() {
            "stop" => self.stop(),
            "cancel" => self.cancel(),
            _ => {}
        }
    }

    pub fn start(&self) {
        {
            let mut i = self.inner();
            if !matches!(i.state, State::Idle | State::Done | State::Error) || i.starting {
                crate::log!("démarrage ignoré (état = {:?})", i.state);
                return;
            }
            i.starting = true;
        }
        let me = self.arc();
        self.rt.spawn(async move {
            me.start_async().await;
            me.inner().starting = false;
        });
    }

    async fn start_async(&self) {
        match self.desk.mic_permission() {
            MicPermission::Denied => {
                crate::log!("micro : autorisation refusée");
                self.desk.open_mic_settings();
                self.fail("Micro non autorisé", 4000, false);
                return;
            }
            MicPermission::Unknown => {
                let ok = self.desk.request_mic().await;
                crate::log!("demande d'accès au micro → {ok}");
                if !ok {
                    self.fail("Micro refusé", 2500, false);
                    return;
                }
            }
            MicPermission::Granted => {}
        }

        let cfg = self.cfg();
        // Identifier l'application au premier plan avant que quoi que ce soit
        // ne s'affiche : c'est elle qui recevra le texte.
        let target = self.desk.frontmost();
        crate::log!("application cible = {}", if target.name.is_empty() { "(inconnue)" } else { &target.name });
        let live = cfg.live_mode;
        let d = Dictation::new(new_id(), live, target);
        self.spawn_worker(&d);
        {
            let mut i = self.inner();
            i.dictation = Some(d.clone());
            i.spoken_ms = 0;
            i.run_started = Some(Instant::now());
            i.capture_started = false;
            i.error_seq += 1; // une erreur précédente ne doit plus rien masquer
        }
        self.set_state(State::Recording, View { live, ..Default::default() });
        self.desk.set_interactive(true);
        self.desk.show_bubble();
        self.beep(Beep::Start);
        crate::log!("capture demandée{} — dictée {}", if live { " (mode direct)" } else { "" }, d.id);
        self.rec.send(Cmd::Start { id: d.id.clone(), live, persist: !cfg.privacy_mode, microphone: cfg.microphone.clone() });
        let ai = self.ai.clone();
        let warm = cfg.clone();
        self.rt.spawn(async move { ai.prewarm(&warm).await });
        if cfg.escape_action != "off" {
            self.desk.escape_watch(true);
        }
        self.arm_watchdog();
        self.arm_max_duration();
    }

    fn arm_watchdog(&self) {
        let gen = {
            let mut i = self.inner();
            i.watchdog_gen += 1;
            i.watchdog_gen
        };
        self.after(CAPTURE_WATCHDOG_MS, move |e| {
            let fire = {
                let i = e.inner();
                i.watchdog_gen == gen && !i.capture_started && i.state == State::Recording
            };
            if fire {
                crate::log!("ERREUR : la capture micro n'a jamais démarré");
                e.rec.send(Cmd::Cancel);
                e.fail("Micro injoignable", 3000, false);
            }
        });
    }

    /// La limite ne compte que le temps de parole : une longue pause ne termine pas la dictée.
    fn arm_max_duration(&self) {
        let (gen, remaining, total) = {
            let mut i = self.inner();
            i.max_gen += 1;
            let total = self.cfg().max_recording_ms();
            (i.max_gen, total.saturating_sub(i.spoken_ms).max(1000), total)
        };
        self.after(remaining, move |e| {
            let fire = {
                let i = e.inner();
                i.max_gen == gen && i.state == State::Recording
            };
            if fire {
                crate::log!("durée maximale atteinte ({} min), arrêt automatique", total / 60_000);
                e.stop();
            }
        });
    }

    fn clear_timers(&self) {
        let mut i = self.inner();
        i.watchdog_gen += 1;
        i.max_gen += 1;
    }

    fn bank_spoken_time(&self) {
        let mut i = self.inner();
        if let Some(t) = i.run_started.take() {
            i.spoken_ms += t.elapsed().as_millis() as u64;
        }
    }

    pub fn stop(&self) {
        if !matches!(self.state(), State::Recording | State::Paused) {
            crate::log!("arrêt ignoré (état = {:?})", self.state());
            return;
        }
        self.bank_spoken_time();
        crate::log!("arrêt demandé");
        self.clear_timers();
        self.desk.set_interactive(false);
        self.desk.escape_watch(false);
        let live = self.current().is_some_and(|d| d.live);
        self.set_state(State::Working, View { live, ..Default::default() });
        self.rec.send(Cmd::Stop);
        if let Some(d) = self.current() {
            self.arm_working_watchdog(&d);
        }
    }

    fn arm_working_watchdog(&self, d: &Arc<Dictation>) {
        let d = d.clone();
        self.after(WORKING_TIMEOUT_MS, move |e| {
            if e.state() != State::Working || !e.is_current(&d) {
                return;
            }
            crate::log!("ERREUR : dictée {} bloquée depuis {} s, abandon (audio gardé)", d.id, WORKING_TIMEOUT_MS / 1000);
            d.token().cancel();
            e.inner().failed = Some(d.clone());
            e.desk.tray_changed();
            e.fail("Délai dépassé", 7000, true);
        });
    }

    pub fn cancel(&self) {
        if !matches!(self.state(), State::Recording | State::Paused) {
            return;
        }
        crate::log!("annulation");
        self.clear_timers();
        self.desk.set_interactive(false);
        self.desk.escape_watch(false);
        self.inner().run_started = None;
        self.rec.send(Cmd::Cancel);
        if let Some(d) = self.current() {
            lock(&d.st).cancelled = true;
            d.token().cancel();
            *lock(&d.jobs) = None;
            // Annuler est un choix : l'audio est oublié. Un texte déjà versé dans
            // l'historique par une pause, lui, y reste.
            self.pending.forget(&d.id);
        }
        self.set_state(State::Idle, View::default());
        self.hide_later(150);
    }

    pub fn toggle_pause(&self) {
        match self.state() {
            State::Recording => self.pause(),
            State::Paused => self.resume(),
            _ => {}
        }
    }

    /// La pause suspend l'enregistrement sans terminer la dictée. Ce qui a été
    /// dit jusque-là est transcrit aussitôt et versé dans l'historique.
    pub fn pause(&self) {
        if self.state() != State::Recording {
            return;
        }
        self.bank_spoken_time();
        self.inner().max_gen += 1;
        self.rec.send(Cmd::Pause);
        let live = self.current().is_some_and(|d| d.live);
        self.set_state(State::Paused, View { live, ..Default::default() });
        crate::log!("pause");
    }

    pub fn resume(&self) {
        if self.state() != State::Paused {
            return;
        }
        {
            let mut i = self.inner();
            i.run_started = Some(Instant::now());
            i.capture_started = false;
        }
        self.arm_max_duration();
        self.arm_watchdog();
        self.rec.send(Cmd::Resume);
        let live = self.current().is_some_and(|d| d.live);
        self.set_state(State::Recording, View { live, resumed: true, ..Default::default() });
        crate::log!("reprise");
    }

    /// Clic sur la bulle : pause pendant la dictée, nouvel essai après un échec.
    /// Le focus revient aussitôt à l'application où l'on écrit.
    pub fn bubble_clicked(&self) {
        match self.state() {
            State::Recording | State::Paused => self.toggle_pause(),
            State::Error => self.retry_from_bubble(),
            _ => {}
        }
        if let Some(d) = self.current() {
            let t = lock(&d.target).clone();
            self.desk.restore_focus(&t);
        }
    }

    pub fn bubble_right_clicked(&self) {
        self.cancel();
    }

    /// Fin d'une dictée en échec : message dans la bulle, puis retour au repos.
    fn fail(&self, message: &str, ms: u64, retry: bool) {
        self.desk.escape_watch(false);
        self.clear_timers();
        self.inner().run_started = None;
        // Une erreur rattrapable laisse la bulle cliquable : un clic relance la dictée.
        self.desk.set_interactive(retry);
        self.set_state(
            State::Error,
            View { message: Some(ai::truncate_chars(message, 60)), retry, ..Default::default() },
        );
        self.desk.show_bubble();
        self.beep(Beep::Error);
        // Chaque erreur porte son numéro : la minuterie d'une ancienne erreur ne
        // remet jamais l'application au repos en pleine dictée suivante.
        let seq = {
            let mut i = self.inner();
            i.error_seq += 1;
            i.error_seq
        };
        self.after(ms, move |e| {
            let stale = {
                let i = e.inner();
                i.error_seq != seq || i.state != State::Error
            };
            if stale {
                return;
            }
            e.desk.set_interactive(false);
            e.set_state(State::Idle, View::default());
            e.desk.hide_bubble();
        });
    }

    fn show_done(&self, words: usize, pasted: bool) {
        self.set_state(State::Done, View { words: Some(words), pasted: Some(pasted), ..Default::default() });
        self.beep(Beep::Done);
        self.after(1100, |e| {
            if e.state() == State::Done {
                e.set_state(State::Idle, View::default());
                e.desk.hide_bubble();
            }
        });
    }

    /* ---------- Événements de la capture ---------- */

    pub fn on_capture(&self, ev: Event) {
        match ev {
            Event::Acquiring => {
                if self.state() == State::Recording {
                    self.arm_watchdog();
                }
            }
            Event::Started { device } => {
                let mut i = self.inner();
                i.capture_started = true;
                i.watchdog_gen += 1;
                drop(i);
                crate::log!("capture active ({device})");
            }
            Event::Level(l) => {
                if self.state() == State::Recording {
                    self.desk.level(l);
                }
            }
            Event::Error(msg) => {
                crate::log!("ERREUR capture : {msg}");
                match self.state() {
                    State::Recording | State::Paused => self.fail(&msg, 3500, false),
                    // Arrêt déjà demandé : la capture a lâché avant le dernier
                    // morceau. On termine avec ce qui est déjà arrivé.
                    State::Working => {
                        if let Some(d) = self.current() {
                            self.push_job(&d, if d.live { Job::FinishLive } else { Job::Finish });
                        }
                    }
                    _ => {}
                }
            }
            Event::Part(p) => self.on_part(p),
        }
    }

    fn on_part(&self, p: Part) {
        let Some(d) = self.current().filter(|d| d.id == p.id) else {
            crate::log!("audio reçu hors dictée, ignoré");
            return;
        };
        if d.is_cancelled() {
            return;
        }
        let kind = if d.live { "phrase" } else if p.pause_part { "morceau avant pause" } else if p.last { "dictée" } else { "morceau" };
        let index = p.audio.as_ref().map(|a| {
            crate::log!("audio reçu ({kind}) : {} octets, {} ms{}", a.bytes.len(), a.duration_ms, if p.last { ", dernier" } else { "" });
            let mut st = lock(&d.st);
            st.parts += 1;
            st.duration_ms += a.duration_ms;
            drop(st);
            self.pending.save(&d.id, p.seq, a.clone(), d.live, &d.app(), self.keeps_history());
            p.seq
        });
        if index.is_none() {
            crate::log!("audio reçu ({kind}) : rien d'audible{}", if p.last { ", dernier" } else { "" });
            // Le brouillon d'un morceau silencieux ne sert à rien.
            self.pending.forget_part(&d.id, p.seq);
        }
        if d.live {
            if let (Some(i), Some(audio)) = (index, p.audio) {
                let me = self.arc();
                let dd = d.clone();
                let handle = self.rt.spawn(async move { me.prepare_segment(&dd, i, audio).await });
                self.push_job(&d, Job::Deliver(i, handle));
            }
            if p.last {
                self.push_job(&d, Job::FinishLive);
            }
        } else {
            if let (Some(i), Some(audio)) = (index, p.audio) {
                self.push_job(&d, Job::Transcribe(i, audio));
            }
            if p.pause_part {
                self.push_job(&d, Job::SavePartial);
            } else if p.last {
                self.push_job(&d, Job::Finish);
            }
        }
    }

    fn push_job(&self, d: &Arc<Dictation>, job: Job) {
        if let Some(tx) = lock(&d.jobs).as_ref() {
            let _ = tx.send(job);
        }
    }

    /// Les morceaux d'une dictée sont traités dans l'ordre où ils ont été dits.
    fn spawn_worker(&self, d: &Arc<Dictation>) {
        let (tx, mut rx) = mpsc::unbounded_channel::<Job>();
        *lock(&d.jobs) = Some(tx);
        let me = self.arc();
        let d = d.clone();
        self.rt.spawn(async move {
            while let Some(job) = rx.recv().await {
                match job {
                    Job::Transcribe(i, audio) => me.transcribe_part(&d, i, &audio).await,
                    Job::SavePartial => me.save_partial(&d),
                    Job::Finish => {
                        me.finish_classic(&d, true, false).await;
                        break;
                    }
                    Job::Deliver(i, handle) => {
                        if let Ok(Some((raw, text))) = handle.await {
                            me.deliver_segment(&d, i, raw, text).await;
                        }
                    }
                    Job::FinishLive => {
                        me.finish_live(&d);
                        break;
                    }
                }
            }
            *lock(&d.jobs) = None;
        });
    }

    /* ---------- Mode classique ---------- */

    async fn transcribe_part(&self, d: &Arc<Dictation>, index: usize, audio: &Audio) {
        let context = {
            let st = lock(&d.st);
            if st.failed.is_some() || st.cancelled {
                return;
            }
            // Le texte des morceaux précédents sert d'amorce : la transcription
            // garde le fil d'une pause à l'autre.
            ai::tail_chars(&st.raws.values().cloned().collect::<Vec<_>>().join(" "), 600).to_string()
        };
        let cfg = self.cfg();
        let t0 = Instant::now();
        let token = d.token();
        let res = tokio::select! {
            _ = token.cancelled() => { lock(&d.st).cancelled = true; return; }
            r = self.ai.transcribe(&cfg, audio, &context) => r,
        };
        match res {
            Ok(raw) => {
                let raw = if is_hallucination(&raw) { String::new() } else { raw };
                crate::log!("transcription morceau {} ({} ms) : {:?}", index + 1, t0.elapsed().as_millis(), raw);
                lock(&d.st).raws.insert(index, raw);
            }
            Err(e) => {
                crate::log!("ERREUR transcription : {e}");
                lock(&d.st).failed = Some(e);
            }
        }
    }

    /// Pause : ce qui a été dit jusque-là entre tout de suite dans l'historique.
    fn save_partial(&self, d: &Arc<Dictation>) {
        {
            let st = lock(&d.st);
            if st.failed.is_some() || st.cancelled {
                return;
            }
        }
        if !self.keeps_history() {
            return;
        }
        let raw = d.raw_text();
        if raw.is_empty() {
            return;
        }
        let limit = self.cfg().history_limit;
        lock(&self.history).upsert(
            &d.id,
            Entry { raw: raw.clone(), words: count_words(&raw), text: raw, app: d.app(), partial: true, ..Default::default() },
            limit,
        );
        crate::log!("pause : texte provisoire versé dans l'historique");
    }

    /// `quiet` : réessai en arrière-plan, la bulle n'est pas touchée.
    async fn finish_classic(&self, d: &Arc<Dictation>, paste: bool, quiet: bool) {
        {
            let mut st = lock(&d.st);
            // Deux annonces de fin (arrêt pendant la fermeture d'un morceau) :
            // la dictée ne doit être collée qu'une fois.
            if st.cancelled || st.finished {
                return;
            }
            if st.failed.is_some() {
                drop(st);
                return self.on_dictation_failed(d, quiet);
            }
            st.finished = true;
        }
        let raw = d.raw_text();
        let spoken_ms = lock(&d.st).duration_ms;
        if raw.is_empty() && spoken_ms >= EMPTY_KEEP_MS && !quiet {
            // Plusieurs secondes d'audio et pas un mot : plutôt qu'effacer, on
            // garde l'audio et on propose de réessayer (bulle, ou menu plus tard).
            crate::log!("transcription vide pour {} s d'audio : audio gardé", spoken_ms / 1000);
            {
                let mut st = lock(&d.st);
                st.finished = false;
                st.failed = Some(AiError::Other("Rien compris — réessayer".into()));
            }
            return self.on_dictation_failed(d, quiet);
        }
        if raw.is_empty() {
            self.pending.forget(&d.id);
            // Une erreur de micro déjà affichée dit mieux ce qui s'est passé.
            if self.is_current(d) && !quiet && self.state() != State::Error {
                self.fail("Rien entendu", 1800, false);
            }
            return;
        }
        if self.is_current(d) && !quiet {
            self.set_state(State::Working, View { step: Some("mise en forme".into()), ..Default::default() });
        }
        let cfg = self.cfg();
        let t1 = Instant::now();
        let token = d.token();
        // Une seule remise en forme pour toute la dictée : une correction dite
        // après la pause (« oublie ce que j'ai dit avant ») s'applique aussi à ce qui précède.
        let app = d.app();
        let text = tokio::select! {
            _ = token.cancelled() => return,
            t = self.ai.format(&cfg, &raw, &app, "") => t,
        };
        if d.is_cancelled() {
            return;
        }
        crate::log!("mise en forme ({} ms) : {:?}", t1.elapsed().as_millis(), text);
        self.deliver_dictation(d, raw, text, paste, quiet).await;
    }

    /// Le texte est prêt : historique d'abord, collage ensuite, audio oublié en dernier.
    async fn deliver_dictation(&self, d: &Arc<Dictation>, raw: String, text: String, paste: bool, quiet: bool) {
        let cfg = self.cfg();
        let words = count_words(&text);
        let (recovered, duration_ms) = {
            let st = lock(&d.st);
            (st.recovered, st.duration_ms)
        };
        if !cfg.privacy_mode {
            // Les phrases rattrapées d'une dictée directe ont leur propre entrée :
            // celle de la dictée contient déjà les phrases collées.
            let id = if d.live { format!("{}-rattrapage", d.id) } else { d.id.clone() };
            lock(&self.history).upsert(
                &id,
                Entry { raw, text: text.clone(), app: d.app(), words, recovered: recovered || d.live, ..Default::default() },
                cfg.history_limit,
            );
        }
        self.settings.update(|c| {
            c.stats.words += words as u64;
            c.stats.seconds += duration_ms / 1000;
            c.stats.sessions += 1;
        });
        self.clear_failed(d);

        if !paste {
            // Dictée reprise plus tard : rien ne dit que le bon champ a encore
            // le focus. Le texte va dans l'historique et le presse-papier.
            self.desk.copy(&text);
            self.pending.forget(&d.id);
            crate::log!("dictée {} retranscrite ({words} mots), copiée dans le presse-papier", d.id);
            if !quiet && self.is_current(d) {
                self.show_done(words, false);
            }
            self.desk.tray_changed();
            return;
        }

        let opts = PasteOpts {
            auto_paste: cfg.auto_paste,
            refocus: cfg.refocus_target,
            restore_clipboard: cfg.restore_clipboard,
            target: lock(&d.target).clone(),
        };
        let outcome = self.desk.paste(text, opts).await;
        crate::log!("insertion : {outcome:?}");
        self.pending.forget(&d.id);
        self.desk.tray_changed();
        // Une dictée suivante a pu commencer entre-temps (après une erreur de
        // micro) : la bulle est à elle, on ne la touche pas.
        if !self.is_current(d) {
            return;
        }
        match outcome {
            PasteOutcome::Pasted => self.show_done(words, true),
            PasteOutcome::ClipboardOnly => self.show_done(words, false),
            PasteOutcome::NeedsAccessibility => {
                self.desk.open_accessibility_settings();
                self.fail("Copié — autorisez l'Accessibilité", 2600, false);
            }
            PasteOutcome::KeystrokeFailed => self.fail("Copié — collez manuellement", 2400, false),
        }
    }

    fn clear_failed(&self, d: &Arc<Dictation>) {
        let mut i = self.inner();
        if i.failed.as_ref().is_some_and(|f| Arc::ptr_eq(f, d)) {
            i.failed = None;
        }
    }

    /// Échec : l'audio reste dans la file, la bulle propose de réessayer.
    fn on_dictation_failed(&self, d: &Arc<Dictation>, quiet: bool) {
        let err = lock(&d.st).failed.clone();
        let show = {
            let mut i = self.inner();
            i.failed = Some(d.clone());
            !quiet && (i.dictation.as_ref().is_some_and(|c| Arc::ptr_eq(c, d)) || i.state == State::Working)
        };
        self.desk.tray_changed();
        if show {
            if let Some(e) = &err {
                self.fail(&failure_message(e), 7000, true);
            }
        }
        if matches!(err, Some(AiError::Network(_))) {
            self.retry_in_background(d);
        }
    }

    /// Panne de réseau : l'audio est gardé et Souffle réessaie tout seul,
    /// de plus en plus espacé. Le texte retrouvé va dans l'historique et le
    /// presse-papier, et la bulle le signale.
    fn retry_in_background(&self, d: &Arc<Dictation>) {
        let me = self.arc();
        let d = d.clone();
        self.rt.spawn(async move {
            for secs in AUTO_RETRY_S {
                tokio::time::sleep(Duration::from_secs(secs)).await;
                // Réessayée entre-temps (bulle, menu) ou annulée : plus rien à faire.
                if !me.inner().failed.as_ref().is_some_and(|f| Arc::ptr_eq(f, &d)) {
                    return;
                }
                // On ne dérange pas une dictée en cours : rendez-vous au suivant.
                if !matches!(me.state(), State::Idle | State::Done | State::Error) {
                    continue;
                }
                crate::log!("nouvel essai automatique de {}", d.id);
                if me.retry_dictation(d.clone(), false).await {
                    return;
                }
            }
            crate::log!("dictée {} toujours en attente : menu « Réessayer » ou prochain démarrage", d.id);
        });
    }

    /// Relance une dictée en échec à partir de son audio gardé.
    /// `paste` : depuis la bulle, juste après l'échec, on colle comme d'habitude ;
    /// depuis le menu, plus tard, on se limite à l'historique et au presse-papier.
    pub async fn retry_dictation(&self, d: Arc<Dictation>, paste: bool) -> bool {
        let parts = self.pending.parts(&d.id);
        if parts.is_empty() {
            crate::log!("réessai impossible : audio de {} introuvable", d.id);
            self.clear_failed(&d);
            self.desk.tray_changed();
            return false;
        }
        crate::log!("réessai de {} ({} morceau(x))", d.id, parts.len());
        let recovered = {
            let mut st = lock(&d.st);
            let recovered = st.recovered;
            *st = DState { recovered, duration_ms: parts.iter().map(|(_, a)| a.duration_ms).sum(), ..Default::default() };
            recovered
        };
        *lock(&d.cancel) = CancellationToken::new();
        // Une dictée en cours garde la bulle : le réessai se fait alors en silence.
        let quiet = recovered || !matches!(self.state(), State::Idle | State::Done | State::Error);
        if !quiet {
            self.inner().dictation = Some(d.clone());
            self.desk.set_interactive(false);
            self.set_state(State::Working, View { step: Some("nouvel essai".into()), ..Default::default() });
            self.desk.show_bubble();
        }
        for (i, audio) in &parts {
            self.transcribe_part(&d, *i, audio).await;
        }
        if let Some(e) = lock(&d.st).failed.clone() {
            self.inner().failed = Some(d.clone());
            self.desk.tray_changed();
            if !quiet {
                self.fail(&failure_message(&e), 7000, true);
            }
            return false;
        }
        self.finish_classic(&d, paste, quiet).await;
        true
    }

    pub fn retry_from_bubble(&self) {
        let failed = {
            let i = self.inner();
            if i.state != State::Error {
                return;
            }
            i.failed.clone()
        };
        let Some(d) = failed else { return };
        // Les phrases ratées d'une dictée directe ne peuvent plus être collées à leur place.
        let paste = !d.live;
        let me = self.arc();
        self.rt.spawn(async move {
            me.retry_dictation(d, paste).await;
        });
    }

    /// Menu « Réessayer la dernière dictée ».
    pub fn retry_last(&self) {
        let Some(d) = self.inner().failed.clone() else { return };
        let me = self.arc();
        self.rt.spawn(async move {
            me.retry_dictation(d, false).await;
        });
    }

    /// Au démarrage, et depuis le menu : les dictées restées sur le disque
    /// (plantage, ordinateur éteint pendant une transcription) sont
    /// retranscrites dans l'historique et le presse-papier.
    pub async fn recover_pending(&self) {
        let salvaged = self.pending.salvage_streams();
        if salvaged > 0 {
            crate::log!("{salvaged} morceau(x) interrompu(s) par un plantage récupéré(s)");
        }
        let skip: Vec<String> = {
            let i = self.inner();
            [i.dictation.as_ref(), i.failed.as_ref()].into_iter().flatten().map(|d| d.id.clone()).collect()
        };
        let leftovers: Vec<_> = self.pending.dictations_on_disk().into_iter().filter(|m| !skip.contains(&m.id)).collect();
        if leftovers.is_empty() {
            return;
        }
        crate::log!("{} dictée(s) en attente sur le disque : reprise", leftovers.len());
        for m in leftovers {
            let d = Dictation::new(m.id.clone(), false, Target { name: m.app.clone(), ..Default::default() });
            lock(&d.st).recovered = true;
            if !self.retry_dictation(d, false).await {
                crate::log!("dictée {} toujours en attente (réseau ou fournisseur indisponible)", m.id);
            }
        }
        self.desk.tray_changed();
    }

    pub fn recover_pending_in_background(&self) {
        let me = self.arc();
        self.rt.spawn(async move { me.recover_pending().await });
    }

    /* ---------- Mode direct ---------- */

    /// Étape parallélisable : audio -> texte prêt à écrire. Les phrases partent
    /// en parallèle ; leur place dans la file d'écriture est réservée dès
    /// l'arrivée de l'audio, avant tout appel distant.
    async fn prepare_segment(&self, d: &Arc<Dictation>, index: usize, audio: Audio) -> Option<(String, String)> {
        let cfg = self.cfg();
        let context = lock(&d.st).live_context.clone();
        let token = d.token();
        let t0 = Instant::now();
        let res = tokio::select! {
            _ = token.cancelled() => return None,
            r = self.ai.transcribe(&cfg, &audio, &context) => r,
        };
        let raw = match res {
            Ok(r) => r,
            Err(e) => {
                crate::log!("ERREUR phrase : {e}");
                let mut st = lock(&d.st);
                if st.failed.is_none() {
                    st.failed = Some(e);
                }
                st.failed_parts.push(index);
                return None;
            }
        };
        crate::log!("transcription phrase {} ({} ms) : {:?}", index + 1, t0.elapsed().as_millis(), raw);
        if raw.is_empty() || is_hallucination(&raw) {
            self.pending.forget_part(&d.id, index); // un blanc n'a rien à garder
            return None;
        }
        {
            let mut st = lock(&d.st);
            let joined = format!("{} {}", st.live_context, raw);
            st.live_context = ai::tail_chars(joined.trim(), 600).to_string();
        }
        let app = d.app();
        let text = tokio::select! {
            _ = token.cancelled() => return None,
            t = self.ai.format(&cfg, &raw, &app, &context) => t,
        };
        Some((raw, text))
    }

    /// Étape strictement ordonnée : écriture de la phrase dans l'application cible.
    async fn deliver_segment(&self, d: &Arc<Dictation>, index: usize, raw: String, text: String) {
        if d.is_cancelled() {
            return;
        }
        let cfg = self.cfg();
        let (piece, entry) = {
            let mut st = lock(&d.st);
            // En continu, chaque phrase s'ajoute à la précédente : il faut l'espace.
            let piece = format!("{}{}", if st.segments > 0 { " " } else { "" }, text);
            st.live_text.push_str(&piece);
            st.raws.insert(index, raw);
            let raw_all = st.raws.values().cloned().collect::<Vec<_>>().join(" ");
            let text_all = st.live_text.trim().to_string();
            (piece, Entry { raw: raw_all, words: count_words(&text_all), text: text_all, app: d.app(), ..Default::default() })
        };
        // Une seule entrée d'historique par dictée, complétée phrase après phrase.
        if !cfg.privacy_mode {
            lock(&self.history).upsert(&d.id, entry, cfg.history_limit);
        }
        let opts = PasteOpts {
            auto_paste: cfg.auto_paste,
            refocus: cfg.refocus_target,
            restore_clipboard: cfg.restore_clipboard,
            target: lock(&d.target).clone(),
        };
        let outcome = self.desk.paste(piece, opts).await;
        crate::log!("insertion : {outcome:?}");
        self.pending.forget_part(&d.id, index);
        let segments = {
            let mut st = lock(&d.st);
            st.segments += 1;
            st.segments
        };
        if self.state() == State::Recording && self.is_current(d) {
            self.set_state(State::Recording, View { live: true, segments: Some(segments), ..Default::default() });
        }
    }

    fn finish_live(&self, d: &Arc<Dictation>) {
        let (words, text_empty, failed_parts, duration_ms) = {
            let mut st = lock(&d.st);
            if st.finished {
                return;
            }
            st.finished = true;
            (count_words(&st.live_text), st.live_text.trim().is_empty(), st.failed_parts.len(), st.duration_ms)
        };
        self.settings.update(|c| {
            c.stats.words += words as u64;
            c.stats.seconds += duration_ms / 1000;
            c.stats.sessions += 1;
        });
        if d.is_cancelled() {
            return;
        }
        if failed_parts > 0 {
            // Les phrases ratées ne peuvent plus être collées à leur place : elles
            // seront retranscrites dans l'historique et le presse-papier.
            self.inner().failed = Some(d.clone());
            self.desk.tray_changed();
            if self.is_current(d) {
                let s = if failed_parts > 1 { "s" } else { "" };
                self.fail(&format!("{failed_parts} phrase{s} non transcrite{s}"), 7000, true);
            }
            if matches!(lock(&d.st).failed, Some(AiError::Network(_))) {
                self.retry_in_background(d);
            }
            return;
        }
        self.pending.forget(&d.id);
        // Une dictée suivante a peut-être déjà commencé : la bulle est à elle.
        if !self.is_current(d) {
            return;
        }
        if text_empty {
            // Une erreur de micro déjà affichée dit mieux ce qui s'est passé.
            if self.state() != State::Error {
                self.fail("Rien entendu", 1800, false);
            }
            return;
        }
        self.show_done(words, true);
    }

    /// Le menu « Replacer la bulle au centre » et le déplacement passent par ici.
    pub fn desk(&self) -> &Arc<dyn Desktop> {
        &self.desk
    }
}

#[cfg(test)]
mod tests;
