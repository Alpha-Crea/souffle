//! WAV 16 bits mono : le format le plus simple que tous les fournisseurs
//! acceptent, sans aucun codec à embarquer.

pub const SAMPLE_RATE: u32 = 16_000;

pub fn encode(samples: &[i16], rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // taille du bloc fmt
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes()); // octets par seconde
    out.extend_from_slice(&2u16.to_le_bytes()); // octets par échantillon
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Échantillons bruts (i16 petit-boutiste) tels qu'écrits au fil de l'eau sur le disque.
pub fn from_raw_pcm(bytes: &[u8], rate: u32) -> Vec<u8> {
    let samples: Vec<i16> = bytes.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
    encode(&samples, rate)
}

pub fn duration_ms(samples: usize, rate: u32) -> u64 {
    samples as u64 * 1000 / rate as u64
}

pub fn to_i16(x: f32) -> i16 {
    (x.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_is_valid() {
        let w = encode(&[0, 1000, -1000], SAMPLE_RATE);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), 16_000);
        assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 6);
        assert_eq!(w.len(), 50);
        assert_eq!(from_raw_pcm(&w[44..], SAMPLE_RATE), w);
    }
}
