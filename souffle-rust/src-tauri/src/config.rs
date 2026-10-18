//! Réglages : un fichier JSON lisible, plus les clés API dans le trousseau du
//! système (Trousseau macOS, Gestionnaire d'identification Windows).
//!
//! Les noms des champs sont ceux de la version Electron : au premier lancement,
//! ses réglages et son historique sont repris tels quels.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const DEFAULT_MAX_MINUTES: u32 = 10;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Snippet {
    #[serde(default)]
    pub trigger: String,
    #[serde(default)]
    pub value: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct Position {
    pub fx: f64,
    pub fy: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct Stats {
    #[serde(default)]
    pub words: u64,
    #[serde(default)]
    pub sessions: u64,
    #[serde(default)]
    pub seconds: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    // Déclenchement
    pub shortcut: String,
    /// "toggle" | "hold"
    pub mode: String,
    /// Échap pendant la dictée : "stop" | "cancel" | "off"
    pub escape_action: String,
    pub live_mode: bool,
    pub max_minutes: u32,
    pub play_sounds: bool,
    pub auto_paste: bool,
    pub restore_clipboard: bool,
    pub refocus_target: bool,
    pub launch_at_login: bool,
    /// Nom du micro choisi ; vide = micro par défaut du système.
    pub microphone: String,

    // Transcription
    pub stt_provider: String,
    pub stt_model: String,
    pub custom_stt_url: String,
    pub language: String,
    pub output_mode: String,
    pub target_language: String,

    // Reformulation
    pub format_enabled: bool,
    pub llm_provider: String,
    pub llm_model: String,
    pub custom_llm_url: String,
    pub tone: String,
    pub app_tones: BTreeMap<String, String>,
    /// Groq en panne ou bloqué (VPN, 403) : on bascule sur OpenAI, et inversement,
    /// si l'autre clé est renseignée.
    pub provider_fallback: bool,

    // Personnalisation
    pub dictionary: Vec<String>,
    pub snippets: Vec<Snippet>,
    pub custom_instructions: String,

    // Confidentialité
    pub privacy_mode: bool,
    pub history_limit: usize,

    // Apparence
    /// "system" | "light" | "dark"
    pub theme: String,
    pub overlay_position: Option<Position>,

    pub stats: Stats,
    pub onboarded: bool,
    /// Migrations des réglages déjà appliquées.
    pub revision: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            // ⌥ Espace sur Mac, Alt + Espace sur PC : le même geste partout.
            // (Ctrl+Espace est réservé au changement de langue du clavier.)
            shortcut: "Alt+Space".into(),
            mode: "toggle".into(),
            escape_action: "stop".into(),
            live_mode: false,
            max_minutes: DEFAULT_MAX_MINUTES,
            play_sounds: true,
            auto_paste: true,
            // Restaurer l'ancien presse-papier fait perdre la dictée dès que le
            // collage rate. Tant que le texte y reste, un ⌘V manuel rattrape tout.
            restore_clipboard: false,
            refocus_target: true,
            launch_at_login: false,
            microphone: String::new(),
            stt_provider: "openai".into(),
            stt_model: "gpt-4o-mini-transcribe".into(),
            custom_stt_url: String::new(),
            language: "auto".into(),
            output_mode: "same".into(),
            target_language: "fr".into(),
            format_enabled: true,
            llm_provider: "openai".into(),
            llm_model: "gpt-4o-mini".into(),
            custom_llm_url: String::new(),
            tone: "neutre".into(),
            app_tones: BTreeMap::new(),
            provider_fallback: true,
            dictionary: vec![],
            snippets: vec![],
            custom_instructions: String::new(),
            privacy_mode: false,
            history_limit: 100,
            theme: "system".into(),
            overlay_position: None,
            stats: Stats::default(),
            onboarded: false,
            revision: 0,
        }
    }
}

/// Modèle de transcription et de reformulation conseillés par fournisseur.
pub fn default_models(provider: &str) -> (&'static str, &'static str) {
    match provider {
        "groq" => ("whisper-large-v3-turbo", "openai/gpt-oss-120b"),
        _ => ("gpt-4o-mini-transcribe", "gpt-4o-mini"),
    }
}

/// Modèle de reformulation sûr, utilisé si celui choisi est refusé (retiré,
/// mal orthographié, paramètres non pris en charge).
pub fn safe_llm_model(provider: &str) -> &'static str {
    match provider {
        "groq" => "llama-3.3-70b-versatile",
        _ => "gpt-4o-mini",
    }
}

impl Config {
    pub fn max_recording_ms(&self) -> u64 {
        let m = if (1..=60).contains(&self.max_minutes) { self.max_minutes } else { DEFAULT_MAX_MINUTES };
        m as u64 * 60_000
    }

