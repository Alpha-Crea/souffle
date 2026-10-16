//! Découpage de la parole.
//!
//! Deux régimes :
//!   - classique : un seul morceau par dictée (ou par pause). Une très longue
//!     dictée est tout de même coupée sur un silence toutes les quatre à six
//!     minutes : chaque morceau reste sous la taille acceptée par les
//!     fournisseurs, part à la transcription pendant qu'on parle encore, et
//!     la fin de dictée n'a plus que le dernier morceau à attendre ;
//!   - direct : on coupe à chaque fin de phrase pour écrire au fil de l'eau.

use super::wav::{to_i16, SAMPLE_RATE};

/// 20 ms d'audio à 16 kHz.
pub const FRAME: usize = (SAMPLE_RATE / 50) as usize;
const FRAME_MS: u32 = 20;

/// Blanc qui clôt une phrase en mode direct.
const SILENCE_TO_CUT_MS: u32 = 500;
/// En deçà, ce n'est pas une phrase mais un bruit.
const MIN_SPEECH_MS: u32 = 600;
/// En mode direct, on coupe de force pour ne pas accumuler.
const MAX_LIVE_SEGMENT_MS: u32 = 15_000;
/// Avant la première parole d'une phrase, on ne garde qu'un court élan.
const PRE_ROLL_MS: u32 = 400;
/// Mode classique : coupe sur un silence passé ce délai…
const CLASSIC_SOFT_CUT_MS: u32 = 240_000;
const CLASSIC_SOFT_SILENCE_MS: u32 = 700;
/// … et de force passé celui-ci.
const CLASSIC_HARD_CUT_MS: u32 = 360_000;
/// Un morceau dont aucune trame ne dépasse ce niveau est un pur silence :
/// l'envoyer ferait inventer à Whisper un « Merci d'avoir regardé ».
const SILENT_PEAK_RMS: f32 = 0.006;

#[derive(Debug, Clone)]
pub struct Cut {
    pub samples: Vec<i16>,
    pub speech_ms: u32,
    pub peak_rms: f32,
}

impl Cut {
    pub fn duration_ms(&self) -> u64 {
        super::wav::duration_ms(self.samples.len(), SAMPLE_RATE)
    }

    /// Y a-t-il quelque chose à transcrire ?
    pub fn audible(&self, live: bool) -> bool {
        if self.duration_ms() < 350 {
            return false;
        }
        if live {
            self.speech_ms >= MIN_SPEECH_MS
        } else {
            // En mode classique, on ne jette que le silence pur : mieux vaut une
            // transcription vide qu'une phrase perdue parce que la voix était basse.
            self.peak_rms >= SILENT_PEAK_RMS
        }
    }
}

pub struct Segmenter {
    live: bool,
    buf: Vec<i16>,
    frame: Vec<f32>,
    speech_ms: u32,
    silence_ms: u32,
    seg_ms: u32,
    peak_rms: f32,
    floor: f32,
    last_rms: f32,
    started: bool,
}

impl Segmenter {
    pub fn new(live: bool) -> Segmenter {
        Segmenter {
            live,
            buf: Vec::with_capacity(SAMPLE_RATE as usize * 30),
            frame: Vec::with_capacity(FRAME),
            speech_ms: 0,
            silence_ms: 0,
            seg_ms: 0,
            peak_rms: 0.0,
            floor: 0.003,
            last_rms: 0.0,
            started: false,
        }
    }

    pub fn level(&self) -> f32 {
        self.last_rms
    }

    /// Seuil de voix : au-dessus du bruit de fond mesuré, jamais en dessous
    /// d'un plancher. Le bruit de fond baisse vite et ne monte que vers des
    /// niveaux proches de lui : une voix ne le relève jamais, un ventilateur
    /// qui démarre si.
    fn is_speech(&mut self, rms: f32) -> bool {
        if !self.started {
            // Première mesure : point de départ du bruit de fond. Si l'on parle
            // déjà, il redescend au premier souffle entre deux mots.
            self.started = true;
            self.floor = rms.clamp(0.001, 0.03);
        }
        if rms < self.floor {
            self.floor = (self.floor * 0.7 + rms * 0.3).max(0.0005);
        } else if rms < self.floor * 4.0 {
            self.floor += (rms - self.floor) * 0.01;
        }
        rms >= (self.floor * 3.0).max(0.008)
    }

    /// Ajoute des échantillons (mono, 16 kHz). Renvoie les morceaux à envoyer.
    pub fn push(&mut self, samples: &[f32]) -> Vec<Cut> {
        let mut out = vec![];
        for &s in samples {
            self.frame.push(s);
            if self.frame.len() == FRAME {
                if let Some(cut) = self.end_frame() {
                    out.push(cut);
                }
            }
        }
        out
    }

