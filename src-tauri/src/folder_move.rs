//! Keeping a recording from starting while the meetings folder moves.
//!
//! `change_meetings_folder` runs on the blocking pool, so the window stays
//! responsive for the whole move — minutes, when it has to copy across
//! volumes. Responsive means ⌘⇧R, the menu-bar item and the Record button all
//! still work, and a recording started mid-move writes under the *old* root,
//! which `move_contents` then deletes with `remove_dir_all`. Checking
//! `Phase::Idle` once before the move does not help: the phase can change the
//! moment after the check.
//!
//! [`FolderGate`] closes that gap without touching the recorder itself. Every
//! toggle holds a [`ToggleGuard`] for as long as `Recorder::toggle` runs, and a
//! move holds a [`MoveGuard`] for as long as the move runs. The two exclude each
//! other: a toggle during a move is refused with `folder-move-in-progress`, and
//! a move while a toggle is still starting or stopping is refused with
//! `recording-in-progress`. Toggles do not exclude each other — a double-tapped
//! shortcut is still the recorder's own phase mutex's job, and must stay the
//! quiet no-op it is rather than become an error.
//!
//! Both guards release on drop, so a panicking move or toggle cannot leave the
//! gate shut until the app is restarted.

use std::sync::{Mutex, MutexGuard};

use tauri::{AppHandle, Manager as _};

use crate::error::UiError;
use crate::recording::{Recorder, Status};

/// `Recorder::toggle`, through the gate. The one way every surface — the
/// button, ⌘⇧R, the menu bar — starts or stops a recording, so none of them can
/// slip a recording in underneath a folder move.
pub fn toggle_recording(app: &AppHandle) -> Result<Status, UiError> {
    let gate = app.state::<FolderGate>();
    let _toggle = gate.begin_toggle()?;
    app.state::<Recorder>().toggle(app)
}

/// Managed state: who is using the meetings root right now.
#[derive(Default)]
pub struct FolderGate {
    state: Mutex<GateState>,
}

#[derive(Default)]
struct GateState {
    /// Toggles currently inside `Recorder::toggle`.
    toggles: usize,
    moving: bool,
}

impl FolderGate {
    /// Enter a start/stop toggle, or learn that the folder is being moved.
    pub fn begin_toggle(&self) -> Result<ToggleGuard<'_>, UiError> {
        let mut state = self.lock();
        if state.moving {
            return Err(UiError::app(
                "folder-move-in-progress",
                "meet-ai is moving your meetings folder. Start the recording once the move \
                 finishes.",
            ));
        }
        state.toggles += 1;
        Ok(ToggleGuard { gate: self })
    }

    /// Enter a folder move, or learn that a recording is starting or stopping
    /// (or another move is already running).
    ///
    /// The caller still checks `Phase::Idle` after this succeeds: the gate only
    /// says no toggle is *in flight*, not that nothing is recording.
    pub fn begin_move(&self) -> Result<MoveGuard<'_>, UiError> {
        let mut state = self.lock();
        if state.moving {
            return Err(UiError::app(
                "folder-move-in-progress",
                "meet-ai is already moving your meetings folder.",
            ));
        }
        if state.toggles > 0 {
            return Err(UiError::app(
                "recording-in-progress",
                "A recording is starting or stopping. Wait for it, then change the meetings \
                 folder.",
            ));
        }
        state.moving = true;
        Ok(MoveGuard { gate: self })
    }

    /// Poison is recovered rather than propagated: the state is two plain
    /// fields that every holder leaves consistent, so a panic elsewhere under
    /// the lock is no reason to refuse every later recording.
    fn lock(&self) -> MutexGuard<'_, GateState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Held for the length of one `Recorder::toggle`.
#[must_use = "the toggle leaves the gate as soon as this is dropped"]
pub struct ToggleGuard<'a> {
    gate: &'a FolderGate,
}

impl Drop for ToggleGuard<'_> {
    fn drop(&mut self) {
        let mut state = self.gate.lock();
        state.toggles = state.toggles.saturating_sub(1);
    }
}

/// Held for the length of one meetings-folder move.
#[must_use = "the move leaves the gate as soon as this is dropped"]
pub struct MoveGuard<'a> {
    gate: &'a FolderGate,
}

impl Drop for MoveGuard<'_> {
    fn drop(&mut self) {
        self.gate.lock().moving = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recording_cannot_start_while_the_folder_is_moving() {
        let gate = FolderGate::default();
        let moving = gate.begin_move().expect("nothing else is using the root");

        let refused = gate
            .begin_toggle()
            .err()
            .expect("a toggle mid-move is refused");
        assert_eq!(refused.kind, "folder-move-in-progress");

        drop(moving);
        assert!(
            gate.begin_toggle().is_ok(),
            "once the move ends, recording works again"
        );
    }

    #[test]
    fn the_folder_cannot_move_while_a_toggle_is_in_flight() {
        let gate = FolderGate::default();
        let first = gate.begin_toggle().unwrap();
        // A double-tap: the second toggle is the recorder's no-op, not an error.
        let second = gate
            .begin_toggle()
            .expect("toggles do not exclude each other");

        assert_eq!(
            gate.begin_move().err().expect("refused").kind,
            "recording-in-progress"
        );
        drop(first);
        assert!(gate.begin_move().is_err(), "one toggle is still in flight");
        drop(second);
        assert!(gate.begin_move().is_ok());
    }

    #[test]
    fn only_one_move_at_a_time() {
        let gate = FolderGate::default();
        let _moving = gate.begin_move().unwrap();
        assert_eq!(
            gate.begin_move().err().expect("refused").kind,
            "folder-move-in-progress"
        );
    }

    #[test]
    fn a_move_that_panics_reopens_the_gate() {
        let gate = FolderGate::default();
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _moving = gate.begin_move().unwrap();
            panic!("the copy blew up");
        }));
        assert!(unwound.is_err());
        assert!(
            gate.begin_toggle().is_ok(),
            "a crashed move must not block recording until a restart"
        );
    }
}