    /// Applique un changement partiel venu des réglages. Un champ au mauvais
    /// type est refusé en bloc : les réglages enregistrés restent cohérents.
    pub fn patched(&self, patch: &Value) -> Result<Config, String> {
        let mut current = serde_json::to_value(self).map_err(|e| e.to_string())?;
        let (Some(obj), Some(changes)) = (current.as_object_mut(), patch.as_object()) else {
            return Err("réglages illisibles".into());
        };
        for (k, v) in changes {
            obj.insert(k.clone(), v.clone());
        }
        serde_json::from_value(current).map_err(|e| format!("réglage invalide : {e}"))
    }

    /// Lecture tolérante : un champ inconnu ou abîmé ne fait pas perdre les autres.
    pub fn from_json(raw: &str) -> Config {
        let Ok(Value::Object(map)) = serde_json::from_str::<Value>(raw) else {
            return Config::default();
        };
        let mut cfg = Config::default();
        for (k, v) in map {
            if k.starts_with("__") {
                continue;
            }
            let mut one = Map::new();
            one.insert(k, v);
            if let Ok(next) = cfg.patched(&Value::Object(one)) {
                cfg = next;
            }
        }
        cfg
    }

    pub fn load(file: &Path) -> Option<Config> {
        std::fs::read_to_string(file).ok().map(|s| Config::from_json(&s))
    }

    pub fn save(&self, file: &Path) -> std::io::Result<()> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = file.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).unwrap_or_default())?;
        std::fs::rename(tmp, file)
    }

    /// Mises à jour des réglages, une seule fois chacune. Renvoie true si
    /// quelque chose a changé.
    pub fn upgrade(&mut self) -> bool {
        let before = self.revision;
        if self.revision < 1 {
            // L'ancien raccourci de Windows (trois touches) devient Alt + Espace,
            // comme ⌥ Espace sur Mac.
            if cfg!(target_os = "windows") && self.shortcut == "Control+Shift+Space" {
                self.shortcut = "Alt+Space".into();
            }
            self.revision = 1;
        }
        self.revision != before
    }

    /// Réglages repris de la version Electron. Le meilleur modèle de
    /// reformulation chez Groq remplace l'ancien réglage par défaut : un modèle
    /// refusé retombe de toute façon sur l'ancien (voir `safe_llm_model`).
    pub fn migrated_from_electron(mut self) -> Config {
        if self.llm_provider == "groq" && self.llm_model == "llama-3.3-70b-versatile" {
            self.llm_model = default_models("groq").1.into();
        }
        self
    }
}

/* ------------------------------------------------------------------ */
/* Clés API                                                            */
/* ------------------------------------------------------------------ */

pub const SECRET_KEYS: [&str; 3] = ["openaiKey", "groqKey", "customKey"];

pub fn key_name_for(provider: &str) -> &'static str {
    match provider {
        "groq" => "groqKey",
        "custom" => "customKey",
        _ => "openaiKey",
    }
}

/// Les clés vivent dans le trousseau du système. S'il est indisponible (Linux
/// sans service de secrets), elles retombent dans un fichier réservé à
/// l'utilisateur, jamais dans le fichier de réglages.
pub struct Secrets {
    fallback: PathBuf,
    cache: BTreeMap<String, String>,
    use_keyring: bool,
}

const SERVICE: &str = "fr.alphagency.souffle";

impl Secrets {
    pub fn new(dir: &Path, use_keyring: bool) -> Secrets {
        let fallback = dir.join("cles.json");
        let mut cache: BTreeMap<String, String> = std::fs::read_to_string(&fallback)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if use_keyring {
            for k in SECRET_KEYS {
                if let Ok(entry) = keyring::Entry::new(SERVICE, k) {
                    if let Ok(v) = entry.get_password() {
                        cache.insert(k.to_string(), v);
                    }
                }
            }
        }
        Secrets { fallback, cache, use_keyring }
    }

    pub fn get(&self, key: &str) -> String {
        self.cache.get(key).cloned().unwrap_or_default()
    }

    pub fn set(&mut self, key: &str, value: &str) {
        if !SECRET_KEYS.contains(&key) {
            return;
        }
        let value = value.trim().to_string();
        let mut stored = false;
        if self.use_keyring {
            if let Ok(entry) = keyring::Entry::new(SERVICE, key) {
                stored = if value.is_empty() {
                    entry.delete_credential().is_ok() || entry.get_password().is_err()
                } else {
                    entry.set_password(&value).is_ok()
                };
            }
        }
        if value.is_empty() {
            self.cache.remove(key);
        } else {
            self.cache.insert(key.to_string(), value);
        }
        if stored {
            // Le trousseau a pris la clé : aucune copie ne reste sur le disque.
            let mut file = self.read_fallback();
            if file.remove(key).is_some() {
                self.write_fallback(&file);
            }
        } else {
            let mut file = self.read_fallback();
            match self.cache.get(key) {
                Some(v) => file.insert(key.to_string(), v.clone()),
                None => file.remove(key),
            };
            self.write_fallback(&file);
        }
    }

