//! Capture micro native, sur un fil dédié.
//!
//! Le micro n'est ouvert que pendant la dictée : l'indicateur orange de macOS
//! s'éteint dès la fin, et aucune autre application n'en est privée. L'ouvrir
//! coûte quelques dizaines de millisecondes ; l'audio des premiers instants est
//! gardé dans la file du pilote, rien n'est perdu au démarrage.
//!
//! Ouvrir un micro n'est pas fiable dès qu'une autre application l'utilise :
//! on descend une liste — le micro choisi, celui par défaut, puis tous les
//! autres — jusqu'à obtenir un flux qui livre vraiment du son.

use super::resample::{to_mono, Resampler};
use super::vad::{normalize, Cut, Segmenter};
use super::wav::{self, SAMPLE_RATE};
use crate::pending::{Audio, Pending, Stream as DiskStream};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub enum Cmd {
    Start { id: String, live: bool, persist: bool, microphone: String },
    Pause,
    Resume,
    Stop,
    Cancel,
}

#[derive(Debug, Clone)]
pub struct Part {
    pub id: String,
    /// Numéro du morceau dans la dictée : c'est aussi son nom dans la file de sécurité.
    pub seq: usize,
    /// None : rien d'audible (bruit, silence, trop court).
    pub audio: Option<Audio>,
    pub live: bool,
    /// Morceau refermé par une pause (mode classique).
    pub pause_part: bool,
    /// Dernier morceau de la dictée.
    pub last: bool,
}

#[derive(Debug, Clone)]
pub enum Event {
    /// Une nouvelle tentative d'ouverture : la capture est vivante.
    Acquiring,
    /// Le son arrive vraiment.
    Started { device: String },
    Level(f32),
    Part(Part),
    Error(String),
}

pub type Sink = Arc<dyn Fn(Event) + Send + Sync>;

enum Msg {
    Cmd(Cmd),
    Samples(u64, Vec<f32>),
    StreamError(u64, String),
}

/// Poignée vers le fil de capture.
pub struct Capture {
    tx: Sender<Msg>,
}

impl Capture {
    pub fn spawn(pending: Arc<Pending>, sink: Sink) -> Capture {
        let (tx, rx) = mpsc::channel();
        let me = tx.clone();
        std::thread::Builder::new()
            .name("souffle-capture".into())
            .spawn(move || Worker::new(pending, sink, me).run(rx))
            .expect("fil de capture");
        Capture { tx }
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(Msg::Cmd(cmd));
    }
}

/// Noms des micros disponibles, pour les réglages.
pub fn microphones() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|it| it.filter_map(|d| device_name(&d)).collect())
        .unwrap_or_default()
}

fn device_name(d: &cpal::Device) -> Option<String> {
    d.description().ok().map(|x| x.name().to_string())
}

struct Session {
    id: String,
    live: bool,
    persist: bool,
    microphone: String,
    seq: usize,
    paused: bool,
    stream: Option<cpal::Stream>,
    /// Numéro du flux en cours : les échantillons d'un flux abandonné sont ignorés.
    generation: u64,
    device: String,
    rate: u32,
    channels: usize,
    resampler: Resampler,
    segmenter: Segmenter,
    disk: Option<DiskStream>,
    disk_buf: Vec<i16>,
    started: bool,
    tried: Vec<String>,
    opened_at: Instant,
    zero_ms: u64,
    last_level: Instant,
}

struct Worker {
    pending: Arc<Pending>,
    sink: Sink,
    me: Sender<Msg>,
    session: Option<Session>,
    generation: u64,
}

/// Délai sans le moindre échantillon avant d'essayer un autre micro.
const NO_SOUND_MS: u64 = 1800;
/// Signal strictement nul : micro coupé, ou autorisation refusée sur macOS
/// (le système livre alors du silence parfait au lieu d'une erreur).
const ZERO_SIGNAL_MS: u64 = 2500;

impl Worker {
    fn new(pending: Arc<Pending>, sink: Sink, me: Sender<Msg>) -> Worker {
        Worker { pending, sink, me, session: None, generation: 0 }
    }

    fn emit(&self, e: Event) {
        (self.sink)(e);
    }

