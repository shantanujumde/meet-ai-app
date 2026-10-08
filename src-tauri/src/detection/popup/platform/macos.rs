//! macOS: the card window as a non-activating panel (TUR-147).
//!
//! A plain window cannot show over another app's full-screen Space, and
//! Tauri's show (`makeKeyAndOrderFront:`) makes it the key window, which
//! takes the keyboard from the call. An `NSPanel` with the
//! `NonactivatingPanel` style and `CanJoinAllSpaces | FullScreenAuxiliary`
//! can do both: it shows on every Space and over full-screen apps, and
//! clicking it does not activate meet-ai. It is shown with
//! `orderFrontRegardless`, which never makes it key.
//!
//! Tauri makes an `NSWindow` subclass of its own (tao's `TaoWindow`), so the
//! window is turned into a panel in place: its class is swapped for
//! [`CLASS_NAME`], an `NSPanel` subclass with the same instance layout (tao's
//! one `focusable` ivar, so tao's own reads of it still work). The swap only
//! happens when the two layouts are the same size; otherwise the card stays a
//! plain floating window and a warning is logged. Whether it shows over a
//! full-screen Zoom call is a manual check (docs/manual-checks).
//!
//! The panel can become key (so VoiceOver and, after a click, the keyboard
//! reach its buttons) but only does when a control needs it
//! (`becomesKeyOnlyIfNeeded`): a click on a button does not take the
//! keyboard.

// Adapted from github.com/ahkohd/tauri-nspanel/src/panel.rs @ ef6e3090a6083c653955ca0b6e7f6c6ae1453d40 (MIT OR Apache-2.0)
// The idea and the private `_setPreventsActivation:` call, rewritten for one
// panel: swap the window's class with `object_setClass`, add the
// non-activating style, and sync the window server's activation tag.

use std::ffi::CStr;
use std::sync::OnceLock;

use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, NSObjectProtocol as _, Sel};
use objc2::{ClassType as _, msg_send, sel};
use objc2_app_kit::{NSPanel, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask};
use tauri::WebviewWindow;

/// The panel class the card window becomes.
pub const CLASS_NAME: &CStr = c"MeetAiPromptPanel";

/// The ivar tao keeps on its window class for `set_focusable`.
const TAO_FOCUSABLE_IVAR: &CStr = c"focusable";

/// Where the panel shows: on every Space, beside a full-screen app, and not
/// in ⌘` window cycling.
pub fn behaviour() -> NSWindowCollectionBehavior {
    NSWindowCollectionBehavior::CanJoinAllSpaces
        | NSWindowCollectionBehavior::FullScreenAuxiliary
        | NSWindowCollectionBehavior::IgnoresCycle
}

/// Turn the card window into a non-activating panel, once, on the main
/// thread. A failure is logged and leaves a plain floating window.
pub fn make_panel(window: &WebviewWindow) {
    let target = window.clone();
    let queued = window.run_on_main_thread(move || match target.ns_window() {
        // SAFETY: `run_on_main_thread` runs this on the main thread, and the
        // pointer is this live window's `NSWindow`; `target` keeps it alive.
        Ok(ns_window) => match unsafe { become_panel(ns_window.cast()) } {
            Ok(()) => tracing::debug!("the prompt card is a non-activating panel"),
            Err(why) => tracing::warn!(why, "the prompt card stays a plain window"),
        },
        Err(error) => tracing::warn!(%error, "no NSWindow for the prompt card"),
    });
    if let Err(error) = queued {
        tracing::warn!(%error, "could not turn the prompt card into a panel");
    }
}

/// Show the card in front of every app without making it key or
/// activating meet-ai.
pub fn show(window: &WebviewWindow) -> tauri::Result<()> {
    window.set_always_on_top(true)?;
    let target = window.clone();
    window.run_on_main_thread(move || match target.ns_window() {
        Ok(ns_window) => {
            // SAFETY: on the main thread, and the pointer is this live
            // window's `NSWindow` (or the panel it became), kept alive by
            // `target`.
            if let Some(ns_window) = unsafe { ns_window.cast::<NSWindow>().as_ref() } {
                ns_window.orderFrontRegardless();
            }
        }
        Err(error) => tracing::warn!(%error, "no NSWindow to show the prompt card"),
    })
}

