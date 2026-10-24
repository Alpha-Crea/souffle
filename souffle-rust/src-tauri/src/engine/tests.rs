//! Scénarios de bout en bout du moteur, sans micro ni fenêtre.
//! L'audio de test porte son texte : « TXT:bonjour », « ERR:403 », « LENT:bonjour ».

use super::*;
use crate::audio::capture::Part;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Default)]
struct MockAi {
    broken: AtomicBool,
    offline: AtomicBool,
    formats: AtomicUsize,
}

#[async_trait]
impl Ai for MockAi {
    async fn transcribe(&self, _cfg: &Config, audio: &Audio, _ctx: &str) -> Result<String, AiError> {
        let s = String::from_utf8_lossy(&audio.bytes).to_string();
        if self.broken.load(Ordering::SeqCst) || s.starts_with("ERR:") {
            return Err(AiError::Http {
                status: 403,
                body: r#"{"error":{"message":"Access denied. Please check your network settings."}}"#.into(),
            });
        }
        if self.offline.load(Ordering::SeqCst) {
            return Err(AiError::Network("connexion réinitialisée".into()));
        }
        if let Some(t) = s.strip_prefix("LENT:") {
            tokio::time::sleep(Duration::from_millis(80)).await;
            return Ok(t.into());
        }
        Ok(s.strip_prefix("TXT:").unwrap_or(&s).to_string())
    }

    async fn format(&self, _cfg: &Config, text: &str, _app: &str, _ctx: &str) -> String {
        self.formats.fetch_add(1, Ordering::SeqCst);
        format!("[{text}]")
    }
}

#[derive(Default)]
struct MockDesk {
    views: Mutex<Vec<View>>,
    pastes: Mutex<Vec<String>>,
    copies: Mutex<Vec<String>>,
    interactive: Mutex<Vec<bool>>,
    mic: Mutex<Option<MicPermission>>,
    outcome: Mutex<Option<PasteOutcome>>,
    accessibility_opened: AtomicUsize,
    escape: Mutex<Vec<bool>>,
}

#[async_trait]
impl Desktop for MockDesk {
    fn view(&self, view: &View) {
        lock(&self.views).push(view.clone());
    }
    fn level(&self, _: f32) {}
    fn show_bubble(&self) {}
    fn hide_bubble(&self) {}
    fn set_interactive(&self, on: bool) {
        lock(&self.interactive).push(on);
    }
    fn tray_changed(&self) {}
    fn frontmost(&self) -> Target {
        Target { name: "Mail".into(), pid: 42, window: 0 }
    }
    async fn paste(&self, text: String, _opts: PasteOpts) -> PasteOutcome {
        let outcome = lock(&self.outcome).clone().unwrap_or(PasteOutcome::Pasted);
        if outcome == PasteOutcome::Pasted {
            lock(&self.pastes).push(text.clone());
        }
        lock(&self.copies).push(text);
        outcome
    }
    fn copy(&self, text: &str) {
        lock(&self.copies).push(text.into());
    }
    fn beep(&self, _: Beep) {}
    fn mic_permission(&self) -> MicPermission {
        lock(&self.mic).unwrap_or(MicPermission::Granted)
    }
    async fn request_mic(&self) -> bool {
        true
    }
    fn open_mic_settings(&self) {}
    fn open_accessibility_settings(&self) {
        self.accessibility_opened.fetch_add(1, Ordering::SeqCst);
    }
    fn escape_watch(&self, on: bool) {
        lock(&self.escape).push(on);
    }
    fn restore_focus(&self, _: &Target) {}
}

#[derive(Default)]
struct MockRec {
    cmds: Mutex<Vec<Cmd>>,
}

impl Recorder for MockRec {
    fn send(&self, cmd: Cmd) {
        lock(&self.cmds).push(cmd);
    }
}

struct Bench {
    engine: Arc<Engine>,
    ai: Arc<MockAi>,
    desk: Arc<MockDesk>,
    rec: Arc<MockRec>,
    dir: PathBuf,
}

