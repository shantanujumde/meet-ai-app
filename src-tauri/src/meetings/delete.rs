//! Deleting a meeting from its ⋯ menu (TUR-116).
//!
//! Delete never erases anything: the meeting folder goes to the system Trash
//! whole, transcript, notes, audio and the tasks in its own `tickets/` folder
//! with it, so the user can take it back out. Tickets in the shared
//! `<root>/tickets/` folder that name the meeting are not inside it and stay
//! where they are.
//!
//! Two refusals come before the Trash, both in [`delete_in`] so they are
//! tested on every OS:
//!
//! * **The meeting being recorded right now.** The recorder is writing into
//!   its folder; pulling it away would lose the recording.
//! * **An id that is not a plain folder name.** The id comes from the
//!   webview, so `..` or a path must never reach the Trash call.

use std::path::Path;

use tauri::{AppHandle, Manager as _};

use super::platform::move_to_trash;
use super::{Live, root};
use crate::agent_run::AgentRuns;
use crate::error::{UiError, on_blocking_pool};
use crate::folder_move::FolderGate;
use crate::recording::Recorder;
use crate::search;

/// Move one meeting's folder to the Trash.
///
/// Through the [`FolderGate`] like every other write under the root, so a
/// folder move cannot run at the same time. The recorder's state is read
/// inside it: while the gate is held for writing no move can start, and a
/// new recording always makes a new folder, never this one. The meeting's
/// notes run, if one is going, is cancelled first so its agent stops writing
/// into a folder that is about to go. The search index then drops the
/// meeting. The folder watcher tells the window the list changed, and the
/// window also reloads it itself.
#[tauri::command]
#[specta::specta]
pub async fn delete_meeting(app: AppHandle, id: String) -> Result<(), UiError> {
    on_blocking_pool(move || {
        app.state::<FolderGate>().writing(|| {
            let status = app.state::<Recorder>().status();
            delete_in(&root()?, &id, Live::from_status(&status), |dir| {
                if let Some(runs) = app.try_state::<AgentRuns>() {
                    runs.cancel(&id);
                }
                move_to_trash(dir)
            })
        })?;
        search::meeting_written(&app, &id);
        Ok(())
    })
    .await?
}

/// [`delete_meeting`] under `root`, with the meeting being recorded named by
/// `live` and the Trash call passed in. `trash` runs only once every check
/// has passed, and gets the meeting's folder.
pub(super) fn delete_in(
    root: &Path,
    id: &str,
    live: Live<'_>,
    trash: impl FnOnce(&Path) -> Result<(), UiError>,
) -> Result<(), UiError> {
    if live == Live::Meeting(id) {
        return Err(UiError::app(
            "recording-in-progress",
            "This meeting is recording. Stop the recording before deleting it.",
        ));
    }
    let dir = store::folder::meeting_dir(root, id)?;
    // The shared tickets folder sits beside the meetings and is a plain name
    // too. It is never a meeting, and trashing it would take every ticket.
    if id == store::TICKETS_DIR {
        return Err(UiError::app(
            "not-a-meeting",
            "That folder holds your tickets, not a meeting, so it is not deleted.",
        ));
    }
    if !dir.is_dir() {
        return Err(UiError::app(
            "meeting-not-found",
            format!("There is no meeting folder at {}.", dir.display()),
        ));
    }
    trash(&dir)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;
    use std::path::PathBuf;

    use super::*;

    const ID: &str = "2026-09-01-1430-standup";

    /// A meetings root with one meeting that has a transcript and a task of
    /// its own, and a shared ticket that names it.
    fn fixture() -> tempfile::TempDir {
        let root = tempfile::tempdir().expect("a temp folder");
        let meeting = root.path().join(ID);
        fs::create_dir_all(meeting.join(store::TICKETS_DIR)).expect("the meeting folder");
        fs::write(meeting.join("transcript.md"), "# Standup\n").expect("a transcript");
        fs::write(
            meeting.join(store::TICKETS_DIR).join("T-1.md"),
            "---\nid: T-1\n---\n",
        )
        .expect("a meeting task");
        let shared = root.path().join(store::TICKETS_DIR);
        fs::create_dir_all(&shared).expect("the shared tickets folder");
        fs::write(
            shared.join("T-2.md"),
            format!("---\nid: T-2\nmeeting: {ID}\n---\n"),
        )
        .expect("a shared ticket");
        root
    }

    /// Stands in for the Trash: records the folder and removes it, as the
    /// real one takes it out of the meetings root.
    fn fake_trash(seen: &RefCell<Vec<PathBuf>>) -> impl FnOnce(&Path) -> Result<(), UiError> + '_ {
        move |dir| {
            seen.borrow_mut().push(dir.to_path_buf());
            fs::remove_dir_all(dir)?;
            Ok(())
        }
    }

    #[test]
    fn the_whole_folder_goes_to_the_trash_and_shared_tickets_stay() {
        let root = fixture();
        let seen = RefCell::new(Vec::new());

        delete_in(root.path(), ID, Live::Nothing, fake_trash(&seen)).expect("deleted");

        assert_eq!(*seen.borrow(), vec![root.path().join(ID)]);
        assert!(!root.path().join(ID).exists());
        assert!(
            root.path()
                .join(store::TICKETS_DIR)
                .join("T-2.md")
                .is_file(),
            "a shared ticket that names the meeting is kept"
        );
    }

    #[test]
    fn the_meeting_being_recorded_is_refused_and_left_alone() {
        let root = fixture();
        let seen = RefCell::new(Vec::new());

        let error = delete_in(root.path(), ID, Live::Meeting(ID), fake_trash(&seen))
            .expect_err("refused while recording");

        assert_eq!(error.kind, "recording-in-progress");
        assert!(seen.borrow().is_empty(), "the Trash is never called");
        assert!(root.path().join(ID).join("transcript.md").is_file());
    }

    #[test]
    fn another_meeting_recording_does_not_block_this_one() {
        let root = fixture();
        let seen = RefCell::new(Vec::new());

        delete_in(
            root.path(),
            ID,
            Live::Meeting("2026-09-02-0900-other"),
            fake_trash(&seen),
        )
        .expect("deleted");

        assert_eq!(seen.borrow().len(), 1);
    }

    #[test]
    fn an_id_outside_the_meetings_folder_never_reaches_the_trash() {
        let root = fixture();
        for hostile in [
            "..",
            "../../etc",
            "/etc",
            "a/b",
            "",
            ".app",
            store::TICKETS_DIR,
        ] {
            let seen = RefCell::new(Vec::new());
            let result = delete_in(root.path(), hostile, Live::Nothing, fake_trash(&seen));
            assert!(result.is_err(), "{hostile:?} must be refused");
            assert!(seen.borrow().is_empty(), "{hostile:?} reached the Trash");
        }
        assert!(root.path().join(ID).is_dir());
        assert!(
            root.path().join(store::TICKETS_DIR).is_dir(),
            "the shared tickets folder is not a meeting"
        );
    }

    #[test]
    fn a_meeting_that_is_not_there_says_so() {
        let root = fixture();
        let seen = RefCell::new(Vec::new());

        let error = delete_in(
            root.path(),
            "2026-01-01-0000-gone",
            Live::Nothing,
            fake_trash(&seen),
        )
        .expect_err("nothing to delete");

        assert_eq!(error.kind, "meeting-not-found");
        assert!(seen.borrow().is_empty());
    }
}
