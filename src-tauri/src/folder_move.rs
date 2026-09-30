//! Keeping anything from writing under the meetings root while it moves.
//!
//! `change_meetings_folder` runs on the blocking pool, so the window stays
//! responsive for the whole move — minutes, when it has to copy across
//! volumes. Responsive means everything else still works: ⌘⇧R, the menu-bar
//! item and the Record button; saving notes; finishing or resetting
//! onboarding; downloading a model. Each of those writes under the *old* root,
//! and anything written there after the copy has passed it is deleted a moment
//! later by `move_contents`' `remove_dir_all`. Checking `Phase::Idle` once
//! before the move covers none of the non-recording writers, and not even the
//! recorder for long: the phase can change the moment after the check.
//!
//! [`FolderGate`] closes that gap without touching the recorder or the
//! meetings module. Every writer holds a [`WriteGuard`] for as long as its
//! write runs, and a move holds a [`MoveGuard`] for as long as the move runs.
//! The two exclude each other: a write during a move is refused with
//! `folder-move-in-progress`, and a move while any write is in flight is
//! refused with `folder-busy`. Writers do not exclude each other — a
//! double-tapped shortcut is still the recorder's own phase mutex's job and
//! must stay the quiet no-op it is, and notes saved while a model downloads
//! touch different files.
//!
//! Both guards release on drop, so a panicking move or write cannot leave the
//! gate shut until the app is restarted.

use std::sync::{Mutex, MutexGuard};

use tauri::{AppHandle, Manager as _};

use crate::error::UiError;
use crate::recording::{Recorder, Status};

/// `Recorder::toggle`, through the gate. The one way every surface — the
/// button, ⌘⇧R, the menu bar — starts or stops a recording, so none of them can
/// slip a recording in underneath a folder move.
pub fn toggle_recording(app: &AppHandle) -> Result<Status, UiError> {
    app.state::<FolderGate>()
        .writing(|| app.state::<Recorder>().toggle(app))
}

/// Managed state: who is using the meetings root right now.
#[derive(Default)]
pub struct FolderGate {
    state: Mutex<GateState>,
}

#[derive(Default)]
struct GateState {
    /// Writes currently in flight under the root.
    writers: usize,
    moving: bool,
}

impl FolderGate {
    /// Run `work`, which writes under the meetings root, holding the gate open
    /// for writes until it returns. Refused, without running `work`, while the
    /// folder is moving.
    pub fn writing<T>(&self, work: impl FnOnce() -> Result<T, UiError>) -> Result<T, UiError> {
        let _writing = self.begin_write()?;
        work()
    }

    /// Enter a write under the meetings root, or learn that the folder is being
    /// moved. For a writer that cannot be wrapped in [`FolderGate::writing`] —
    /// the model download, whose guard has to live on the thread doing the
    /// writing.
    pub fn begin_write(&self) -> Result<WriteGuard<'_>, UiError> {
        let mut state = self.lock();
        if state.moving {
            return Err(moving());
        }
        state.writers += 1;
        Ok(WriteGuard { gate: self })
    }

    /// Enter a folder move, or learn that something is still writing under the
    /// root (or another move is already running).
    ///
    /// The caller still checks `Phase::Idle` after this succeeds: the gate only
    /// says no write is *in flight*, not that nothing is recording.
    pub fn begin_move(&self) -> Result<MoveGuard<'_>, UiError> {
        let mut state = self.lock();
        if state.moving {
            return Err(moving());
        }
        if state.writers > 0 {
            return Err(UiError::app(
                "folder-busy",
                "meet-ai is still writing to your meetings folder — a recording starting or \
                 stopping, notes being saved, or a model downloading. Change the folder once \
                 that finishes.",
            ));
        }
        state.moving = true;
        Ok(MoveGuard { gate: self })
    }

    /// Poison is recovered rather than propagated: the state is two plain
    /// fields that every holder leaves consistent, so a panic elsewhere under
    /// the lock is no reason to refuse every later write.
    fn lock(&self) -> MutexGuard<'_, GateState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The one refusal every blocked caller gets — a write, a second move — so the
/// wording is about the move, not about whichever action bumped into it.
fn moving() -> UiError {
    UiError::app(
        "folder-move-in-progress",
        "meet-ai is moving your meetings folder. Try again when it finishes.",
    )
}

/// Held for the length of one write under the meetings root.
#[must_use = "the write leaves the gate as soon as this is dropped"]
pub struct WriteGuard<'a> {
    gate: &'a FolderGate,
}

impl Drop for WriteGuard<'_> {
    fn drop(&mut self) {
        let mut state = self.gate.lock();
        state.writers = state.writers.saturating_sub(1);
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
            .begin_write()
            .err()
            .expect("a toggle mid-move is refused");
        assert_eq!(refused.kind, "folder-move-in-progress");

        drop(moving);
        assert!(
            gate.begin_write().is_ok(),
            "once the move ends, recording works again"
        );
    }

    #[test]
    fn notes_are_not_saved_while_the_folder_is_moving() {
        // `save_notes` is `writing(|| meetings::write_notes(..))`. A refusal
        // must mean the write never ran, not that it ran and reported failure —
        // a note written under the old root mid-copy is the one that gets lost.
        let gate = FolderGate::default();
        let _moving = gate.begin_move().unwrap();

        let mut wrote = false;
        let refused = gate.writing(|| {
            wrote = true;
            Ok(())
        });
        assert_eq!(
            refused.expect_err("refused").kind,
            "folder-move-in-progress"
        );
        assert!(!wrote, "the notes write must not run during a move");
    }

    #[test]
    fn the_folder_cannot_move_while_a_download_holds_the_gate() {
        // `engine::download` takes a write guard on its blocking thread and
        // keeps it until the thread ends.
        let gate = FolderGate::default();
        let downloading = gate.begin_write().unwrap();

        assert_eq!(
            gate.begin_move().err().expect("refused").kind,
            "folder-busy"
        );
        drop(downloading);
        assert!(gate.begin_move().is_ok());
    }

    #[test]
    fn writers_do_not_exclude_each_other_but_all_block_a_move() {
        let gate = FolderGate::default();
        let first = gate.begin_write().unwrap();
        // A double-tap: the second toggle is the recorder's no-op, not an error.
        let second = gate
            .begin_write()
            .expect("writers do not exclude each other");

        drop(first);
        assert!(gate.begin_move().is_err(), "one write is still in flight");
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
            gate.begin_write().is_ok(),
            "a crashed move must not block writes until a restart"
        );
    }

    #[test]
    fn a_write_that_panics_does_not_block_moves_forever() {
        let gate = FolderGate::default();
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _: Result<(), UiError> = gate.writing(|| panic!("the write blew up"));
        }));
        assert!(unwound.is_err());
        assert!(gate.begin_move().is_ok());
    }
}
