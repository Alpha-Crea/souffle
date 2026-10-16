//! Ramène le micro (44,1 ou 48 kHz, mono ou stéréo) à 16 kHz mono : la
//! fréquence de Whisper, et trois fois moins d'octets à envoyer.

pub struct Resampler {
    ratio: f64,
    next: f64,
    consumed: u64,
    acc: f32,
    n: u32,
    last: f32,
}

impl Resampler {
    pub fn new(input_rate: u32, output_rate: u32) -> Resampler {
        Resampler {
            ratio: input_rate as f64 / output_rate as f64,
            next: input_rate as f64 / output_rate as f64,
            consumed: 0,
            acc: 0.0,
            n: 0,
            last: 0.0,
        }
    }

    /// `input` : échantillons mono. Moyenne par fenêtre pour descendre (filtre
    /// passe-bas suffisant pour la voix), répétition pour monter.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        for &x in input {
            self.acc += x;
            self.n += 1;
            self.consumed += 1;
            self.last = x;
            while self.consumed as f64 >= self.next {
                out.push(if self.n > 0 { self.acc / self.n as f32 } else { self.last });
                self.acc = 0.0;
                self.n = 0;
                self.next += self.ratio;
            }
        }
    }
}

/// Moyenne des canaux d'un tampon entrelacé.
pub fn to_mono(interleaved: &[f32], channels: usize, out: &mut Vec<f32>) {
    if channels <= 1 {
        out.extend_from_slice(interleaved);
        return;
    }
    for frame in interleaved.chunks_exact(channels) {
        out.push(frame.iter().sum::<f32>() / channels as f32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates() {
        for (rate, n) in [(48_000u32, 48_000usize), (44_100, 44_100), (16_000, 16_000), (8_000, 8_000)] {
            let mut r = Resampler::new(rate, 16_000);
            let mut out = vec![];
            for chunk in vec![0.5f32; n].chunks(480) {
                r.process(chunk, &mut out);
            }
            assert!((out.len() as i64 - 16_000).abs() <= 1, "{rate} Hz → {}", out.len());
            assert!(out.iter().all(|&x| (x - 0.5).abs() < 1e-5));
        }
        let mut m = vec![];
        to_mono(&[1.0, 0.0, 0.5, 0.5], 2, &mut m);
        assert_eq!(m, vec![0.5, 0.5]);
    }
}
