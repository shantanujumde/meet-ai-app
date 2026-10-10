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

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use tauri::{AppHandle, Manager as _};

use crate::error::UiError;
use crate::recording::{Phase, Recorder, Status};

/// `Recorder::toggle`, through the gate. The one way every surface — the
/// button, ⌘⇧R, the menu bar — starts or stops a recording, so none of them can
/// slip a recording in underneath a folder move.
///
/// A refusal from the gate also goes onto the recorder's idle status
/// (`Recorder::refuse_start`): the button gets it back as this `Err`, but ⌘⇧R
/// and the menu bar have no caller that shows it, and the status is the only
/// way the window hears about a recorder refusal.
pub fn toggle_recording(app: &AppHandle) -> Result<Status, UiError> {
    let recorder = app.state::<Recorder>();
    gated_start(
        recorder.status().phase,
        &app.state::<FolderGate>(),
        || recorder.stop(app),
        || recorder.toggle(app),
        |refused| recorder.refuse_start(app, refused),
    )
}

/// A toggle takes the gate only when it will start (TUR-170). A recorder
/// that is not idle is stopped (a no-op mid-transition), ungated like the
/// Stop command: a move only runs while idle, so a stop has nothing to race,
/// and gating it refused ⌘⇧R's Stop while a move was only being checked.
/// `stop` never starts, so a recording that ended meanwhile stays ended.
fn gated_start<T>(
    phase: Phase,
    gate: &FolderGate,
    stop: impl FnOnce() -> Result<T, UiError>,
    toggle: impl FnOnce() -> Result<T, UiError>,
    refused: impl FnOnce(&UiError),
) -> Result<T, UiError> {
    if phase != Phase::Idle {
        return stop();
    }
    gated_toggle(gate, toggle, refused)
}

/// `Recorder::start`, through the gate: a **Record** for one meeting (the
/// menu bar's Today, a reminder or a prompt; TUR-77). `pinned` is the event
/// it was clicked for, which names the recording; it travels with this start
/// alone (TUR-169). Starts only from idle: a recording already going is left
/// alone rather than stopped, which a toggle would do, and the click hears
/// so ([`only_from_idle`]) instead of getting the running recording's status
/// back as if it had started.
pub fn start_recording(
    app: &AppHandle,
    pinned: Option<::calendar::Event>,
) -> Result<Status, UiError> {
    let recorder = app.state::<Recorder>();
    gated_toggle(
        &app.state::<FolderGate>(),
        || only_from_idle(recorder.status().phase, || recorder.start(app, pinned)),
        |refused| recorder.refuse_start(app, refused),
    )
}

/// Run `start` only when the recorder was idle (`before`), and turn "a
/// recording, or another start, got there first" into
/// [`already_recording`]. `Recorder::start` answers a start that lost the
/// race with the winner's status, still `Starting`; its own start ends
/// `Recording`.
fn only_from_idle(
    before: Phase,
    start: impl FnOnce() -> Result<Status, UiError>,
) -> Result<Status, UiError> {
    if before != Phase::Idle {
        return Err(already_recording());
    }
    let status = start()?;
    if status.phase == Phase::Recording {
        Ok(status)
    } else {
        Err(already_recording())
    }
}

/// A Record for one meeting while another recording runs (TUR-169).
pub fn already_recording() -> UiError {
    UiError::app(
        "already-recording",
        "A recording is already running. Stop it first to record this meeting.",
    )
}

