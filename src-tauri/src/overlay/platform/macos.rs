//! The overlay's macOS window behaviour that Tauri's builder has no option
//! for (TUR-146).

use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
use tauri::WebviewWindow;

/// Let the overlay join every Space, a full-screen app's included
/// (`FullScreenAuxiliary`; `visible_on_all_workspaces` sets only
/// `CanJoinAllSpaces`), and keep it out of the ⌘` window cycle
/// (`IgnoresCycle`). Whether macOS then draws it over another app's
/// full-screen Space is a manual check (docs/manual-checks/worktree-tur146.md).
pub fn float_over_full_screen(window: &WebviewWindow) {
    let ns_window = match window.ns_window() {
        Ok(pointer) => pointer as usize,
        Err(error) => {
            tracing::debug!(%error, "no NSWindow behind the overlay; leaving its Spaces alone");
            return;
        }
    };
    let queued = window.run_on_main_thread(move || {
        // SAFETY: `ns_window` is the live NSWindow Tauri keeps behind the
        // overlay (it is only closed from the same main thread, later), and
        // AppKit is touched here on the main thread, as it must be.
        let ns_window = unsafe { &*(ns_window as *const NSWindow) };
        let behavior = ns_window.collectionBehavior()
            | NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle;
        ns_window.setCollectionBehavior(behavior);
    });
    if let Err(error) = queued {
        tracing::debug!(%error, "could not set the overlay's Spaces behaviour");
    }
}
