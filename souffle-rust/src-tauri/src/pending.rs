//! File de sécurité des dictées.
//!
//! Chaque morceau d'audio est gardé dès son arrivée et n'est jeté qu'une fois
//! son texte versé dans l'historique. Réseau coupé, fournisseur qui refuse, VPN
//! qui bloque, plantage : rien de ce qui a été dit n'est perdu.
//!
//! Trois niveaux :
//!   - en mémoire, toujours : c'est ce qui sert au « Réessayer » immédiat ;
//!   - sur le disque, sauf en mode confidentialité : ce qui survit à un plantage ;
//!   - au fil de l'eau : pendant qu'on parle, l'audio s'écrit sur le disque
//!     seconde après seconde (`.pcm`). Même un plantage en pleine phrase ne
//!     perd que la dernière seconde.

use crate::audio::wav;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Clone, Debug)]
pub struct Audio {
    pub bytes: Vec<u8>,
    pub mime: String,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub id: String,
    pub index: usize,
    #[serde(default = "default_mime")]
    pub mime: String,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub at: u64,
    #[serde(default)]
    pub live: bool,
    #[serde(default)]
    pub app: String,
}

fn default_mime() -> String {
    "audio/webm".into()
}

pub fn ext_for(mime: &str) -> &'static str {
    if mime.contains("wav") {
        "wav"
    } else if mime.contains("ogg") {
        "ogg"
    } else if mime.contains("mp4") {
        "mp4"
    } else {
        "webm"
    }
}

fn base_name(id: &str, index: usize) -> String {
    format!("{id}-{index:03}")
}

pub struct Pending {
    dir: PathBuf,
    memory: Mutex<HashMap<String, BTreeMap<usize, Audio>>>,
}

/// Écriture au fil de l'eau d'un morceau en cours d'enregistrement.
pub struct Stream {
    file: File,
}

impl Stream {
    pub fn write(&mut self, samples: &[i16]) {
        let mut buf = Vec::with_capacity(samples.len() * 2);
        for s in samples {
            buf.extend_from_slice(&s.to_le_bytes());
        }
        let _ = self.file.write_all(&buf);
    }
}

