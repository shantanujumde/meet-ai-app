//! The review view: one meeting opened as a list row or in full, saving its
//! notes, and renaming it. Turns a loaded [`store::folder::MeetingFolder`] into the shapes
//! the webview renders.

use std::path::{Path, PathBuf};

use audio::segments::duration_ms;
use audio::wav_repair::classify_audio;
use serde::Serialize;
use store::folder_name::split_folder_name;

use super::list::{Live, live_id};
use super::root::root;
use crate::error::UiError;
use crate::recording_state::RecordingState;

/// Files inside a meeting folder (SPEC §3.1).
const AUDIO: &str = meeting_format::layout::AUDIO_DIR;

/// One row in the meeting list.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MeetingSummary {
    /// The folder name, e.g. `2026-09-01-1430-standup`. Also the route param.
    pub id: String,
    /// What to show in the list.
    pub title: String,
    /// Who was invited, from `meeting.md`'s `attendees` (filled from the
    /// calendar event, TUR-29). Empty when it names nobody.
    pub attendees: Vec<String>,
    /// `YYYY-MM-DD`, parsed from the folder name. `None` if it does not match.
    pub date: Option<String>,
    /// `HH:MM`, parsed from the folder name.
    pub time: Option<String>,
    /// How many §3.4 lines parsed. 0 is a real, displayable answer.
    #[specta(type = specta_typescript::Number)]
    pub line_count: usize,
    /// The timestamp on the last parsed line — the meeting's readable length.
    pub last_timestamp: Option<String>,
    /// `notes.md` exists and has something in it.
    pub has_notes: bool,
    /// The meeting has been wrapped up: `meeting.md` exists and its notes are
    /// written (`analyzed_by` is set, or one of the four sections has text).
    /// A `meeting.md` that holds only the notes switch does not count.
    pub has_analysis: bool,
    /// `meeting.md` says `agent_notes: off`: the user switched notes off for
    /// this meeting (SPEC A11, TUR-12), so no notes run sends it.
    pub notes_off: bool,
    /// Whether the recording ended on purpose. See [`RecordingState`].
    pub recording_state: RecordingState,
    /// Milliseconds of audio a player can actually reach: the longer of the
    /// two tracks, measured from its WAV header (SPEC A5 — header frames are
    /// the only true duration). `None` when the folder holds no audio at all,
    /// which is also what a meeting looks like after the retention job (L16)
    /// has deleted its WAVs.
    #[specta(type = Option<specta_typescript::Number>)]
    pub audio_ms: Option<u64>,
}

/// One parsed transcript line.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptLine {
    /// 0-based line number within `transcript.md`, per the Phase 2a contract.
    #[specta(type = specta_typescript::Number)]
    pub seq: usize,
    /// `HH:MM:SS`, the utterance *start* (SPEC §3.4).
    pub time: String,
    /// `You` or `Others` (L5). Kept as the literal spec string.
    pub speaker: SpeakerLabel,
    pub text: String,
}

/// A line's speaker as `transcript.md` writes it (L5): `You` or `Others`,
/// capitalised, unlike the live pane's lower-case `meeting_format::Speaker`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
pub enum SpeakerLabel {
    You,
    Others,
}

impl From<meeting_format::Speaker> for SpeakerLabel {
    fn from(speaker: meeting_format::Speaker) -> Self {
        match speaker {
            meeting_format::Speaker::You => Self::You,
            meeting_format::Speaker::Others => Self::Others,
        }
    }
}

/// A finished meeting, opened for review.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MeetingDetail {
    pub summary: MeetingSummary,
    /// Absolute path, so "Reveal in Finder" and the error copy can name it.
    pub path: String,
    pub lines: Vec<TranscriptLine>,
    /// `transcript.md` is missing entirely — a different state from "empty".
    pub transcript_missing: bool,
    /// Lines that did not match §3.4 and were skipped. Surfaced, not hidden.
    #[specta(type = specta_typescript::Number)]
    pub unparsed_line_count: usize,
    pub notes: String,
}

/// Open one meeting: its transcript and its notes.
pub fn detail(id: &str, live: Live<'_>) -> Result<MeetingDetail, UiError> {
    let dir = existing_meeting_dir(id)?;
    let folder = store::folder::load(&dir)?;

    let (lines, unparsed_line_count) = match &folder.transcript {
        Some(transcript) => (
            transcript
                .lines
                .iter()
                .map(|line| TranscriptLine {
                    seq: line.seq,
                    time: line.time.clone(),
                    speaker: line.speaker.into(),
                    text: line.text.clone(),
                })
                .collect(),
            unparsed_lines(transcript),
        ),
        None => (Vec::new(), 0),
    };

    let is_live = live_id(live).as_deref() == Some(folder.id.as_str());

    Ok(MeetingDetail {
        summary: summarize(&store::folder::FolderSummary::from(&folder), is_live),
        path: dir.display().to_string(),
        lines,
        transcript_missing: folder.transcript.is_none(),
        unparsed_line_count,
        notes: folder.notes,
    })
}