    fn run(mut self, rx: Receiver<Msg>) {
        loop {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(Msg::Cmd(c @ (Cmd::Stop | Cmd::Pause))) => {
                    // La dernière syllabe est peut-être encore dans le pilote ou
                    // dans la file : on la laisse arriver avant de refermer.
                    std::thread::sleep(Duration::from_millis(60));
                    let mut later = vec![];
                    while let Ok(m) = rx.try_recv() {
                        match m {
                            Msg::Samples(g, s) => self.samples(g, &s),
                            other => later.push(other),
                        }
                    }
                    self.command(c);
                    for m in later {
                        match m {
                            Msg::Cmd(c) => self.command(c),
                            Msg::StreamError(g, e) => self.stream_error(g, e),
                            Msg::Samples(..) => {}
                        }
                    }
                }
                Ok(Msg::Cmd(c)) => self.command(c),
                Ok(Msg::Samples(g, s)) => self.samples(g, &s),
                Ok(Msg::StreamError(g, e)) => self.stream_error(g, e),
                Err(RecvTimeoutError::Timeout) => self.tick(),
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    fn command(&mut self, c: Cmd) {
        match c {
            Cmd::Start { id, live, persist, microphone } => {
                self.drop_session(true);
                self.session = Some(Session {
                    id,
                    live,
                    persist,
                    microphone,
                    seq: 0,
                    paused: false,
                    stream: None,
                    generation: 0,
                    device: String::new(),
                    rate: SAMPLE_RATE,
                    channels: 1,
                    resampler: Resampler::new(SAMPLE_RATE, SAMPLE_RATE),
                    segmenter: Segmenter::new(live),
                    disk: None,
                    disk_buf: vec![],
                    started: false,
                    tried: vec![],
                    opened_at: Instant::now(),
                    zero_ms: 0,
                    last_level: Instant::now(),
                });
                self.open_disk();
                self.open_next_device();
            }
            Cmd::Pause => {
                let Some(s) = self.session.as_mut() else { return };
                if s.paused {
                    return;
                }
                s.paused = true;
                // Micro libéré pendant la pause : l'indicateur s'éteint.
                s.stream = None;
                let pause_part = !s.live;
                self.close_segment(pause_part, false);
            }
            Cmd::Resume => {
                let Some(s) = self.session.as_mut() else { return };
                if !s.paused {
                    return;
                }
                s.paused = false;
                s.seq += 1;
                s.tried.clear();
                s.started = false;
                self.open_disk();
                self.open_next_device();
            }
            Cmd::Stop => {
                if let Some(s) = self.session.as_mut() {
                    s.stream = None;
                }
                self.close_segment(false, true);
                self.session = None;
            }
            Cmd::Cancel => self.drop_session(true),
        }
    }

    /// Annulation : rien n'est envoyé, le brouillon sur le disque est effacé.
    fn drop_session(&mut self, discard: bool) {
        if let Some(s) = self.session.take() {
            drop(s.stream);
            drop(s.disk);
            if discard {
                self.pending.forget_part(&s.id, s.seq);
            }
        }
    }

    fn open_disk(&mut self) {
        let Some(s) = self.session.as_mut() else { return };
        s.disk = if s.persist { self.pending.stream(&s.id, s.seq) } else { None };
        s.disk_buf.clear();
    }

    fn flush_disk(s: &mut Session) {
        if let Some(d) = s.disk.as_mut() {
            d.write(&s.disk_buf);
        }
        s.disk_buf.clear();
    }

    /// Envoie le morceau en cours.
    fn close_segment(&mut self, pause_part: bool, last: bool) {
        let Some(s) = self.session.as_mut() else { return };
        Self::flush_disk(s);
        let cut = s.segmenter.take();
        let part = Self::to_part(s, cut, pause_part, last);
        self.emit(Event::Part(part));
    }

    fn to_part(s: &Session, mut cut: Cut, pause_part: bool, last: bool) -> Part {
        let audible = cut.audible(s.live);
        let audio = audible.then(|| {
            let duration_ms = cut.duration_ms();
            normalize(&mut cut.samples);
            Audio { bytes: wav::encode(&cut.samples, SAMPLE_RATE), mime: "audio/wav".into(), duration_ms }
        });
        Part { id: s.id.clone(), seq: s.seq, audio, live: s.live, pause_part, last }
    }

    fn samples(&mut self, generation: u64, interleaved: &[f32]) {
        let Some(s) = self.session.as_mut() else { return };
        if generation != s.generation || s.paused {
            return;
        }
        if !s.started {
            s.started = true;
            crate::log!("capture active : {} ({} Hz, {} canal·aux)", s.device, s.rate, s.channels);
            let device = s.device.clone();
            (self.sink)(Event::Started { device });
        }
        let mut mono = Vec::with_capacity(interleaved.len());
        to_mono(interleaved, s.channels, &mut mono);
        if mono.iter().all(|&x| x == 0.0) {
            s.zero_ms += mono.len() as u64 * 1000 / s.rate as u64;
        } else {
            s.zero_ms = 0;
        }
        let mut pcm = Vec::with_capacity(mono.len() / 2 + 8);
        s.resampler.process(&mono, &mut pcm);

        s.disk_buf.extend(pcm.iter().map(|&x| wav::to_i16(x)));
        if s.disk_buf.len() >= SAMPLE_RATE as usize {
            Self::flush_disk(s);
        }

        let cuts = s.segmenter.push(&pcm);
        let mut parts = vec![];
        for cut in cuts {
            // Le brouillon du morceau qui part est complet : on en ouvre un nouveau.
            Self::flush_disk(s);
            parts.push(Self::to_part(s, cut, false, false));
            s.seq += 1;
            s.disk = if s.persist { self.pending.stream(&s.id, s.seq) } else { None };
        }
        let level = (s.last_level.elapsed() >= Duration::from_millis(50)).then(|| {
            s.last_level = Instant::now();
            // Compression douce : une voix normale remplit la barre sans la saturer.
            (s.segmenter.level() * 5.0).powf(0.7).min(1.0)
        });
        let zero = s.zero_ms >= ZERO_SIGNAL_MS;
        if zero {
            s.zero_ms = 0;
        }
        for p in parts {
            self.emit(Event::Part(p));
        }
        if let Some(l) = level {
            self.emit(Event::Level(l));
        }
        if zero {
            crate::log!("capture : signal strictement nul depuis {ZERO_SIGNAL_MS} ms");
            self.emit(Event::Error("Micro muet — autorisation ou micro coupé".into()));
            self.drop_session(false);
        }
    }

    fn stream_error(&mut self, generation: u64, e: String) {
        let Some(s) = self.session.as_mut() else { return };
        if generation != s.generation {
            return;
        }
        crate::log!("capture : erreur du flux ({}) : {e}", s.device);
        s.stream = None;
        if s.paused {
            return;
        }
        // Micro débranché en pleine dictée : on garde ce qui a été dit et on
        // poursuit sur le micro suivant.
        s.started = false;
        self.open_next_device();
    }

    fn tick(&mut self) {
        let Some(s) = self.session.as_mut() else { return };
        if s.paused || s.started {
            return;
        }
        if s.stream.is_some() && s.opened_at.elapsed() >= Duration::from_millis(NO_SOUND_MS) {
            crate::log!("capture : aucun son de « {} », micro suivant", s.device);
            s.stream = None;
            self.open_next_device();
        }
    }

    fn open_next_device(&mut self) {
        let Some(s) = self.session.as_mut() else { return };
        let host = cpal::default_host();
        let mut candidates: Vec<cpal::Device> = vec![];
        let all: Vec<cpal::Device> = host.input_devices().map(|it| it.collect()).unwrap_or_default();
        if !s.microphone.is_empty() {
            candidates.extend(all.iter().filter(|d| device_name(d).as_deref() == Some(s.microphone.as_str())).cloned());
        }
        if let Some(d) = host.default_input_device() {
            candidates.push(d);
        }
        candidates.extend(all);

        for device in candidates {
            let name = device_name(&device).unwrap_or_else(|| "micro".into());
            if s.tried.contains(&name) {
                continue;
            }
            s.tried.push(name.clone());
            (self.sink)(Event::Acquiring);
            self.generation += 1;
            let generation = self.generation;
            match open_stream(&device, generation, self.me.clone()) {
                Ok((stream, rate, channels)) => {
                    s.stream = Some(stream);
                    s.generation = generation;
                    s.device = name;
                    s.rate = rate;
                    s.channels = channels;
                    s.resampler = Resampler::new(rate, SAMPLE_RATE);
                    s.opened_at = Instant::now();
                    s.zero_ms = 0;
                    return;
                }
                Err(e) => crate::log!("capture : « {name} » refuse de s'ouvrir : {e}"),
            }
        }
        crate::log!("capture : aucun micro utilisable");
        self.emit(Event::Error("Micro injoignable".into()));
        // Ce qui a déjà été dit n'est pas perdu : on l'envoie comme dernier morceau.
        self.close_segment(false, true);
        self.session = None;
    }
}

fn open_stream(device: &cpal::Device, generation: u64, tx: Sender<Msg>) -> Result<(cpal::Stream, u32, usize), String> {
    let supported = device.default_input_config().map_err(|e| e.to_string())?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let rate = config.sample_rate;
    let channels = config.channels as usize;
    let err_tx = tx.clone();
    let on_err = move |e: cpal::Error| {
        let _ = err_tx.send(Msg::StreamError(generation, e.to_string()));
    };
    macro_rules! build {
        ($t:ty, $conv:expr) => {{
            let tx = tx.clone();
            device.build_input_stream(
                config.clone(),
                move |data: &[$t], _: &cpal::InputCallbackInfo| {
                    let v: Vec<f32> = data.iter().map($conv).collect();
                    let _ = tx.send(Msg::Samples(generation, v));
                },
                on_err,
                None,
            )
        }};
    }
    let stream = match format {
        cpal::SampleFormat::F32 => build!(f32, |&x| x),
        cpal::SampleFormat::I16 => build!(i16, |&x| x as f32 / 32768.0),
        cpal::SampleFormat::U16 => build!(u16, |&x| (x as f32 - 32768.0) / 32768.0),
        cpal::SampleFormat::I32 => build!(i32, |&x| x as f32 / 2_147_483_648.0),
        other => return Err(format!("format {other:?} non pris en charge")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, rate, channels))
}
