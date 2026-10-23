//! Journal horodaté : dans le terminal et dans `souffle.log`, à côté des réglages.
//! C'est ce qui rend une panne lisible après coup, sans avoir lancé Souffle
//! depuis un terminal.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static FILE: Mutex<Option<File>> = Mutex::new(None);

/// Au-delà, le journal précédent est mis de côté : il ne grossit jamais sans fin.
const MAX_BYTES: u64 = 2 * 1024 * 1024;

pub fn init(dir: &Path) {
    let path = dir.join("souffle.log");
    if std::fs::metadata(&path).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let _ = std::fs::rename(&path, dir.join("souffle.ancien.log"));
    }
    if let Ok(f) = OpenOptions::new().create(true).append(true).open(&path) {
        *FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(f);
    }
}

pub fn stamp() -> String {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let day = ms % 86_400_000;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        day / 3_600_000,
        (day / 60_000) % 60,
        (day / 1000) % 60,
        day % 1000
    )
}

pub fn write(line: &str) {
    let line = format!("[souffle {} UTC] {}", stamp(), line);
    eprintln!("{line}");
    if let Some(f) = FILE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        let _ = writeln!(f, "{line}");
    }
}

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => { $crate::journal::write(&format!($($arg)*)) };
}
