//! How a finished or cut-short meeting is named in the UI (TUR-97).

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::Serialize;

use crate::lock::lock_or_recover;

/// How a meeting's recording ended, as the UI names it (TUR-97).
///
/// **The name for a partly written meeting is "Interrupted".** Chosen over the
/// alternatives because it says what happened and nothing more:
///
/// * not *failed*, *corrupt* or *error* — the audio and transcript up to the
///   cut are good, and opening the meeting is not a failure;
/// * not *incomplete* or *partial* — those suggest the rest might still turn
///   up, or that the user should go and find it;
/// * not *recovered* — that implies a repair step the user took part in, and
///   TUR-97 says there is none;
/// * and not *finished*, because it is not one: the recording ended because
///   the app or the Mac stopped, not because someone pressed Stop.
///
/// The list shows the word as a label; the meeting itself says in one line
/// how much audio was kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum RecordingState {
    /// Stopped on purpose, or there is no audio to judge by (a folder from
    /// before recording existed, or one whose WAVs retention has deleted).
    Finished,
    /// The recording was cut short — force quit, `kill -9`, a crash, the
    /// battery. Opened exactly like any other meeting.
    Interrupted,
    /// This app is writing to it right now. Its files look the way an
    /// interrupted meeting's do — the headers are behind the samples and the
    /// last segment has not been closed — because it has not been stopped
    /// *yet*, so it must never be labelled interrupted.
    Recording,
    /// Stopped cleanly by a backup stop because the computer went to sleep
    /// or its lid closed (TUR-145). Finished in every other way: the files
    /// closed and it plays.
    StoppedForSleep,
    /// Stopped cleanly after ten minutes in which no one spoke and nobody
    /// pressed Keep recording (TUR-145).
    StoppedForSilence,
}

/// Which backup stop ended a recording (TUR-145).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupStop {
    /// The computer went to sleep, or its lid closed.
    Sleep,
    /// No one spoke for ten minutes, and nobody kept the recording going.
    Silence,
}

impl BackupStop {
    /// The word for the log.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sleep => "sleep",
            Self::Silence => "silence",
        }
    }
}

/// The meetings a backup stop ended since the app started, by folder name.
///
/// Kept in memory only, for as long as the app runs: that covers the case
/// that matters, the window after the wake, without a new key in the
/// meeting's files. After a restart such a meeting reads as finished, which
/// it is.
static BACKUP_STOPS: Mutex<BTreeMap<String, BackupStop>> = Mutex::new(BTreeMap::new());

/// Meeting `meeting_id` is being stopped by `why`. Called before the stop,
/// so the meeting list the stop's own state change refreshes already says so.
pub fn note_backup_stop(meeting_id: &str, why: BackupStop) {
    lock_or_recover(&BACKUP_STOPS).insert(meeting_id.to_owned(), why);
}

/// A new recording into `meeting_id` starts clean, even when a deleted
/// meeting once had the same folder name.
pub fn forget_backup_stop(meeting_id: &str) {
    lock_or_recover(&BACKUP_STOPS).remove(meeting_id);
}

impl RecordingState {
    /// The state of meeting `meeting_id`, whose files closed cleanly:
    /// finished, or which backup stop ended it. A meeting that did not close
    /// cleanly is interrupted whatever stopped it.
    pub fn ended_cleanly(meeting_id: &str) -> Self {
        match lock_or_recover(&BACKUP_STOPS).get(meeting_id) {
            None => Self::Finished,
            Some(BackupStop::Sleep) => Self::StoppedForSleep,
            Some(BackupStop::Silence) => Self::StoppedForSilence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_ending_names_the_backup_stop_that_made_it() {
        let (slept, silent, plain) = (
            "2026-10-08-0930-meeting-test-sleep",
            "2026-10-08-0930-meeting-test-silence",
            "2026-10-08-0930-meeting-test-plain",
        );
        note_backup_stop(slept, BackupStop::Sleep);
        note_backup_stop(silent, BackupStop::Silence);
        assert_eq!(
            RecordingState::ended_cleanly(slept),
            RecordingState::StoppedForSleep
        );
        assert_eq!(
            RecordingState::ended_cleanly(silent),
            RecordingState::StoppedForSilence
        );
        assert_eq!(
            RecordingState::ended_cleanly(plain),
            RecordingState::Finished
        );

        forget_backup_stop(slept);
        assert_eq!(
            RecordingState::ended_cleanly(slept),
            RecordingState::Finished
        );
        forget_backup_stop(silent);
    }

    #[test]
    fn the_wire_names_are_kebab_case() {
        for (state, wire) in [
            (RecordingState::StoppedForSleep, "stopped-for-sleep"),
            (RecordingState::StoppedForSilence, "stopped-for-silence"),
        ] {
            assert_eq!(serde_json::to_value(state).unwrap(), wire);
        }
    }
}