/// [`toggle_recording`] without the `AppHandle`, so the refusal path is
/// testable: `toggle` runs only if the gate lets it, and `refused` hears
/// the gate's reason when it does not.
fn gated_toggle<T>(
    gate: &FolderGate,
    toggle: impl FnOnce() -> Result<T, UiError>,
    refused: impl FnOnce(&UiError),
) -> Result<T, UiError> {
    let _writing = gate.begin_write().inspect_err(refused)?;
    toggle()
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
                "meet-ai is still writing to your meetings folder: a recording starting or \
                 stopping, notes being saved, or a model downloading. Change the folder when \
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

/// Run `write` against the meetings root as it is *now*, holding the gate for
/// as long as it runs (TUR-17).
///
/// For work that writes into a meeting folder long after it started — a
/// notes run, a ticket sync — while the window stays free to move the
/// meetings folder in between. The root is looked up when the write starts,
/// not when the work did, so the result lands in the meeting's new place
/// rather than under a root the move has emptied. A meeting is its folder
/// name under the root, so `write` finds it from the root and the meeting id.
/// Refused while a move is running, like every other write.
pub fn writing_in_root<T, E: From<UiError>>(
    app: &AppHandle,
    write: impl FnOnce(&Path) -> Result<T, E>,
) -> Result<T, E> {
    let gate = app.try_state::<FolderGate>();
    writing_in(gate.as_deref(), crate::meetings::root, write)
}

/// [`writing_in_root`] without the `AppHandle`: `root` looks the root up,
/// and with no `gate` nothing is held.
pub fn writing_in<T, E: From<UiError>>(
    gate: Option<&FolderGate>,
    root: impl FnOnce() -> Result<PathBuf, UiError>,
    write: impl FnOnce(&Path) -> Result<T, E>,
) -> Result<T, E> {
    let _writing = gate.map(FolderGate::begin_write).transpose()?;
    write(&root()?)
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

    fn status(phase: Phase) -> Status {
        Status {
            phase,
            meeting_id: None,
            started_at_ms: None,
            error: None,
            pause: Box::default(),
        }
    }

    /// TUR-169: Record for meeting B while A records says so, rather than
    /// answering with A's status as if B had started.
    #[test]
    fn a_record_while_recording_is_told_so_and_starts_nothing() {
        for phase in [Phase::Starting, Phase::Recording, Phase::Stopping] {
            let refused = only_from_idle(phase, || unreachable!("must not start"));
            assert_eq!(refused.expect_err("refused").kind, "already-recording");
        }
        // Idle, but another start claimed the recorder first.
        let lost = only_from_idle(Phase::Idle, || Ok(status(Phase::Starting)));
        assert_eq!(lost.expect_err("lost the race").kind, "already-recording");
        // Its own start.
        let started = only_from_idle(Phase::Idle, || Ok(status(Phase::Recording)));
        assert_eq!(started.expect("started").phase, Phase::Recording);
        // A refusal of its own passes through.
        let failed = only_from_idle(Phase::Idle, || Err(UiError::app("mic-denied", "no")));
        assert_eq!(failed.expect_err("refused").kind, "mic-denied");
    }

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
    fn a_toggle_refused_mid_move_is_reported_and_never_runs() {
        let gate = FolderGate::default();
        let moving = gate.begin_move().unwrap();

        let mut toggled = false;
        let mut reported = None;
        let refused = gated_toggle(
            &gate,
            || {
                toggled = true;
                Ok(())
            },
            |error| reported = Some(error.kind),
        );
        assert_eq!(
            refused.expect_err("refused").kind,
            "folder-move-in-progress"
        );
        assert!(!toggled, "the recorder must not run during a move");
        assert_eq!(
            reported,
            Some("folder-move-in-progress"),
            "the refusal reaches the recorder's status, not just the caller"
        );

        drop(moving);
        let mut reported = false;
        assert!(gated_toggle(&gate, || Ok(()), |_| reported = true).is_ok());
        assert!(!reported, "nothing to report once the move is over");
    }

    /// TUR-170: a move holding the gate (even only while it checks the phase)
    /// must not turn ⌘⇧R's Stop away; only a start waits for the move.
    #[test]
    fn a_toggle_stop_is_never_gated_but_a_toggle_start_is() {
        let gate = FolderGate::default();
        let _moving = gate.begin_move().unwrap();

        for phase in [Phase::Starting, Phase::Recording, Phase::Stopping] {
            let mut reported = false;
            let stopped = gated_start(
                phase,
                &gate,
                || Ok("stopped"),
                || unreachable!("a non-idle toggle only stops"),
                |_| reported = true,
            );
            assert_eq!(stopped.expect("not refused"), "stopped", "{phase:?}");
            assert!(!reported, "{phase:?}: nothing refused");
        }

        let mut reported = None;
        let refused = gated_start(
            Phase::Idle,
            &gate,
            || unreachable!("an idle toggle starts"),
            || Ok("started"),
            |error| reported = Some(error.kind),
        );
        assert_eq!(
            refused.expect_err("refused").kind,
            "folder-move-in-progress"
        );
        assert_eq!(reported, Some("folder-move-in-progress"));
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
    fn a_late_write_looks_the_root_up_when_it_runs_and_holds_the_gate() {
        let gate = FolderGate::default();
        let looked_up = std::cell::Cell::new(false);
        let wrote: Result<PathBuf, UiError> = writing_in(
            Some(&gate),
            || {
                looked_up.set(true);
                Ok(PathBuf::from("/Moved/Meetings"))
            },
            |root| {
                assert!(gate.begin_move().is_err(), "no move under a write");
                Ok(root.to_path_buf())
            },
        );
        assert!(looked_up.get());
        assert_eq!(wrote.unwrap(), PathBuf::from("/Moved/Meetings"));
        assert!(gate.begin_move().is_ok(), "the gate opens after the write");
    }

    #[test]
    fn a_late_write_during_a_move_never_runs() {
        let gate = FolderGate::default();
        let _moving = gate.begin_move().unwrap();
        let refused: Result<(), UiError> = writing_in(
            Some(&gate),
            || panic!("no root lookup mid-move"),
            |_| panic!("no write mid-move"),
        );
        assert_eq!(refused.unwrap_err().kind, "folder-move-in-progress");
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
