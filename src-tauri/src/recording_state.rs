//! How a finished or cut-short meeting is named in the UI (TUR-97).

use serde::Serialize;

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
}
