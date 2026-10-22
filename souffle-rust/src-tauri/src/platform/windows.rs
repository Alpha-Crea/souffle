//! Windows : application au premier plan, focus, bulle qui ne prend jamais le focus.

use crate::engine::{MicPermission, Target};
use std::ffi::c_void;
use std::os::windows::process::CommandExt;
use std::sync::Mutex;
use windows::core::{Interface, PWSTR};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, CUIAutomation8, IUIAutomation, IUIAutomation2, IUIAutomationElement,
};
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow, SetForegroundWindow,
    SetWindowPos, ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
    SW_HIDE, SW_RESTORE,
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

/// Montre la bulle sans lui donner le focus, au-dessus de tout, à sa place.
pub fn show_at(hwnd: isize, frame: Option<(i32, i32, i32, i32)>) {
    unsafe {
        let h = HWND(hwnd as *mut c_void);
        let mut flags = SWP_NOACTIVATE | SWP_SHOWWINDOW;
        let (x, y, w, hh) = frame.unwrap_or_default();
        if frame.is_none() {
            flags |= SWP_NOMOVE | SWP_NOSIZE;
        }
        let _ = SetWindowPos(h, Some(HWND_TOPMOST), x, y, w, hh, flags);
    }
}

pub fn hide(hwnd: isize) {
    unsafe {
        let _ = ShowWindow(HWND(hwnd as *mut c_void), SW_HIDE);
    }
}

pub fn move_to(hwnd: isize, x: i32, y: i32) {
    unsafe {
        let flags = SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER;
        let _ = SetWindowPos(HWND(hwnd as *mut c_void), None, x, y, 0, 0, flags);
    }
}

/* ---------- Le champ où l'on écrivait ---------- */

/// Champ de saisie qui avait le focus au début de la dictée, et sa fenêtre.
/// Les objets d'automatisation sont utilisés depuis des fils « multithread »
/// de COM uniquement : ils y passent d'un fil à l'autre sans risque.
struct Field {
    window: isize,
    element: IUIAutomationElement,
}
unsafe impl Send for Field {}

static FIELD: Mutex<Option<Field>> = Mutex::new(None);

fn automation() -> Option<IUIAutomation> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        if let Ok(a) = CoCreateInstance::<_, IUIAutomation2>(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) {
            // Une application figée ne doit pas retenir Souffle plus d'une seconde.
            let _ = a.SetConnectionTimeout(1000);
            let _ = a.SetTransactionTimeout(1000);
            return a.cast().ok();
        }
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()
    }
}

/// Note le champ qui a le focus dans la fenêtre visée. À appeler hors du fil
/// principal : le navigateur peut mettre un instant à répondre.
pub fn remember_field(t: &Target) {
    let element = automation().and_then(|a| unsafe { a.GetFocusedElement() }.ok());
    let mut slot = FIELD.lock().unwrap_or_else(|e| e.into_inner());
    *slot = element.map(|element| Field { window: t.window, element });
}

/// Si l'on a cliqué ailleurs dans la même fenêtre pendant la dictée, rend le
/// focus au champ du départ, pourvu qu'il soit encore là et visible.
pub fn restore_field(t: &Target) -> bool {
    let slot = FIELD.lock().unwrap_or_else(|e| e.into_inner());
    let Some(field) = slot.as_ref().filter(|f| f.window == t.window && t.window != 0) else { return false };
    let Some(a) = automation() else { return false };
    unsafe {
        let Ok(now) = a.GetFocusedElement() else { return false };
        if a.CompareElements(&field.element, &now).map(|b| b.as_bool()).unwrap_or(true) {
            return false;
        }
        let visible = field.element.CurrentIsOffscreen().map(|b| !b.as_bool()).unwrap_or(false);
        let enabled = field.element.CurrentIsEnabled().map(|b| b.as_bool()).unwrap_or(false);
        visible && enabled && field.element.SetFocus().is_ok()
    }
}
