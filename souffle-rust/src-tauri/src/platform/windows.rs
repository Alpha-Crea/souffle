//! Windows : application au premier plan, focus, bulle qui ne prend jamais le focus.

use crate::engine::{MicPermission, Target};
use std::ffi::c_void;
use std::os::windows::process::CommandExt;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow, SetForegroundWindow,
    SetWindowPos, ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_RESTORE, SW_SHOWNOACTIVATE,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn process_name(pid: u32) -> String {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        std::path::Path::new(&path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
    }
}

pub fn frontmost() -> Target {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return Target::default();
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        Target { name: process_name(pid), pid: pid as i64, window: hwnd.0 as isize }
    }
}

/// Redonne le focus à la fenêtre où l'on a commencé à parler. Windows refuse
/// SetForegroundWindow à une application d'arrière-plan : on se rattache le
/// temps de l'appel au fil de la fenêtre active, ce qui lève ce verrou.
pub fn focus(t: &Target) -> bool {
    if t.window == 0 {
        return false;
    }
    unsafe {
        let hwnd = HWND(t.window as *mut c_void);
        if !IsWindow(Some(hwnd)).as_bool() {
            return false;
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let fg = GetForegroundWindow();
        let fg_thread = GetWindowThreadProcessId(fg, None);
        let me = GetCurrentThreadId();
        let attached = fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, true).as_bool();
        let ok = SetForegroundWindow(hwnd).as_bool();
        let _ = BringWindowToTop(hwnd);
        if attached {
            let _ = AttachThreadInput(me, fg_thread, false);
        }
        ok
    }
}

pub fn accessibility_granted() -> bool {
    true
}

pub fn prompt_accessibility() {}

pub fn ask_accessibility() {}

pub fn mic_permission() -> MicPermission {
    // Windows ne demande rien à une application de bureau : si le micro est
    // coupé dans Confidentialité, la capture le dira.
    MicPermission::Granted
}

pub fn request_mic(done: Box<dyn FnOnce(bool) + Send>) {
    done(true);
}

pub fn open_mic_settings() {
    open_url("ms-settings:privacy-microphone");
}

pub fn open_url(url: &str) {
    let _ = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

/// Montre la bulle sans lui donner le focus, au-dessus de tout.
pub fn show_no_activate(hwnd: isize) {
    unsafe {
        let h = HWND(hwnd as *mut c_void);
        let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE);
    }
}
