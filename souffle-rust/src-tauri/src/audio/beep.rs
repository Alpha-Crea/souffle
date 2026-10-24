//! Petits sons générés : aucun fichier à embarquer.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Beep {
    Start,
    Done,
    Error,
}

impl Beep {
    fn spec(self) -> (f32, f32, f32) {
        match self {
            Beep::Start => (660.0, 0.09, 0.045),
            Beep::Done => (880.0, 0.08, 0.045),
            Beep::Error => (220.0, 0.16, 0.05),
        }
    }
}

/// Joue le son sur un fil à part : la dictée n'attend jamais la sortie audio.
pub fn play(kind: Beep) {
    std::thread::spawn(move || {
        if let Err(e) = play_blocking(kind) {
            crate::log!("son indisponible : {e}");
        }
    });
}

fn play_blocking(kind: Beep) -> Result<(), String> {
    let (freq, duration, gain) = kind.spec();
    let device = cpal::default_host().default_output_device().ok_or("aucune sortie audio")?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    if supported.sample_format() != cpal::SampleFormat::F32 {
        return Err("format de sortie non pris en charge".into());
    }
    let config: cpal::StreamConfig = supported.into();
    let rate = config.sample_rate as f32;
    let channels = config.channels as usize;
    let total = (rate * (duration + 0.02)) as usize;
    let mut n = 0usize;
    let stream = device
        .build_output_stream(
            config,
            move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for frame in out.chunks_mut(channels) {
                    let t = n as f32 / rate;
                    // Attaque de 12 ms, puis extinction exponentielle : pas de clic.
                    let env = if t < 0.012 { t / 0.012 } else { (-(t - 0.012) * 6.0 / duration).exp() };
                    let v = if n < total { gain * env * (2.0 * std::f32::consts::PI * freq * t).sin() } else { 0.0 };
                    frame.iter_mut().for_each(|s| *s = v);
                    n += 1;
                }
            },
            |e| crate::log!("sortie audio : {e}"),
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_secs_f32(duration + 0.08));
    Ok(())
}