/// Swap `ns_window`'s class for the panel class and set it up as a
/// non-activating panel on every Space.
///
/// # Safety
///
/// On the main thread, with `ns_window` pointing at a live `NSWindow` that
/// nothing else is changing the class of.
unsafe fn become_panel(ns_window: *mut AnyObject) -> Result<(), &'static str> {
    // SAFETY: the caller passes a live object or null.
    let object = unsafe { ns_window.as_ref() }.ok_or("no NSWindow")?;
    let panel_class = panel_class().ok_or("could not make the panel class")?;
    let current = object.class();
    if !std::ptr::eq(current, panel_class) {
        if !same_layout(current, panel_class) {
            return Err("the window's layout is not a panel's");
        }
        // SAFETY: both classes are `NSWindow`s with the same instance size
        // (checked above) and the same extra ivar, and the panel class only
        // adds `NSPanel` behaviour and two `BOOL` getters. tauri-nspanel
        // swaps Tauri's windows the same way.
        unsafe { objc2::ffi::object_setClass(ns_window, panel_class) };
    }
    // SAFETY: the object is now an instance of an `NSPanel` subclass, and
    // we are on the main thread (the caller's contract).
    let panel = unsafe { &*ns_window.cast::<NSPanel>() };
    panel.setStyleMask(panel.styleMask() | NSWindowStyleMask::NonactivatingPanel);
    // Changing the class skips NSPanel's own set-up, and `setStyleMask:`
    // does not tell the window server the panel is non-activating; this
    // private call does, where it exists.
    let prevents_activation = sel!(_setPreventsActivation:);
    if panel.respondsToSelector(prevents_activation) {
        // SAFETY: `-[NSWindow _setPreventsActivation:(BOOL)]` takes one BOOL
        // and returns nothing; checked to exist just above.
        let () = unsafe { msg_send![panel, _setPreventsActivation: Bool::YES] };
    }
    panel.setCollectionBehavior(behaviour());
    // A panel hides when its app is not active by default; meet-ai is
    // almost never the active app when the card shows.
    panel.setHidesOnDeactivate(false);
    panel.setBecomesKeyOnlyIfNeeded(true);
    panel.setFloatingPanel(true);
    Ok(())
}

/// Can an object of class `from` become a `to` in place?
fn same_layout(from: &AnyClass, to: &AnyClass) -> bool {
    from.instance_size() == to.instance_size()
}

/// The `NSPanel` subclass the card becomes, made once.
pub fn panel_class() -> Option<&'static AnyClass> {
    static CLASS: OnceLock<Option<&'static AnyClass>> = OnceLock::new();
    *CLASS.get_or_init(|| {
        if let Some(existing) = AnyClass::get(CLASS_NAME) {
            return Some(existing);
        }
        let mut builder = ClassBuilder::new(CLASS_NAME, NSPanel::class())?;
        // tao's `TaoWindow` keeps one BOOL ivar; the panel keeps the same,
        // at the same place, so the layouts match and tao can still read it.
        builder.add_ivar::<Bool>(TAO_FOCUSABLE_IVAR);
        // SAFETY: both match `-[NSWindow canBecomeKeyWindow]` and
        // `canBecomeMainWindow`: no arguments, a BOOL back.
        unsafe {
            builder.add_method(
                sel!(canBecomeKeyWindow),
                can_become_key as extern "C-unwind" fn(_, _) -> _,
            );
            builder.add_method(
                sel!(canBecomeMainWindow),
                can_become_main as extern "C-unwind" fn(_, _) -> _,
            );
        }
        Some(builder.register())
    })
}

/// A borderless window cannot become key by default; the panel can, so
/// VoiceOver and the keyboard can reach its buttons.
extern "C-unwind" fn can_become_key(_this: &NSPanel, _cmd: Sel) -> Bool {
    Bool::YES
}

/// The card is never the main window: the main window stays meet-ai's own.
extern "C-unwind" fn can_become_main(_this: &NSPanel, _cmd: Sel) -> Bool {
    Bool::NO
}

#[cfg(test)]
mod tests {
    use objc2::runtime::{AnyClass, Bool, ClassBuilder};
    use objc2_app_kit::{NSPanel, NSWindow, NSWindowCollectionBehavior};

    use super::*;

    /// A stand-in for tao's `TaoWindow`: `NSWindow` plus one BOOL ivar,
    /// `focusable` (tao 0.35's `platform_impl/macos/window.rs`).
    fn tao_like_window() -> &'static AnyClass {
        let name = c"MeetAiTestTaoWindow";
        if let Some(existing) = AnyClass::get(name) {
            return existing;
        }
        let mut builder = ClassBuilder::new(name, NSWindow::class()).expect("the name is free");
        builder.add_ivar::<Bool>(c"focusable");
        builder.register()
    }

    #[test]
    fn the_panel_has_the_layout_of_taos_window_so_the_swap_happens() {
        let panel = panel_class().expect("the class registers");
        assert!(
            same_layout(tao_like_window(), panel),
            "tao window {} bytes, panel {} bytes",
            tao_like_window().instance_size(),
            panel.instance_size()
        );
        assert!(panel.superclass() == Some(NSPanel::class()));
        assert!(panel.instance_variable(c"focusable").is_some());
    }

    #[test]
    fn the_panel_can_become_key_but_never_main() {
        let panel = panel_class().expect("the class registers");
        assert!(panel.responds_to(sel!(canBecomeKeyWindow)));
        assert!(panel.responds_to(sel!(canBecomeMainWindow)));
        assert!(
            std::ptr::eq(panel_class().expect("again"), panel),
            "made once"
        );
    }

    #[test]
    fn the_panel_joins_every_space_and_full_screen_apps() {
        let behaviour = behaviour();
        assert!(behaviour.contains(NSWindowCollectionBehavior::CanJoinAllSpaces));
        assert!(behaviour.contains(NSWindowCollectionBehavior::FullScreenAuxiliary));
        assert!(behaviour.contains(NSWindowCollectionBehavior::IgnoresCycle));
    }
}