impl Drop for Bench {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn tmpdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("souffle-engine-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn bench_in(dir: PathBuf, cfg: Config) -> Bench {
    let ai = Arc::new(MockAi::default());
    let desk = Arc::new(MockDesk::default());
    let rec = Arc::new(MockRec::default());
    let engine = Engine::new(
        Arc::new(Settings::new(cfg, None)),
        History::open(&dir),
        Arc::new(Pending::new(dir.join("en-attente"))),
        ai.clone(),
        desk.clone(),
        rec.clone(),
        Handle::current(),
    );
    Bench { engine, ai, desk, rec, dir }
}

fn bench(name: &str, f: impl FnOnce(&mut Config)) -> Bench {
    let mut cfg = Config::default();
    f(&mut cfg);
    bench_in(tmpdir(name), cfg)
}

/// Laisse tourner les tâches (le temps est simulé).
async fn settle() {
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

impl Bench {
    async fn start(&self) -> String {
        self.engine.start();
        settle().await;
        self.engine.on_capture(Event::Started { device: "test".into() });
        self.id()
    }

    fn id(&self) -> String {
        self.engine.current().expect("dictée en cours").id.clone()
    }

    fn part(&self, seq: usize, text: Option<&str>, pause_part: bool, last: bool) {
        let live = self.engine.current().is_some_and(|d| d.live);
        self.engine.on_capture(Event::Part(Part {
            id: self.id(),
            seq,
            audio: text.map(|t| Audio { bytes: t.as_bytes().to_vec(), mime: "audio/wav".into(), duration_ms: 2000 }),
            live,
            pause_part,
            last,
        }));
    }

    fn last_view(&self) -> View {
        lock(&self.desk.views).last().cloned().unwrap_or_default()
    }

    fn pastes(&self) -> Vec<String> {
        lock(&self.desk.pastes).clone()
    }

    fn copies(&self) -> Vec<String> {
        lock(&self.desk.copies).clone()
    }

    fn history(&self) -> Vec<Entry> {
        lock(&self.engine.history).list().to_vec()
    }

    fn files(&self) -> usize {
        std::fs::read_dir(self.dir.join("en-attente")).map(|r| r.count()).unwrap_or(0)
    }

    fn cmds(&self) -> Vec<String> {
        lock(&self.rec.cmds).iter().map(|c| format!("{c:?}").split([' ', '{']).next().unwrap_or("").to_string()).collect()
    }
}

#[tokio::test(start_paused = true)]
async fn simple_dictation() {
    let b = bench("simple", |_| {});
    let id = b.start().await;
    assert_eq!(b.engine.state(), State::Recording);
    assert!(matches!(&lock(&b.rec.cmds)[0], Cmd::Start { persist: true, live: false, .. }));
    b.part(0, Some("TXT:bonjour le monde"), false, true);
    settle().await;
    assert_eq!(b.pastes(), vec!["[bonjour le monde]"]);
    let h = b.history();
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].id, id);
    assert_eq!(h[0].app, "Mail");
    assert!(!h[0].partial);
    assert_eq!(b.files(), 0, "file de sécurité vidée");
    assert_eq!(b.engine.state(), State::Done);
    assert_eq!(b.engine.cfg().stats.words, 3);
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(b.engine.state(), State::Idle);
}

#[tokio::test(start_paused = true)]
async fn pause_saves_to_history_then_resume() {
    let b = bench("pause", |_| {});
    b.start().await;
    b.engine.toggle_pause();
    assert_eq!(b.engine.state(), State::Paused);
    b.part(0, Some("TXT:première partie"), true, false);
    settle().await;
    let h = b.history();
    assert_eq!(h.len(), 1);
    assert!(h[0].partial);
    assert_eq!(h[0].text, "première partie");
    assert!(b.pastes().is_empty(), "rien n'est collé pendant la pause");
    assert!(b.files() >= 2, "l'audio de la pause est gardé sur le disque");
    b.engine.toggle_pause();
    assert_eq!(b.engine.state(), State::Recording);
    assert!(b.last_view().resumed);
    b.part(1, Some("TXT:et la suite"), false, true);
    settle().await;
    assert_eq!(b.pastes(), vec!["[première partie et la suite]"]);
    assert_eq!(b.ai.formats.load(Ordering::SeqCst), 1, "une seule mise en forme pour toute la dictée");
    let h = b.history();
    assert_eq!(h.len(), 1, "l'entrée provisoire est remplacée, pas dupliquée");
    assert!(!h[0].partial);
    assert_eq!(b.files(), 0);
    assert_eq!(b.cmds(), vec!["Start", "Pause", "Resume"]);
}