impl Pending {
    pub fn new(dir: PathBuf) -> Pending {
        Pending { dir, memory: Mutex::new(HashMap::new()) }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn mem(&self) -> std::sync::MutexGuard<'_, HashMap<String, BTreeMap<usize, Audio>>> {
        self.memory.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Ouvre l'écriture au fil de l'eau d'un morceau.
    pub fn stream(&self, id: &str, index: usize) -> Option<Stream> {
        std::fs::create_dir_all(&self.dir).ok()?;
        let path = self.dir.join(format!("{}.pcm", base_name(id, index)));
        OpenOptions::new().create(true).append(true).open(path).ok().map(|file| Stream { file })
    }

    /// Garde un morceau terminé. `persist` vaut false en mode confidentialité.
    pub fn save(&self, id: &str, index: usize, audio: Audio, live: bool, app: &str, persist: bool) {
        self.mem().entry(id.to_string()).or_default().insert(index, audio.clone());
        let base = self.dir.join(base_name(id, index));
        if persist {
            let meta = Meta {
                id: id.into(),
                index,
                mime: audio.mime.clone(),
                duration_ms: audio.duration_ms,
                at: crate::history::now_ms(),
                live,
                app: app.into(),
            };
            let res = std::fs::create_dir_all(&self.dir)
                .and_then(|_| std::fs::write(base.with_extension(ext_for(&audio.mime)), &audio.bytes))
                // Les métadonnées en dernier : un morceau sans .json n'est jamais relu à moitié écrit.
                .and_then(|_| std::fs::write(base.with_extension("json"), serde_json::to_vec(&meta).unwrap_or_default()));
            if let Err(e) = res {
                crate::log!("file de sécurité : écriture impossible : {e}");
            }
        }
        // Le morceau est complet : son brouillon au fil de l'eau ne sert plus.
        let _ = std::fs::remove_file(base.with_extension("pcm"));
    }

    /// Morceaux d'une dictée, dans l'ordre : mémoire d'abord, disque sinon.
    pub fn parts(&self, id: &str) -> Vec<(usize, Audio)> {
        if let Some(m) = self.mem().get(id) {
            if !m.is_empty() {
                return m.iter().map(|(i, a)| (*i, a.clone())).collect();
            }
        }
        self.on_disk()
            .into_iter()
            .filter(|m| m.id == id)
            .filter_map(|m| {
                let bytes = std::fs::read(self.dir.join(base_name(&m.id, m.index)).with_extension(ext_for(&m.mime))).ok()?;
                (!bytes.is_empty()).then(|| (m.index, Audio { bytes, mime: m.mime.clone(), duration_ms: m.duration_ms }))
            })
            .collect()
    }

    /// Métadonnées de tout ce qui attend sur le disque.
    pub fn on_disk(&self) -> Vec<Meta> {
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return vec![] };
        let mut metas: Vec<Meta> = rd
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok())
            .collect();
        metas.sort_by(|a, b| if a.id == b.id { a.index.cmp(&b.index) } else { a.at.cmp(&b.at) });
        metas
    }

    /// Dictées restées sur le disque (plantage, redémarrage), une ligne par dictée.
    pub fn dictations_on_disk(&self) -> Vec<Meta> {
        let mut seen: Vec<Meta> = vec![];
        for m in self.on_disk() {
            if !seen.iter().any(|s| s.id == m.id) {
                seen.push(m);
            }
        }
        seen
    }

    /// Au démarrage : les brouillons au fil de l'eau d'une dictée interrompue
    /// par un plantage deviennent des morceaux ordinaires, prêts à retranscrire.
    pub fn salvage_streams(&self) -> usize {
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return 0 };
        let mut n = 0;
        for e in rd.flatten() {
            let path = e.path();
            if path.extension().is_none_or(|x| x != "pcm") {
                continue;
            }
            if path.with_extension("json").exists() {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
            let Some((id, index)) = stem.rsplit_once('-').and_then(|(id, i)| Some((id.to_string(), i.parse::<usize>().ok()?)))
            else {
                continue;
            };
            let bytes = std::fs::read(&path).unwrap_or_default();
            let samples = bytes.len() / 2;
            if wav::duration_ms(samples, wav::SAMPLE_RATE) < 350 {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            let audio = Audio {
                bytes: wav::from_raw_pcm(&bytes, wav::SAMPLE_RATE),
                mime: "audio/wav".into(),
                duration_ms: wav::duration_ms(samples, wav::SAMPLE_RATE),
            };
            self.save(&id, index, audio, false, "", true);
            self.mem().remove(&id);
            n += 1;
        }
        n
    }

    /// Un morceau est arrivé à bon port (mode direct) : on oublie son seul audio.
    pub fn forget_part(&self, id: &str, index: usize) {
        if let Some(m) = self.mem().get_mut(id) {
            m.remove(&index);
        }
        self.remove_matching(&format!("{}.", base_name(id, index)));
    }

    /// La dictée est arrivée à bon port : on oublie son audio, partout.
    pub fn forget(&self, id: &str) {
        self.mem().remove(id);
        self.remove_matching(&format!("{id}-"));
    }

    fn remove_matching(&self, prefix: &str) {
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return };
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with(prefix) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    /// Reprend les morceaux laissés par la version Electron.
    pub fn import_dir(&self, old: &Path) -> usize {
        let Ok(rd) = std::fs::read_dir(old) else { return 0 };
        let _ = std::fs::create_dir_all(&self.dir);
        let mut n = 0;
        for e in rd.flatten() {
            let to = self.dir.join(e.file_name());
            if !to.exists() && std::fs::copy(e.path(), &to).is_ok() {
                n += 1;
            }
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("souffle-pending-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn audio(n: u8) -> Audio {
        Audio { bytes: vec![n; 10], mime: "audio/wav".into(), duration_ms: 1000 }
    }

    #[test]
    fn save_parts_forget() {
        let dir = tmp("basic");
        let p = Pending::new(dir.clone());
        p.save("d1", 1, audio(2), false, "Mail", true);
        p.save("d1", 0, audio(1), false, "Mail", true);
        p.save("d2", 0, audio(3), true, "", false);
        assert_eq!(p.parts("d1").iter().map(|x| x.0).collect::<Vec<_>>(), vec![0, 1]);
        // Après un redémarrage, seul le disque reste : d2 (confidentialité) a disparu.
        let q = Pending::new(dir.clone());
        assert_eq!(q.parts("d1").len(), 2);
        assert!(q.parts("d2").is_empty());
        assert_eq!(q.dictations_on_disk().len(), 1);
        assert_eq!(q.dictations_on_disk()[0].app, "Mail");
        q.forget_part("d1", 0);
        assert_eq!(Pending::new(dir.clone()).parts("d1").len(), 1);
        q.forget("d1");
        assert!(Pending::new(dir.clone()).on_disk().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn streams_survive_a_crash() {
        let dir = tmp("stream");
        let p = Pending::new(dir.clone());
        let mut s = p.stream("d9", 0).unwrap();
        s.write(&vec![1000i16; 16_000]); // une seconde
        drop(s);
        let mut short = p.stream("d9", 1).unwrap();
        short.write(&[1i16; 100]); // un bruit : rien à garder
        drop(short);
        // Redémarrage
        let q = Pending::new(dir.clone());
        assert_eq!(q.salvage_streams(), 1);
        let parts = q.parts("d9");
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].1.duration_ms, 1000);
        assert_eq!(&parts[0].1.bytes[0..4], b"RIFF");
        assert!(std::fs::read_dir(&dir).unwrap().flatten().all(|e| e.path().extension().unwrap() != "pcm"));
        // Un morceau terminé normalement efface son brouillon.
        let mut s = q.stream("d10", 0).unwrap();
        s.write(&[5i16; 20_000]);
        drop(s);
        q.save("d10", 0, audio(1), false, "", true);
        assert_eq!(q.salvage_streams(), 0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
