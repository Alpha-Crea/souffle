//! macOS : application au premier plan, focus, autorisations, bulle en verre.

use crate::engine::{MicPermission, Target};
use block2::RcBlock;
use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::{CFString, CFStringRef};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2::{msg_send, MainThreadMarker};
use objc2_app_kit::{
    NSEvent, NSEventMask, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindow,
    NSWindowCollectionBehavior, NSWindowOrderingMode, NSWorkspace,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> *mut c_void;
    fn AXUIElementSetAttributeValue(element: *mut c_void, attribute: CFStringRef, value: CFTypeRef) -> i32;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {
    static AVMediaTypeAudio: &'static NSString;
}

pub fn frontmost() -> Target {
    let ws = NSWorkspace::sharedWorkspace();
    let Some(app) = ws.frontmostApplication() else { return Target::default() };
    Target {
        name: app.localizedName().map(|n| n.to_string()).unwrap_or_default(),
        pid: app.processIdentifier() as i64,
        window: 0,
    }
}

/// Redonne le premier plan à l'application visée. Passe par l'Accessibilité,
/// comme « System Events » : depuis macOS 14, une application d'arrière-plan
/// ne peut plus en activer une autre par la voie ordinaire.
pub fn focus(t: &Target) -> bool {
    if t.pid <= 0 {
        return false;
    }
    unsafe {
        let el = AXUIElementCreateApplication(t.pid as i32);
        if el.is_null() {
            return false;
        }
        let attr = CFString::from_static_string("AXFrontmost");
        let err = AXUIElementSetAttributeValue(el, attr.as_concrete_TypeRef(), CFBoolean::true_value().as_CFTypeRef());
        CFRelease(el as CFTypeRef);
        err == 0
    }
}

pub fn accessibility_granted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Affiche la demande système d'Accessibilité, sans rien ouvrir d'autre.
pub fn ask_accessibility() {
    unsafe {
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let dict = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), CFBoolean::true_value().as_CFType())]);
        AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef() as *const c_void);
    }
}

/// Affiche la demande système d'Accessibilité, puis ouvre le bon volet des Réglages.
pub fn prompt_accessibility() {
    unsafe {
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let dict = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), CFBoolean::true_value().as_CFType())]);
        AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef() as *const c_void);
    }
    open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility");
}

fn capture_device_class() -> Option<&'static AnyClass> {
    AnyClass::get(c"AVCaptureDevice")
}

pub fn mic_permission() -> MicPermission {
    let Some(cls) = capture_device_class() else { return MicPermission::Granted };
    // 0 : pas encore demandé · 1 : restreint · 2 : refusé · 3 : accordé
    let status: isize = unsafe { msg_send![cls, authorizationStatusForMediaType: AVMediaTypeAudio] };
    match status {
        3 => MicPermission::Granted,
        1 | 2 => MicPermission::Denied,
        _ => MicPermission::Unknown,
    }
}

pub fn request_mic(done: Box<dyn FnOnce(bool) + Send>) {
    let Some(cls) = capture_device_class() else {
        done(true);
        return;
    };
    let done = Mutex::new(Some(done));
    let block = RcBlock::new(move |granted: Bool| {
        if let Some(f) = done.lock().ok().and_then(|mut d| d.take()) {
            f(granted.as_bool());
        }
    });
    unsafe {
        let _: () = msg_send![cls, requestAccessForMediaType: AVMediaTypeAudio, completionHandler: &*block];
    }
}

pub fn open_mic_settings() {
    open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone");
}

pub fn open_url(url: &str) {
    let _ = std::process::Command::new("open").arg(url).spawn();
}

/* ---------- Bulle ---------- */

const SCREEN_SAVER_LEVEL: isize = 1000;