#[tokio::test(start_paused = true)]
async fn stop_while_paused_and_double_final() {
    let b = bench("stop-pause", |_| {});
    b.start().await;
    b.engine.pause();
    b.part(0, Some("TXT:tout"), true, false);
    b.engine.stop();
    assert_eq!(b.engine.state(), State::Working);
    b.part(1, None, false, true);
    b.part(1, None, false, true);
    settle().await;
    assert_eq!(b.pastes(), vec!["[tout]"], "collée une seule fois");
}

#[tokio::test(start_paused = true)]
async fn provider_refusal_then_retry() {
    let b = bench("retry", |_| {});
    b.ai.broken.store(true, Ordering::SeqCst);
    b.start().await;
    b.part(0, Some("TXT:une idée importante"), false, true);
    settle().await;
    let v = b.last_view();
    assert_eq!(v.state, State::Error);
    assert!(v.retry);
    assert!(v.message.unwrap().starts_with("Accès refusé — Access denied"));
    assert_eq!(lock(&b.desk.interactive).last(), Some(&true), "la bulle reste cliquable");
    assert!(b.files() >= 2, "l'audio est gardé");
    assert!(b.engine.snapshot().has_failed);
    b.ai.broken.store(false, Ordering::SeqCst);
    b.engine.bubble_clicked();
    settle().await;
    assert_eq!(b.pastes(), vec!["[une idée importante]"]);
    assert_eq!(b.history().len(), 1);
    assert_eq!(b.files(), 0);
    assert!(!b.engine.snapshot().has_failed);
}

#[tokio::test(start_paused = true)]
async fn retry_from_menu_copies_only() {
    let b = bench("retry-menu", |_| {});
    b.ai.broken.store(true, Ordering::SeqCst);
    b.start().await;
    b.part(0, Some("TXT:plus tard"), false, true);
    settle().await;
    tokio::time::sleep(Duration::from_secs(8)).await;
    assert_eq!(b.engine.state(), State::Idle);
    b.ai.broken.store(false, Ordering::SeqCst);
    b.engine.retry_last();
    settle().await;
    assert!(b.pastes().is_empty(), "jamais collée à l'aveugle");
    assert_eq!(b.copies(), vec!["[plus tard]"]);
    assert_eq!(b.engine.state(), State::Done);
}

#[tokio::test(start_paused = true)]
async fn cancel_during_pause_keeps_history() {
    let b = bench("cancel", |_| {});
    b.start().await;
    b.engine.pause();
    b.part(0, Some("TXT:déjà dit"), true, false);
    settle().await;
    b.engine.cancel();
    settle().await;
    assert_eq!(b.engine.state(), State::Idle);
    assert_eq!(b.history().len(), 1, "ce qui était dit avant la pause reste dans l'historique");
    assert!(b.pastes().is_empty());
    assert_eq!(b.files(), 0, "annuler oublie l'audio");
    // Un morceau qui arriverait encore est ignoré.
    b.engine.on_capture(Event::Part(Part {
        id: "inconnu".into(),
        seq: 9,
        audio: None,
        live: false,
        pause_part: false,
        last: true,
    }));
}

#[tokio::test(start_paused = true)]
async fn live_mode_keeps_order() {
    let b = bench("live", |c| c.live_mode = true);
    b.start().await;
    b.part(0, Some("LENT:un"), false, false);
    b.part(1, Some("TXT:deux"), false, false);
    settle().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    settle().await;
    assert_eq!(b.pastes(), vec!["[un]", " [deux]"], "dans l'ordre, avec l'espace");
    assert_eq!(b.last_view().segments, Some(2));
    b.engine.stop();
    b.part(2, None, false, true);
    settle().await;
    let h = b.history();
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].text, "[un] [deux]");
    assert_eq!(b.files(), 0);
    assert_eq!(b.engine.state(), State::Done);
}

