//! Ce qui dépend du système. Chaque plateforme expose les mêmes fonctions.

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "windows")]
pub use self::windows::*;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod other {
    //! Linux et autres : de quoi compiler et tester, sans plus.
    use crate::engine::{MicPermission, Target};

    pub fn frontmost() -> Target {
        Target::default()
    }
    pub fn focus(_: &Target) -> bool {
        false
    }
    pub fn remember_field(_: &Target) {}
    pub fn restore_field(_: &Target) -> bool {
        false
    }
    pub fn accessibility_granted() -> bool {
        true
    }
    pub fn prompt_accessibility() {}
    pub fn ask_accessibility() {}
    pub fn mic_permission() -> MicPermission {
        MicPermission::Granted
    }
    pub fn request_mic(done: Box<dyn FnOnce(bool) + Send>) {
        done(true);
    }
    pub fn open_mic_settings() {}
    pub fn open_url(url: &str) {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub use other::*;