/// Prépare la fenêtre de la bulle : au-dessus de tout, sur tous les bureaux
/// et par-dessus les applications en plein écran, sans ombre, avec un vrai
/// verre dessiné par macOS derrière la page. Le verre liquide de macOS 26
/// (NSGlassEffectView) s'il existe, le flou système sinon.
///
/// # Safety
/// `ns_window` doit être la NSWindow de la bulle ; appelé sur le fil principal.
pub unsafe fn setup_bubble(ns_window: *mut c_void, inset: f64, radius: f64) {
    let Some(window) = (ns_window as *const NSWindow).as_ref() else { return };
    window.setHasShadow(false);
    window.setLevel(SCREEN_SAVER_LEVEL);
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    let Some(content) = window.contentView() else { return };
    let b = content.bounds();
    let frame = NSRect::new(
        NSPoint::new(b.origin.x + inset, b.origin.y + inset),
        NSSize::new(b.size.width - 2.0 * inset, b.size.height - 2.0 * inset),
    );
    let Some(mtm) = MainThreadMarker::new() else { return };

    let glass: Option<Retained<NSView>> = AnyClass::get(c"NSGlassEffectView").and_then(|cls| {
        let obj: *mut AnyObject = msg_send![cls, alloc];
        let obj: *mut AnyObject = msg_send![obj, initWithFrame: frame];
        let view = Retained::from_raw(obj as *mut NSView)?;
        let _: () = msg_send![&*view, setCornerRadius: radius];
        Some(view)
    });
    let view = match glass {
        Some(v) => {
            crate::log!("bulle : verre liquide de macOS (NSGlassEffectView)");
            v
        }
        None => {
            crate::log!("bulle : flou système (NSVisualEffectView)");
            let v = NSVisualEffectView::initWithFrame(mtm.alloc(), frame);
            v.setMaterial(NSVisualEffectMaterial::HUDWindow);
            v.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
            // Toujours actif : Souffle n'est jamais l'application au premier plan.
            v.setState(NSVisualEffectState::Active);
            v.setWantsLayer(true);
            if let Some(layer) = v.layer() {
                let _: () = msg_send![&*layer, setCornerRadius: radius];
                let _: () = msg_send![&*layer, setMasksToBounds: true];
            }
            Retained::into_super(v)
        }
    };
    content.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, None);
}

/// Montre la bulle sans activer Souffle : le focus reste là où l'on écrit.
///
/// # Safety
/// `ns_window` doit être une NSWindow vivante ; appelé sur le fil principal.
pub unsafe fn order_front(ns_window: *mut c_void) {
    if let Some(window) = (ns_window as *const NSWindow).as_ref() {
        window.orderFrontRegardless();
    }
}

/* ---------- Échap pendant la dictée ---------- */

struct Monitors(Vec<Retained<AnyObject>>);
// Les moniteurs ne sont créés et retirés que sur le fil principal.
unsafe impl Send for Monitors {}

static MONITORS: Mutex<Option<Monitors>> = Mutex::new(None);
const ESCAPE: u16 = 53;

/// Écoute Échap sans l'intercepter : enregistrer une touche nue comme
/// raccourci global casserait le gestionnaire de raccourcis de macOS.
/// Demande l'Accessibilité, déjà nécessaire au collage. Fil principal.
pub fn escape_monitor(on: bool, on_escape: Arc<dyn Fn() + Send + Sync>) {
    let mut guard = MONITORS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(Monitors(old)) = guard.take() {
        for m in old {
            unsafe { NSEvent::removeMonitor(&m) };
        }
    }
    if !on {
        return;
    }
    let cb = on_escape.clone();
    let global = RcBlock::new(move |ev: NonNull<NSEvent>| {
        if unsafe { ev.as_ref() }.keyCode() == ESCAPE {
            crate::log!("Échap reçu (moniteur)");
            cb();
        }
    });
    let cb = on_escape;
    let local = RcBlock::new(move |ev: NonNull<NSEvent>| -> *mut NSEvent {
        if unsafe { ev.as_ref() }.keyCode() == ESCAPE {
            cb();
        }
        ev.as_ptr()
    });
    let mut list = vec![];
    if let Some(m) = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &global) {
        list.push(m);
    }
    if let Some(m) = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &local) } {
        list.push(m);
    }
    *guard = Some(Monitors(list));
}