#[tokio::test(start_paused = true)]
async fn live_failed_sentence_is_recovered_apart() {
    let b = bench("live-fail", |c| c.live_mode = true);
    let id = b.start().await;
    b.part(0, Some("TXT:un"), false, false);
    b.part(1, Some("ERR:deux"), false, false);
    settle().await;
    b.engine.stop();
    b.part(2, None, false, true);
    settle().await;
    let v = b.last_view();
    assert_eq!(v.message.as_deref(), Some("1 phrase non transcrite"));
    assert!(v.retry);
    let parts = b.engine.pending.parts(&id);
    assert_eq!(parts.iter().map(|p| p.0).collect::<Vec<_>>(), vec![1], "seul l'audio raté est gardé");
    // Le fichier de test porte « ERR: » : on le remplace par une version qui passe.
    b.engine.pending.save(&id, 1, Audio { bytes: b"TXT:deux".to_vec(), mime: "audio/wav".into(), duration_ms: 1 }, true, "Mail", true);
    b.engine.bubble_clicked();
    settle().await;
    assert_eq!(b.pastes(), vec!["[un]"], "la phrase rattrapée n'est pas collée à l'aveugle");
    assert_eq!(b.copies().last().map(String::as_str), Some("[deux]"));
    let h = b.history();
    assert_eq!(h.len(), 2, "rangée à part, sans écraser la dictée");
    assert!(h.iter().any(|e| e.id == format!("{id}-rattrapage") && e.recovered));
}

#[tokio::test(start_paused = true)]
async fn crash_recovery_from_disk() {
    let dir = tmpdir("recovery");
    {
        let p = Pending::new(dir.join("en-attente"));
        p.save("dvieux", 0, Audio { bytes: b"TXT:avant le plantage".to_vec(), mime: "audio/wav".into(), duration_ms: 1 }, false, "Notes", true);
        let mut s = p.stream("dcoupe", 0).unwrap();
        s.write(&vec![300i16; 32_000]);
    }
    let b = bench_in(dir, Config::default());
    assert_eq!(b.engine.snapshot().waiting_on_disk, 1);
    b.engine.recover_pending().await;
    settle().await;
    let h = b.history();
    assert!(h.iter().any(|e| e.text == "[avant le plantage]" && e.recovered && e.app == "Notes"));
    assert!(b.pastes().is_empty(), "jamais collée à l'aveugle");
    // Le brouillon .pcm a été transformé en morceau WAV et envoyé (le faux
    // fournisseur renvoie alors les octets bruts : peu importe, il est passé).
    assert_eq!(h.len(), 2);
    assert_eq!(b.files(), 0, "fichiers supprimés");
    assert_eq!(b.engine.state(), State::Idle, "la bulle n'est pas touchée");
}

#[tokio::test(start_paused = true)]
async fn privacy_mode_writes_nothing() {
    let b = bench("privacy", |c| c.privacy_mode = true);
    b.ai.broken.store(true, Ordering::SeqCst);
    b.start().await;
    assert!(matches!(&lock(&b.rec.cmds)[0], Cmd::Start { persist: false, .. }));
    b.part(0, Some("TXT:secret"), false, true);
    settle().await;
    assert_eq!(b.files(), 0, "rien n'est écrit sur le disque");
    b.ai.broken.store(false, Ordering::SeqCst);
    b.engine.bubble_clicked();
    settle().await;
    assert_eq!(b.pastes(), vec!["[secret]"], "le réessai marche depuis la mémoire");
    assert!(b.history().is_empty());
}

#[tokio::test(start_paused = true)]
async fn max_duration_ignores_pauses() {
    let b = bench("max", |c| c.max_minutes = 1);
    b.start().await;
    tokio::time::sleep(Duration::from_secs(30)).await;
    b.engine.pause();
    tokio::time::sleep(Duration::from_secs(120)).await;
    assert_eq!(b.engine.state(), State::Paused);
    b.engine.resume();
    b.engine.on_capture(Event::Started { device: "test".into() });
    tokio::time::sleep(Duration::from_secs(28)).await;
    assert_eq!(b.engine.state(), State::Recording);
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(b.engine.state(), State::Working);
    assert_eq!(b.cmds().last().map(String::as_str), Some("Stop"));
}

