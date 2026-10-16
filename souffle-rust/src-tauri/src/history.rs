//! Historique local des dictées (rien n'est écrit en mode confidentialité).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub at: u64,
    #[serde(default)]
    pub raw: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub app: String,
    #[serde(default)]
    pub words: usize,
    /// Dictée en pause, pas encore terminée.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub partial: bool,
    /// Retranscrite plus tard (réessai, reprise après plantage).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub recovered: bool,
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub struct History {
    file: PathBuf,
    items: Vec<Entry>,
}

impl History {
    pub fn open(dir: &Path) -> History {
        let file = dir.join("history.json");
        let items = std::fs::read_to_string(&file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        History { file, items }
    }

    pub fn list(&self) -> &[Entry] {
        &self.items
    }

    /// Ajoute une entrée, ou remplace celle qui porte déjà cet identifiant.
    /// Une dictée mise en pause est versée tout de suite, puis remplacée par son
    /// texte final : elle n'apparaît qu'une fois, à la place qu'elle avait.
    pub fn upsert(&mut self, id: &str, mut entry: Entry, limit: usize) {
        entry.id = id.to_string();
        if let Some(i) = self.items.iter().position(|x| x.id == id) {
            entry.at = self.items[i].at;
            self.items[i] = entry;
        } else {
            entry.at = now_ms();
            self.items.insert(0, entry);
            self.items.truncate(limit);
        }
        self.flush();
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.flush();
    }

    /// Reprend l'historique de la version Electron, une seule fois.
    pub fn import(&mut self, file: &Path, limit: usize) -> usize {
        let Some(old) = std::fs::read_to_string(file).ok().and_then(|s| serde_json::from_str::<Vec<Entry>>(&s).ok())
        else {
            return 0;
        };
        let mut n = 0;
        for mut e in old {
            if e.id.is_empty() {
                e.id = format!("h{}-{n}", e.at);
            }
            if !self.items.iter().any(|x| x.id == e.id) {
                self.items.push(e);
                n += 1;
            }
        }
        self.items.sort_by_key(|e| std::cmp::Reverse(e.at));
        self.items.truncate(limit);
        self.flush();
        n
    }

    fn flush(&self) {
        if let Some(dir) = self.file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = self.file.with_extension("json.tmp");
        if std::fs::write(&tmp, serde_json::to_vec(&self.items).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(tmp, &self.file);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("souffle-hist-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn upsert_replaces_in_place() {
        let dir = tmp("upsert");
        let mut h = History::open(&dir);
        h.upsert("a", Entry { text: "un".into(), partial: true, ..Default::default() }, 10);
        h.upsert("b", Entry { text: "deux".into(), ..Default::default() }, 10);
        let at = h.list()[1].at;
        h.upsert("a", Entry { text: "un final".into(), ..Default::default() }, 10);
        assert_eq!(h.list().len(), 2);
        assert_eq!(h.list()[1].text, "un final");
        assert!(!h.list()[1].partial);
        assert_eq!(h.list()[1].at, at);
        assert_eq!(History::open(&dir).list().len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn limit_and_import() {
        let dir = tmp("import");
        let mut h = History::open(&dir);
        for i in 0..5 {
            h.upsert(&format!("x{i}"), Entry::default(), 3);
        }
        assert_eq!(h.list().len(), 3);
        let old = dir.join("old.json");
        std::fs::write(&old, r#"[{"id":"e1","at":5,"text":"ancien"},{"at":4,"text":"sans id"}]"#).unwrap();
        assert_eq!(h.import(&old, 100), 2);
        assert_eq!(h.import(&old, 100), 1, "une entrée sans identifiant reçoit un nouvel id à chaque import");
        let _ = std::fs::remove_dir_all(dir);
    }
}