    fn end_frame(&mut self) -> Option<Cut> {
        let rms = (self.frame.iter().map(|x| x * x).sum::<f32>() / FRAME as f32).sqrt();
        self.last_rms = rms;
        self.peak_rms = self.peak_rms.max(rms);
        let speech = self.is_speech(rms);
        self.buf.extend(self.frame.iter().map(|&x| to_i16(x)));
        self.frame.clear();
        self.seg_ms += FRAME_MS;

        if speech {
            self.speech_ms += FRAME_MS;
            self.silence_ms = 0;
        } else {
            self.silence_ms += FRAME_MS;
        }

        if self.live {
            if self.speech_ms == 0 {
                // Rien dit encore : on ne garde qu'un court élan avant la parole.
                let keep = (SAMPLE_RATE * PRE_ROLL_MS / 1000) as usize;
                if self.buf.len() > keep * 2 {
                    self.buf.drain(..self.buf.len() - keep);
                    self.seg_ms = PRE_ROLL_MS;
                    self.peak_rms = rms;
                }
                return None;
            }
            let end_of_sentence = self.speech_ms >= MIN_SPEECH_MS && self.silence_ms >= SILENCE_TO_CUT_MS;
            let too_long = self.seg_ms >= MAX_LIVE_SEGMENT_MS && self.speech_ms >= MIN_SPEECH_MS;
            if end_of_sentence || too_long {
                return Some(self.take());
            }
            // Un bruit isolé suivi d'un long blanc : on repart de zéro.
            if self.speech_ms < MIN_SPEECH_MS && self.silence_ms >= 1500 {
                self.speech_ms = 0;
            }
            return None;
        }

        let soft = self.seg_ms >= CLASSIC_SOFT_CUT_MS && self.silence_ms >= CLASSIC_SOFT_SILENCE_MS;
        if soft || self.seg_ms >= CLASSIC_HARD_CUT_MS {
            return Some(self.take());
        }
        None
    }

    /// Referme le morceau en cours (pause, arrêt).
    pub fn take(&mut self) -> Cut {
        // La trame entamée fait partie de la parole : on la garde.
        if !self.frame.is_empty() {
            self.buf.extend(self.frame.drain(..).map(to_i16));
        }
        let cut = Cut { samples: std::mem::take(&mut self.buf), speech_ms: self.speech_ms, peak_rms: self.peak_rms };
        self.speech_ms = 0;
        self.silence_ms = 0;
        self.seg_ms = 0;
        self.peak_rms = 0.0;
        cut
    }
}

/// Une voix lointaine ou un micro faible donne un signal très bas, que Whisper
/// comprend moins bien. On le remonte, sans jamais saturer ni amplifier un silence.
pub fn normalize(samples: &mut [i16]) {
    let peak = samples.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0) as f32;
    if peak < 300.0 {
        return;
    }
    let gain = (0.89 * i16::MAX as f32 / peak).min(6.0);
    if gain <= 1.05 {
        return;
    }
    for s in samples.iter_mut() {
        *s = (*s as f32 * gain).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(ms: u32, amp: f32) -> Vec<f32> {
        let n = (SAMPLE_RATE * ms / 1000) as usize;
        (0..n).map(|i| amp * (i as f32 * 0.19).sin()).collect()
    }

    fn silence(ms: u32) -> Vec<f32> {
        tone(ms, 0.0005)
    }

    #[test]
    fn live_cuts_at_end_of_sentence() {
        let mut s = Segmenter::new(true);
        assert!(s.push(&silence(3000)).is_empty());
        assert!(s.push(&tone(1200, 0.2)).is_empty());
        let cuts = s.push(&silence(700));
        assert_eq!(cuts.len(), 1);
        let c = &cuts[0];
        assert!(c.audible(true));
        // Trois secondes de blanc avant la phrase ne sont pas envoyées.
        assert!(c.duration_ms() < 2300, "{} ms", c.duration_ms());
        // Un claquement isolé n'est pas une phrase.
        assert!(s.push(&tone(100, 0.3)).is_empty());
        assert!(s.push(&silence(2000)).is_empty());
        assert!(!s.take().audible(true));
    }

    #[test]
    fn live_forces_long_sentences() {
        let mut s = Segmenter::new(true);
        let cuts = s.push(&tone(16_000, 0.2));
        assert_eq!(cuts.len(), 1);
        assert!(cuts[0].duration_ms() >= 15_000);
    }

    #[test]
    fn classic_keeps_everything_until_long() {
        let mut s = Segmenter::new(false);
        assert!(s.push(&tone(1000, 0.2)).is_empty());
        assert!(s.push(&silence(5000)).is_empty());
        assert!(s.push(&tone(1000, 0.2)).is_empty());
        let c = s.take();
        assert_eq!(c.duration_ms(), 7000);
        assert!(c.audible(false));
        // Un silence pur n'est pas envoyé.
        s.push(&silence(4000));
        assert!(!s.take().audible(false));
        // Une voix très basse, elle, l'est.
        s.push(&tone(2000, 0.012));
        assert!(s.take().audible(false));
    }

    #[test]
    fn classic_cuts_very_long_dictations_on_a_silence() {
        let mut s = Segmenter::new(false);
        let mut cuts = vec![];
        for _ in 0..245 {
            cuts.extend(s.push(&tone(1000, 0.2)));
        }
        assert!(cuts.is_empty(), "pas de coupe sans silence avant six minutes");
        cuts.extend(s.push(&silence(800)));
        assert_eq!(cuts.len(), 1);
        for _ in 0..365 {
            cuts.extend(s.push(&tone(1000, 0.2)));
        }
        assert_eq!(cuts.len(), 2, "coupe forcée à six minutes");
    }

    #[test]
    fn normalize_raises_quiet_voices() {
        let mut v = vec![1000i16, -2000, 500];
        normalize(&mut v);
        assert!(v[1] < -11_000 && v[1] > -12_500);
        let mut quiet = vec![10i16, -20];
        normalize(&mut quiet);
        assert_eq!(quiet, vec![10, -20]);
    }
}