#[tokio::test(start_paused = true)]
async fn hold_mode() {
    let b = bench("hold", |c| c.mode = "hold".into());
    b.engine.shortcut_pressed();
    b.engine.shortcut_pressed(); // répétition de la touche : ignorée
    settle().await;
    assert_eq!(b.engine.state(), State::Recording);
    tokio::time::sleep(Duration::from_millis(600)).await;
    b.engine.shortcut_released();
    assert_eq!(b.engine.state(), State::Working, "relâcher termine");

    let b = bench("hold-tap", |c| c.mode = "hold".into());
    b.engine.shortcut_pressed();
    settle().await;
    b.engine.shortcut_released();
    assert_eq!(b.engine.state(), State::Recording, "un appui bref laisse la dictée continuer");
    b.engine.shortcut_pressed();
    assert_eq!(b.engine.state(), State::Working, "un second appui la termine");
}

#[tokio::test(start_paused = true)]
async fn capture_error_after_stop_still_finishes() {
    for live in [false, true] {
        let b = bench("error-working", |c| c.live_mode = live);
        b.start().await;
        b.part(0, Some("TXT:bonjour"), false, false);
        b.engine.stop();
        settle().await;
        assert_eq!(b.engine.state(), State::Working);
        // La capture lâche avant le dernier morceau.
        b.engine.on_capture(Event::Error("Micro injoignable".into()));
        settle().await;
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert_eq!(b.engine.state(), State::Idle, "live = {live}");
    }
}

#[tokio::test(start_paused = true)]
async fn stuck_transcription_gives_up_and_keeps_audio() {
    let b = bench("stuck", |_| {});
    b.start().await;
    b.engine.stop();
    settle().await;
    // Aucun dernier morceau n'arrive jamais.
    tokio::time::sleep(Duration::from_secs(181)).await;
    assert_eq!(b.engine.state(), State::Error);
    assert_eq!(b.last_view().message.as_deref(), Some("Délai dépassé"));
    tokio::time::sleep(Duration::from_secs(8)).await;
    assert_eq!(b.engine.state(), State::Idle);
}

#[tokio::test(start_paused = true)]
async fn old_dictation_does_not_touch_the_new_bubble() {
    let b = bench("overlap", |_| {});
    b.start().await;
    // Erreur de micro : la capture referme la dictée avec un dernier morceau,
    // encore en cours de transcription quand une nouvelle dictée commence.
    b.engine.on_capture(Event::Error("Micro muet".into()));
    b.part(0, Some("LENT:bonjour"), false, true);
    b.start().await;
    assert_eq!(b.engine.state(), State::Recording);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(b.pastes(), vec!["[bonjour]"], "l'ancienne dictée est bien écrite");
    assert_eq!(b.engine.state(), State::Recording, "la nouvelle dictée continue");
}

#[tokio::test(start_paused = true)]
async fn network_outage_keeps_audio_and_retries_alone() {
    let b = bench("offline", |_| {});
    b.ai.offline.store(true, Ordering::SeqCst);
    b.start().await;
    let id = b.id();
    b.part(0, Some("TXT:bonjour le monde"), false, true);
    settle().await;
    assert_eq!(b.engine.state(), State::Error);
    let v = b.last_view();
    assert_eq!(v.message.as_deref(), Some("Pas de réseau"));
    assert!(v.retry);
    assert_eq!(b.engine.pending.parts(&id).len(), 1, "l'audio est gardé");

    // Toujours pas de réseau au premier nouvel essai : rien ne se perd.
    tokio::time::sleep(Duration::from_secs(21)).await;
    assert_eq!(b.engine.pending.parts(&id).len(), 1);

    // Le réseau revient : le nouvel essai suivant retrouve le texte.
    b.ai.offline.store(false, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_secs(61)).await;
    assert_eq!(b.copies(), vec!["[bonjour le monde]"], "texte dans le presse-papier");
    assert!(b.history().iter().any(|e| e.text == "[bonjour le monde]"), "et dans l'historique");
    assert!(b.engine.pending.parts(&id).is_empty(), "audio oublié une fois transcrit");
    assert!(!b.engine.snapshot().has_failed);
}

