//! The recording overlay (TUR-146): a small window that floats over other
//! apps while a meeting records, so the user can see it is live and pause or
//! stop it without switching back to meet-ai.
//!
//! It shows the timer (paused stretches left out), the last line or two of
//! the live transcript, and Pause or Resume and Stop; a click on the text
//! brings the main window forward ([`overlay_show_main`]). The webview half
//! is `src/ui/Overlay.tsx`; the window half is [`window`]: always on top,
//! undecorated, transparent with the OS blur where there is one, no taskbar
//! entry, on every Space or workspace, and remembered where it was dragged.
//!
//! It follows the recorder rather than being driven by it: every
//! [`RECORDING_STATE_EVENT`] (and a change of the setting) wakes one worker
//! thread, which reads the recorder's phase and `audio.show_recording_overlay`
//! and opens or closes the window to match ([`wanted`]). So every way a
//! recording starts or ends (the button, ⌘⇧R, the menu bar, a reminder, a
//! tick that failed) opens or closes it the same way, and nothing in the
//! recorder waits on a window. The worker, not the listener, does the window
//! work because the state event can be sent from the main thread (the quit
//! path), where building a window would deadlock.

use std::sync::Mutex;
use std::sync::mpsc::{self, Sender};

use tauri::{AppHandle, Listener as _, Manager as _};

use crate::config;
use crate::error::{UiError, on_blocking_pool};
use crate::events::RECORDING_STATE_EVENT;
use crate::folder_move::FolderGate;
use crate::lock::lock_or_recover;
use crate::recording::{Phase, Recorder};

mod placement;
mod platform;
mod position;
mod window;

/// The worker thread's name, beside `meet-ai-recording-ticker` in a sample.
const WORKER_THREAD_NAME: &str = "meet-ai-overlay";

/// Managed state: how to wake the worker.
pub struct Overlay(Mutex<Option<Sender<()>>>);

impl Default for Overlay {
    fn default() -> Self {
        Self(Mutex::new(None))
    }
}

impl Overlay {
    /// Ask the worker to bring the window in line. A worker that is gone
    /// (it never started) is logged once per wake, never an error.
    fn wake(&self) {
        let sent = lock_or_recover(&self.0)
            .as_ref()
            .is_some_and(|tx| tx.send(()).is_ok());
        if !sent {
            tracing::debug!("the overlay worker is not running");
        }
    }
}

/// Should the overlay be up? Only while recording (paused counts: Resume and
/// Stop are on it), and only when the setting is on. `enabled` is asked
/// only then, since it reads `config.jsonc`.
pub fn wanted(phase: Phase, enabled: impl FnOnce() -> bool) -> bool {
    phase == Phase::Recording && enabled()
}

/// Start following the recorder. Called once from `setup`.
pub fn init(app: &AppHandle) {
    let (tx, rx) = mpsc::channel::<()>();
    let handle = app.clone();
    let spawned = std::thread::Builder::new()
        .name(WORKER_THREAD_NAME.to_string())
        .spawn(move || {
            while rx.recv().is_ok() {
                // A burst of state events needs one look, not one each.
                while rx.try_recv().is_ok() {}
                reconcile(&handle);
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not start the overlay worker; no recording overlay");
        return;
    }
    *lock_or_recover(&app.state::<Overlay>().0) = Some(tx);
    let handle = app.clone();
    app.listen_any(RECORDING_STATE_EVENT, move |_event| {
        handle.state::<Overlay>().wake();
    });
}

/// Open or close the window so it matches the recorder and the setting.
fn reconcile(app: &AppHandle) {
    let phase = app.state::<Recorder>().status().phase;
    let open = window::is_open(app);
    let want = wanted(phase, config::show_recording_overlay);
    if want && !open {
        if let Err(error) = window::open(app) {
            tracing::warn!(%error, "could not show the recording overlay");
        }
    } else if !want && open {
        window::close(app);
    }
}

/// `audio.show_recording_overlay`, on unless `config.jsonc` turns it off.
#[tauri::command]
#[specta::specta]
pub async fn show_recording_overlay() -> Result<bool, UiError> {
    on_blocking_pool(config::show_recording_overlay).await
}

/// Save the setting and return it as saved. Writes under the meetings root,
/// so through the [`FolderGate`]. Takes effect at once: turned off mid-call,
/// the overlay closes; turned on, it opens.
#[tauri::command]
#[specta::specta]
pub async fn set_show_recording_overlay(app: AppHandle, on: bool) -> Result<bool, UiError> {
    let handle = app.clone();
    let saved = on_blocking_pool(move || {
        handle
            .state::<FolderGate>()
            .writing(|| Ok(config::set_show_recording_overlay(on)?))
    })
    .await??;
    app.state::<Overlay>().wake();
    Ok(saved)
}

/// The overlay's transcript text was clicked: bring the main window forward.
#[tauri::command]
#[specta::specta]
pub fn overlay_show_main(app: AppHandle) {
    crate::lifecycle::show_main_window(&app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_recording_with_the_setting_on_shows_the_overlay() {
        assert!(wanted(Phase::Recording, || true));
        assert!(!wanted(Phase::Recording, || false), "turned off");
        for phase in [Phase::Idle, Phase::Starting, Phase::Stopping] {
            assert!(
                !wanted(phase, || panic!("the setting is only read while recording")),
                "{phase:?}"
            );
        }
    }

    #[test]
    fn a_wake_with_no_worker_is_harmless() {
        Overlay::default().wake();
    }
}