/// Save the user's notes for one meeting.
///
/// `store` writes through a temp file and `rename(2)`, so a crash mid-save
/// cannot leave a half-written `notes.md`. The notes pane autosaves while the
/// user types, so that matters.
pub fn write_notes(id: &str, body: &str) -> Result<(), UiError> {
    let dir = existing_meeting_dir(id)?;
    store::notes::write(&dir, body)?;
    Ok(())
}

/// Rename one meeting, as the user asked from its page (TUR-103). Answers with
/// the title as written. A user's name is never replaced by the calendar's or
/// the agent's; see [`store::meeting_title`].
pub fn rename(id: &str, title: &str) -> Result<String, UiError> {
    existing_meeting_dir(id)?;
    rename_in(&root()?, id, title)
}

/// [`rename`] under a given root.
pub(super) fn rename_in(root: &Path, id: &str, title: &str) -> Result<String, UiError> {
    if store::meeting_title::clean(title).is_none() {
        return Err(UiError::app(
            "blank-title",
            "A meeting needs a name. Type one, or press Escape to keep the old one.",
        ));
    }
    Ok(store::meeting_title::set_by_user(root, id, title)?)
}

/// Resolve a meeting id to its folder, refusing anything that is not a plain
/// folder name.
///
/// The id arrives from the webview, so `..` or an absolute path would otherwise
/// let a compromised page read and write anywhere the app can reach.
pub(super) fn meeting_dir(id: &str) -> Result<PathBuf, UiError> {
    Ok(store::folder::meeting_dir(&root()?, id)?)
}

/// [`meeting_dir`], plus a clear "not found" when the folder is not there.
fn existing_meeting_dir(id: &str) -> Result<PathBuf, UiError> {
    let dir = meeting_dir(id)?;
    if !dir.is_dir() {
        return Err(UiError::app(
            "meeting-not-found",
            format!("There is no meeting folder at {}.", dir.display()),
        ));
    }
    Ok(dir)
}

/// Build a list row from a folder's summary. The audio beside it is never
/// read, only its two 44-byte headers and `segments.json` ([`classify_audio`]).
pub(super) fn summarize(folder: &store::folder::FolderSummary, is_live: bool) -> MeetingSummary {
    let (date, time, _) = split_folder_name(&folder.id);

    let (line_count, last_timestamp) = match &folder.transcript {
        Some(stats) => (stats.line_count, stats.last_time.clone()),
        None => (0, None),
    };

    // meeting.md's title wins: the calendar's, the agent's or the user's
    // (TUR-103). A meeting.md with broken frontmatter has none and falls back
    // to the folder slug, which is never wrong, only less specific.
    let title = store::meeting::display_title(folder.meeting.as_ref(), &folder.id);

    let audio = classify_audio(&folder.path.join(AUDIO));
    let recording_state = if is_live {
        RecordingState::Recording
    } else if audio.ended_cleanly {
        RecordingState::ended_cleanly(&folder.id)
    } else {
        RecordingState::Interrupted
    };

    MeetingSummary {
        recording_state,
        audio_ms: audio.header_frames.map(|frames| duration_ms(frames) as u64),
        id: folder.id.clone(),
        title,
        attendees: folder
            .meeting
            .as_ref()
            .map(store::meeting::Meeting::attendees)
            .unwrap_or_default(),
        date,
        time,
        line_count,
        last_timestamp,
        has_notes: folder.has_notes,
        has_analysis: folder.meeting.as_ref().is_some_and(is_wrapped_up),
        notes_off: folder
            .meeting
            .as_ref()
            .is_some_and(store::notes_switch::is_off),
    }
}

/// Notes are written: `analyzed_by` is set, or a section has text (an agent
/// on the copy-prompt path may leave `analyzed_by` out). Switching notes off
/// can make a `meeting.md` with neither.
fn is_wrapped_up(meeting: &store::meeting::Meeting) -> bool {
    meeting.is_analyzed()
        || store::meeting::SECTIONS.iter().any(|heading| {
            meeting
                .section(heading)
                .is_some_and(|body| !body.trim().is_empty())
        })
}

/// The §3.4 lines `store` skipped, as the single count the review view shows.
pub(super) fn unparsed_lines(transcript: &store::transcript::Transcript) -> usize {
    transcript
        .problems
        .iter()
        .map(|problem| match problem {
            store::Problem::UnparsedLines { count } => *count,
            _ => 0,
        })
        .sum()
}