    pub fn status(&self) -> BTreeMap<String, bool> {
        SECRET_KEYS.iter().map(|k| (k.to_string(), !self.get(k).is_empty())).collect()
    }

    fn read_fallback(&self) -> BTreeMap<String, String> {
        std::fs::read_to_string(&self.fallback)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn write_fallback(&self, map: &BTreeMap<String, String>) {
        if map.is_empty() {
            let _ = std::fs::remove_file(&self.fallback);
            return;
        }
        if let Some(dir) = self.fallback.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&self.fallback, serde_json::to_vec(map).unwrap_or_default());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.fallback, std::fs::Permissions::from_mode(0o600));
        }
    }
}

/* ------------------------------------------------------------------ */
/* Emplacements                                                        */
/* ------------------------------------------------------------------ */

/// Dossier de la version Electron (`userData`).
pub fn electron_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("Souffle"))
}

/// Dossier de cette version : à l'intérieur de celui d'Electron, pour ne pas
/// semer un second dossier « Souffle » chez l'utilisateur.
pub fn data_dir() -> PathBuf {
    electron_dir().unwrap_or_else(|| std::env::temp_dir().join("Souffle")).join("natif")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn electron_config_is_read_as_is() {
        let raw = r#"{"shortcut":"Alt+Space","sttProvider":"groq","llmProvider":"groq",
            "llmModel":"llama-3.3-70b-versatile","dictionary":["KACHÉ"],"liveMode":true,
            "appTones":{"Slack":"casual"},"snippets":[{"trigger":"mon email","value":"a@b.fr"}],
            "__secrets":{"groqKey":"xxx"},"unknown":42,"maxMinutes":"oops"}"#;
        let cfg = Config::from_json(raw).migrated_from_electron();
        assert_eq!(cfg.stt_provider, "groq");
        assert!(cfg.live_mode);
        assert_eq!(cfg.dictionary, vec!["KACHÉ"]);
        assert_eq!(cfg.app_tones.get("Slack").map(String::as_str), Some("casual"));
        assert_eq!(cfg.snippets[0].value, "a@b.fr");
        // champ abîmé : valeur par défaut, le reste est gardé
        assert_eq!(cfg.max_minutes, DEFAULT_MAX_MINUTES);
        assert_eq!(cfg.llm_model, "openai/gpt-oss-120b");
    }

    #[test]
    fn upgrade_runs_once() {
        let mut cfg = Config { shortcut: "Control+Shift+Space".into(), ..Default::default() };
        assert!(cfg.upgrade());
        assert_eq!(cfg.revision, 1);
        let expected = if cfg!(target_os = "windows") { "Alt+Space" } else { "Control+Shift+Space" };
        assert_eq!(cfg.shortcut, expected);
        cfg.shortcut = "Control+Shift+Space".into();
        assert!(!cfg.upgrade(), "une seule fois : un choix fait ensuite est respecté");
        assert_eq!(cfg.shortcut, "Control+Shift+Space");
        assert_eq!(Config::default().shortcut, "Alt+Space");
    }

    #[test]
    fn patch_rejects_wrong_types() {
        let cfg = Config::default();
        assert!(cfg.patched(&json!({"liveMode": "oui"})).is_err());
        let next = cfg.patched(&json!({"liveMode": true, "maxMinutes": 20})).unwrap();
        assert!(next.live_mode);
        assert_eq!(next.max_recording_ms(), 20 * 60_000);
        let next = cfg.patched(&json!({"overlayPosition": {"fx": 0.2, "fy": 0.9}})).unwrap();
        assert_eq!(next.overlay_position, Some(Position { fx: 0.2, fy: 0.9 }));
        let next = next.patched(&json!({"overlayPosition": null})).unwrap();
        assert_eq!(next.overlay_position, None);
    }

    #[test]
    fn secrets_fall_back_to_private_file() {
        let dir = std::env::temp_dir().join(format!("souffle-secrets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = Secrets::new(&dir, false);
        s.set("groqKey", " gsk_123 ");
        assert_eq!(Secrets::new(&dir, false).get("groqKey"), "gsk_123");
        assert!(s.status()["groqKey"]);
        s.set("groqKey", "");
        assert_eq!(Secrets::new(&dir, false).get("groqKey"), "");
        assert!(!dir.join("cles.json").exists());
        s.set("notAKey", "x");
        assert_eq!(s.get("notAKey"), "");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