#[tokio::test(start_paused = true)]
async fn every_dictation_lost_in_an_outage_comes_back() {
    let b = bench("offline-many", |_| {});
    b.ai.offline.store(true, Ordering::SeqCst);
    b.start().await;
    let first = b.id();
    b.part(0, Some("TXT:première"), false, true);
    settle().await;
    b.start().await;
    let second = b.id();
    b.part(0, Some("TXT:seconde"), false, true);
    settle().await;
    assert_eq!(b.engine.state(), State::Error);
    // Le réseau revient quelques minutes plus tard.
    tokio::time::sleep(Duration::from_secs(100)).await;
    b.ai.offline.store(false, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_secs(300)).await;
    let texts: Vec<String> = b.history().iter().map(|e| e.text.clone()).collect();
    assert!(texts.contains(&"[première]".to_string()), "{texts:?}");
    assert!(texts.contains(&"[seconde]".to_string()), "{texts:?}");
    assert!(b.engine.pending.parts(&first).is_empty() && b.engine.pending.parts(&second).is_empty());
}

#[tokio::test(start_paused = true)]
async fn watchdog_and_microphone() {
    let b = bench("watchdog", |_| {});
    b.engine.start();
    settle().await;
    tokio::time::sleep(Duration::from_millis(4600)).await;
    assert_eq!(b.last_view().message.as_deref(), Some("Micro injoignable"));

    let b = bench("mic-denied", |_| {});
    *lock(&b.desk.mic) = Some(MicPermission::Denied);
    b.engine.start();
    settle().await;
    assert_eq!(b.last_view().message.as_deref(), Some("Micro non autorisé"));
    assert!(lock(&b.rec.cmds).is_empty());
}

#[tokio::test(start_paused = true)]
async fn old_error_timer_does_not_hide_next_dictation() {
    let b = bench("error-seq", |_| {});
    b.ai.broken.store(true, Ordering::SeqCst);
    b.start().await;
    b.part(0, Some("TXT:a"), false, true);
    settle().await;
    assert_eq!(b.engine.state(), State::Error);
    b.ai.broken.store(false, Ordering::SeqCst);
    b.start().await;
    tokio::time::sleep(Duration::from_secs(8)).await;
    assert_eq!(b.engine.state(), State::Recording);
}

#[tokio::test(start_paused = true)]
async fn long_dictation_with_empty_transcript_is_kept() {
    let b = bench("empty-long", |_| {});
    b.start().await;
    let id = b.id();
    b.part(0, Some("TXT:"), false, false);
    b.part(1, Some("TXT:Merci d'avoir regardé !"), false, false);
    b.part(2, Some("TXT:"), false, true);
    settle().await;
    assert_eq!(b.engine.state(), State::Error);
    let v = b.last_view();
    assert_eq!(v.message.as_deref(), Some("Rien compris — réessayer"));
    assert!(v.retry, "la bulle propose de réessayer");
    assert_eq!(b.engine.pending.parts(&id).len(), 3, "l'audio n'est pas effacé");
}

#[tokio::test(start_paused = true)]
async fn whisper_inventions_and_accessibility() {
    let b = bench("hallucination", |_| {});
    b.start().await;
    b.part(0, Some("TXT:Merci d'avoir regardé !"), false, true);
    settle().await;
    assert_eq!(b.last_view().message.as_deref(), Some("Rien entendu"));
    assert!(b.pastes().is_empty());

    let b = bench("accessibility", |_| {});
    *lock(&b.desk.outcome) = Some(PasteOutcome::NeedsAccessibility);
    b.start().await;
    b.part(0, Some("TXT:texte"), false, true);
    settle().await;
    assert_eq!(b.last_view().message.as_deref(), Some("Copié — autorisez l'Accessibilité"));
    assert_eq!(b.desk.accessibility_opened.load(Ordering::SeqCst), 1);
    assert_eq!(b.copies(), vec!["[texte]"], "le texte reste dans le presse-papier");
    assert_eq!(lock(&b.desk.escape).last(), Some(&false));
}

#[test]
fn hallucination_list() {
    assert!(is_hallucination("Sous-titrage ST' 501"));
    assert!(is_hallucination("  Thank you for watching. "));
    assert!(!is_hallucination("Merci d'avoir regardé le devis, on en reparle demain."));
    let a = new_id();
    let b = new_id();
    assert_ne!(a, b);
    let _ = Path::new("");
}
