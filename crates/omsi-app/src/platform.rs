//! What differs between a computer and a phone, in one place.
//!
//! A phone runs the launcher and the game in one process and one window (see
//! `android.rs`): ending a session goes back to the launcher instead of ending the program,
//! and the game is driven by fingers (see `touch.rs`).

use std::sync::atomic::{AtomicBool, Ordering};
use winit::event_loop::ActiveEventLoop;

/// Built for a phone or a tablet.
pub const MOBILE: bool = cfg!(target_os = "android");

/// The session asked to end (a phone: back to the launcher, the program runs on).
static LEAVE: AtomicBool = AtomicBool::new(false);

/// A computer whose launcher is the game's menu in one window (see `big_picture.rs`): a session
/// that ends goes back to it, as on a phone.
static BIG_PICTURE: AtomicBool = AtomicBool::new(false);

pub(crate) fn set_big_picture() {
    BIG_PICTURE.store(true, Ordering::Relaxed);
}

/// No "not responding" ghost over the window (Windows draws one, and offers to close the
/// program, when the window has not looked at its messages for five seconds: a drive being
/// made or written in the one window, a loading screen on it).
pub(crate) fn no_ghosting() {
    #[cfg(windows)]
    // SAFETY: a plain call without arguments, for this process
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::DisableProcessWindowsGhosting();
    }
}

/// The launcher is the game's menu in this window (a session ends back in it).
pub(crate) fn big_picture() -> bool {
    BIG_PICTURE.load(Ordering::Relaxed)
}

/// In the one window: the game menu's "Quit openOMSI" - the session ends, then the program.
static QUIT_ALL: AtomicBool = AtomicBool::new(false);

pub(crate) fn request_quit_all() {
    QUIT_ALL.store(true, Ordering::Relaxed);
}

#[allow(dead_code)]
pub(crate) fn take_quit_all() -> bool {
    QUIT_ALL.swap(false, Ordering::Relaxed)
}

/// End the session: the program on a computer, back to the launcher on a phone (and in the
/// one window of `big_picture`).
pub(crate) fn exit(event_loop: &ActiveEventLoop) {
    if MOBILE || BIG_PICTURE.load(Ordering::Relaxed) {
        LEAVE.store(true, Ordering::Relaxed);
    } else {
        event_loop.exit();
    }
}

/// Whether the session asked to end since the last call.
#[allow(dead_code)]
pub(crate) fn take_leave() -> bool {
    LEAVE.swap(false, Ordering::Relaxed)
}

/// The on-screen controls: always on a phone; `OMSI_TOUCH=1` shows them on a computer
/// (driven by `touch` commands of `OMSI_INPUT`, or the mouse as a finger).
pub(crate) fn touch_controls() -> bool {
    MOBILE || omsi_cfg::flags::OMSI_TOUCH.is_set()
}

/// The phone's tilt as a steering wheel's turn (-1 left .. 1 right), when the tilt sensor
/// is on and there is one.
pub(crate) fn tilt_steering() -> Option<f32> {
    #[cfg(target_os = "android")]
    {
        crate::android::tilt()
    }
    #[cfg(not(target_os = "android"))]
    {
        None
    }
}

/// Switch the tilt sensor on or off (it costs battery while on).
pub(crate) fn set_tilt(on: bool) {
    #[cfg(target_os = "android")]
    crate::android::set_tilt(on);
    #[cfg(not(target_os = "android"))]
    let _ = on;
}

/// A short buzz of the phone (a button that did something the eye may miss).
pub(crate) fn buzz(ms: u32) {
    #[cfg(target_os = "android")]
    crate::android::vibrate(ms);
    #[cfg(not(target_os = "android"))]
    let _ = ms;
}

/// Show a saved file in the system's file browser, selected (Finder, Explorer; the folder
/// on Linux). Nothing on a phone: its photos are in the gallery (`to_gallery`).
pub(crate) fn reveal(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
    let _ = std::process::Command::new("xdg-open").arg(path.parent().unwrap_or(path)).spawn();
    #[cfg(target_os = "android")]
    let _ = path;
}

/// A picture just saved shown in the phone's gallery at once (the media scanner would find
/// it only some time later). Nothing on a computer.
pub(crate) fn to_gallery(path: &std::path::Path) {
    #[cfg(target_os = "android")]
    crate::android::scan_media(path);
    #[cfg(not(target_os = "android"))]
    let _ = path;
}
