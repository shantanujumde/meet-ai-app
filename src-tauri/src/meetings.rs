//! Reading the meetings folder for the app shell.
//!
//! Phase 2a only, and deliberately thin. `crates/store` owns markdown
//! read/write, frontmatter and the FTS index from Phase 3 (SPEC §5); this
//! module exists so the meeting list and review view have something real to
//! render before that lands, and it should shrink to a call into `store` when
//! it does.
//!
//! Two rules carried over from the spec, because getting them wrong here is
//! invisible until a recording is lost:
//!
//! * **`transcript.md` is read-only to the app shell.** L7 makes markdown the
//!   source of truth and §3.4 makes it append-only. Nothing in this file opens
//!   it for writing.
//! * **The §3.4 line format is parsed, not guessed.** A line that does not match
//!   is counted and skipped, never half-parsed — SPEC §7 says the UI shows a
//!   "needs attention" signal rather than failing, so the count is returned.
//!
//! It also tells a finished meeting from one whose recording was cut short
//! (TUR-97) — see [`RecordingState`] for the name and [`classify_audio`] for
//! the rule.

use std::fs::{self, OpenOptions};
use std::io::{Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

use audio::Channel;
use audio::segments::{Segments, duration_ms};
use audio::wav_writer::read_header_frames;
use serde::{Deserialize, Serialize};

use crate::error::UiError;
use crate::recording::{Phase, Status};

/// Files inside a meeting folder (SPEC §3.1).
const TRANSCRIPT: &str = "transcript.md";
const NOTES: &str = "notes.md";
const MEETING: &str = "meeting.md";
const AUDIO: &str = "audio";
const SEGMENTS: &str = "segments.json";

/// The canonical header `audio::wav_writer` writes, and the size of one mono
/// 16-bit frame. Private there; restated here because the audio check below
/// compares a file's real length against what its header declares, and
/// [`read_header_frames`] refuses any file that is not exactly this shape.
const WAV_HEADER_LEN: u64 = 44;
const WAV_BYTES_PER_FRAME: u64 = 2;
/// Where the RIFF chunk size and the `data` chunk size live in that header.
const WAV_RIFF_SIZE_OFFSET: u64 = 4;
const WAV_DATA_SIZE_OFFSET: u64 = 40;

/// Where meetings live.
///
/// `~/Meetings` per SPEC §3.1, built with `dirs` + `PathBuf::join` and no
/// literal `~` (the Windows seam, SPEC §8.2) — unless the user has picked
/// somewhere else, in which case [`configured_root`] wins.
///
/// `MEET_AI_MEETINGS_ROOT` overrides both. That exists so this screen can be
/// driven against a fixture folder without writing into the developer's real
/// meetings.
pub fn root() -> Result<PathBuf, UiError> {
    if let Some(custom) = std::env::var_os("MEET_AI_MEETINGS_ROOT") {
        return Ok(PathBuf::from(custom));
    }
    if let Some(configured) = configured_root() {
        return Ok(configured);
    }
    dirs::home_dir()
        .map(|home| home.join("Meetings"))
        .ok_or_else(|| {
            UiError::app(
                "no-home-dir",
                "meet-ai could not work out where your home folder is, so it does not know where \
                 to keep your meetings.",
            )
        })
}

/// Where the app remembers a user-chosen meetings folder.
///
/// This cannot live inside the meetings root itself — the whole point of the
/// pointer is to find the root before reading anything under it, and a folder
/// that has just been moved away from is the one place we can no longer read.
/// It lives in the OS's own per-app support folder instead: one small file
/// with nothing meeting-shaped in it, so this does not conflict with L10 (the
/// app writes nothing *meeting* data outside `~/Meetings/`).
fn pointer_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("meet-ai").join("root.json"))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RootPointer {
    custom_root: Option<String>,
}

/// The user's chosen folder, if they ever changed it from the default.
///
/// Same failure rule as everywhere else in this module: a missing or corrupt
/// pointer is not an error, it just means "no override", and the app falls
/// back to `~/Meetings` rather than refusing to start.
fn configured_root() -> Option<PathBuf> {
    let raw = fs::read_to_string(pointer_path()?).ok()?;
    let pointer: RootPointer = serde_json::from_str(&raw).ok()?;
    pointer
        .custom_root
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

fn write_pointer(new_root: &Path) -> Result<(), UiError> {
    let path = pointer_path().ok_or_else(|| {
        UiError::app(
            "no-config-dir",
            "meet-ai could not find a place on this Mac to remember your chosen folder.",
        )
    })?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let pointer = RootPointer {
        custom_root: Some(new_root.display().to_string()),
    };
    let body = serde_json::to_string_pretty(&pointer)
        .map_err(|error| UiError::app("serialize", error.to_string()))?;
    fs::write(&path, body)?;
    Ok(())
}

/// Move the meetings folder to `new_root`, taking every existing meeting with
/// it, and remember the new location for next launch.
///
/// Existing files always move — they are never left behind at the old path.
/// A meetings list that quietly stopped showing yesterday's standup the moment
/// someone picked a new folder would look exactly like data loss, even though
/// nothing was actually deleted.
pub fn change_root(new_root: PathBuf) -> Result<MeetingList, UiError> {
    let old_root = root()?;

    if new_root == old_root {
        return Err(UiError::app(
            "same-folder",
            "That is already your meetings folder.",
        ));
    }
    if new_root.starts_with(&old_root) || old_root.starts_with(&new_root) {
        return Err(UiError::app(
            "nested-folder",
            "The new folder can't be inside your current meetings folder, or the other way \
             around.",
        ));
    }

    if old_root.is_dir() {
        move_contents(&old_root, &new_root)?;
    } else {
        fs::create_dir_all(&new_root)?;
    }

    write_pointer(&new_root)?;
    // The IPC command refuses a move unless the recorder is idle, so nothing
    // under the new root is being written.
    list(Live::Nothing)
}

/// Move everything from `old_root` into `new_root`, merging rather than
/// clobbering if `new_root` already exists (e.g. the user picked an existing
/// folder inside an already-synced Dropbox or iCloud Drive location).
fn move_contents(old_root: &Path, new_root: &Path) -> Result<(), UiError> {
    if !new_root.exists() {
        if let Some(parent) = new_root.parent() {
            fs::create_dir_all(parent)?;
        }
        // The common case: one atomic rename, nothing to merge.
        if fs::rename(old_root, new_root).is_ok() {
            return Ok(());
        }
        // `rename(2)` refuses to jump filesystems (e.g. onto a different
        // volume), so fall back to an explicit copy-then-delete.
        copy_dir(old_root, new_root)?;
        fs::remove_dir_all(old_root)?;
        return Ok(());
    }

    // The destination already has something in it. Refuse outright on any
    // name collision rather than guessing which of two same-named folders is
    // the real meeting — silently overwriting one would be a straightforward
    // way to lose a recording.
    let mut conflicts = Vec::new();
    for entry in fs::read_dir(old_root)? {
        let name = entry?.file_name();
        if new_root.join(&name).exists() {
            conflicts.push(name.to_string_lossy().into_owned());
        }
    }
    if !conflicts.is_empty() {
        let noun = if conflicts.len() == 1 {
            "item"
        } else {
            "items"
        };
        return Err(UiError::app(
            "folder-conflict",
            format!(
                "\"{}\" already has {noun} named the same as something in your current meetings \
                 folder: {}. Rename or remove {noun} there first, then try again.",
                new_root.display(),
                conflicts.join(", "),
            ),
        ));
    }

    for entry in fs::read_dir(old_root)? {
        let entry = entry?;
        let from = entry.path();
        let to = new_root.join(entry.file_name());
        if fs::rename(&from, &to).is_err() {
            if entry.file_type()?.is_dir() {
                copy_dir(&from, &to)?;
                fs::remove_dir_all(&from)?;
            } else {
                fs::copy(&from, &to)?;
                fs::remove_file(&from)?;
            }
        }
    }
    // Best-effort: the folder is empty at this point on every platform this
    // ships on, but a leftover `.DS_Store` must not turn a successful move
    // into a reported failure.
    fs::remove_dir_all(old_root).ok();
    Ok(())
}

/// A recursive copy for the cross-volume fallback path. Std has no
/// `fs::copy` for directories.
fn copy_dir(from: &Path, to: &Path) -> Result<(), UiError> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}

/// One row in the meeting list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingSummary {
    /// The folder name, e.g. `2026-09-01-1430-standup`. Also the route param.
    pub id: String,
    /// What to show in the list.
    pub title: String,
    /// `YYYY-MM-DD`, parsed from the folder name. `None` if it does not match.
    pub date: Option<String>,
    /// `HH:MM`, parsed from the folder name.
    pub time: Option<String>,
    /// How many §3.4 lines parsed. 0 is a real, displayable answer.
    pub line_count: usize,
    /// The timestamp on the last parsed line — the meeting's readable length.
    pub last_timestamp: Option<String>,
    /// `notes.md` exists and has something in it.
    pub has_notes: bool,
    /// `meeting.md` exists, i.e. an agent has wrapped this meeting up.
    pub has_analysis: bool,
    /// Whether the recording ended on purpose. See [`RecordingState`].
    pub recording_state: RecordingState,
    /// Milliseconds of audio a player can actually reach: the longer of the
    /// two tracks, measured from its WAV header (SPEC A5 — header frames are
    /// the only true duration). `None` when the folder holds no audio at all,
    /// which is also what a meeting looks like after the retention job (L16)
    /// has deleted its WAVs.
    pub audio_ms: Option<u64>,
}

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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

/// Which meeting, if any, the running app is recording into.
///
/// Built from the recorder's [`Status`] so the list can leave a live meeting
/// alone: mid-recording, its files are indistinguishable from a killed one's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Live<'a> {
    #[default]
    Nothing,
    Meeting(&'a str),
}

impl<'a> Live<'a> {
    pub fn from_status(status: &'a Status) -> Self {
        match (status.phase, status.meeting_id.as_deref()) {
            (Phase::Idle, _) => Live::Nothing,
            (_, Some(id)) => Live::Meeting(id),
            // `Starting` before the recorder has picked an id. It publishes the
            // id before it creates the folder, so no folder can be live yet.
            (_, None) => Live::Nothing,
        }
    }
}

/// One parsed transcript line.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptLine {
    /// 0-based line number within `transcript.md`, per the Phase 2a contract.
    pub seq: usize,
    /// `HH:MM:SS`, the utterance *start* (SPEC §3.4).
    pub time: String,
    /// `You` or `Others` (L5). Kept as the literal spec string.
    pub speaker: String,
    pub text: String,
}

/// A finished meeting, opened for review.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingDetail {
    pub summary: MeetingSummary,
    /// Absolute path, so "Reveal in Finder" and the error copy can name it.
    pub path: String,
    pub lines: Vec<TranscriptLine>,
    /// `transcript.md` is missing entirely — a different state from "empty".
    pub transcript_missing: bool,
    /// Lines that did not match §3.4 and were skipped. Surfaced, not hidden.
    pub unparsed_line_count: usize,
    pub notes: String,
}

/// The meeting list plus enough context to write honest empty-state copy.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingList {
    pub root: String,
    /// The folder does not exist yet. Before the first recording it never does,
    /// and that is not an error worth alarming anyone about.
    pub root_exists: bool,
    pub meetings: Vec<MeetingSummary>,
}

/// List every meeting folder under the root, newest first.
pub fn list(live: Live<'_>) -> Result<MeetingList, UiError> {
    list_in(&root()?, live)
}

fn list_in(root: &Path, live: Live<'_>) -> Result<MeetingList, UiError> {
    if !root.is_dir() {
        return Ok(MeetingList {
            root: root.display().to_string(),
            root_exists: false,
            meetings: Vec::new(),
        });
    }

    let folders = meeting_folders(root)?;
    let live_id = live_id(live);
    let meetings = folders
        .into_iter()
        .map(|(path, name)| {
            let is_live = live_id.as_deref() == Some(name.as_str());
            summarize(&path, name, is_live)
        })
        .collect();

    Ok(MeetingList {
        root: root.display().to_string(),
        root_exists: true,
        meetings,
    })
}

/// Every meeting folder under `root`, newest first.
///
/// One odd entry never fails the whole list: a folder that cannot be stat'd
/// is logged and left out, the same way a stray file is, rather than turning
/// every other meeting into an error screen.
fn meeting_folders(root: &Path) -> Result<Vec<(PathBuf, String)>, UiError> {
    let mut folders = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!(%error, "skipping an unreadable entry in the meetings folder");
                continue;
            }
        };
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => {}
            Ok(_) => continue,
            Err(error) => {
                tracing::warn!(%error, path = %entry.path().display(), "skipping an entry that could not be inspected");
                continue;
            }
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        // `.app` holds config, models, logs and the derived index — it is not a
        // meeting. Skipping every dotfile also covers `.DS_Store` directories
        // and anything else the OS leaves lying around.
        if name.starts_with('.') {
            continue;
        }
        folders.push((entry.path(), name));
    }

    // Folder names start with `YYYY-MM-DD-HHMM`, so a reverse string sort is a
    // reverse chronological sort. Names that do not match sort to the end,
    // which is where anything unrecognised belongs.
    folders.sort_by(|a, b| b.1.cmp(&a.1));
    Ok(folders)
}

/// The folder name of the live meeting, if any.
fn live_id(live: Live<'_>) -> Option<String> {
    match live {
        Live::Nothing => None,
        Live::Meeting(id) => Some(id.to_string()),
    }
}

/// Open one meeting: its transcript and its notes.
pub fn detail(id: &str, live: Live<'_>) -> Result<MeetingDetail, UiError> {
    let dir = meeting_dir(id)?;
    if !dir.is_dir() {
        return Err(UiError::app(
            "meeting-not-found",
            format!("There is no meeting folder at {}.", dir.display()),
        ));
    }

    let transcript_path = dir.join(TRANSCRIPT);
    let transcript_missing = !transcript_path.is_file();
    let raw = if transcript_missing {
        String::new()
    } else {
        fs::read_to_string(&transcript_path)?
    };

    let mut lines = Vec::new();
    let mut unparsed_line_count = 0;
    for (seq, raw_line) in raw.lines().enumerate() {
        // A trailing newline produces one empty final entry. That is file
        // structure, not a malformed line, so it must not raise the count.
        if raw_line.trim().is_empty() {
            continue;
        }
        match parse_line(raw_line) {
            Some((time, speaker, text)) => lines.push(TranscriptLine {
                seq,
                time,
                speaker: speaker.to_string(),
                text,
            }),
            None => unparsed_line_count += 1,
        }
    }

    let notes = read_notes(&dir)?;
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| id.to_string());

    let is_live = match live {
        Live::Nothing => false,
        Live::Meeting(live) => live == name,
    };

    Ok(MeetingDetail {
        summary: summarize(&dir, name, is_live),
        path: dir.display().to_string(),
        lines,
        transcript_missing,
        unparsed_line_count,
        notes,
    })
}

/// Read `notes.md`, treating "not there yet" as an empty page rather than an
/// error — a meeting nobody has written notes on is the normal case.
fn read_notes(dir: &Path) -> Result<String, UiError> {
    match fs::read_to_string(dir.join(NOTES)) {
        Ok(body) => Ok(body),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.into()),
    }
}

/// Save the user's notes for one meeting.
///
/// Writes through a temp file and `rename(2)` so a crash mid-save cannot leave
/// a half-written `notes.md`. The notes pane autosaves while the user types, so
/// this runs often enough for that to matter.
pub fn write_notes(id: &str, body: &str) -> Result<(), UiError> {
    let dir = meeting_dir(id)?;
    if !dir.is_dir() {
        return Err(UiError::app(
            "meeting-not-found",
            format!("There is no meeting folder at {}.", dir.display()),
        ));
    }

    let final_path = dir.join(NOTES);
    let temp_path = dir.join(".notes.md.tmp");
    fs::write(&temp_path, body)?;
    fs::rename(&temp_path, &final_path)?;
    Ok(())
}

/// Resolve a meeting id to its folder, refusing anything that is not a plain
/// folder name.
///
/// The id arrives from the webview, so `..` or an absolute path would otherwise
/// let a compromised page read and write anywhere the app can reach.
fn meeting_dir(id: &str) -> Result<PathBuf, UiError> {
    let looks_like_a_folder_name = !id.is_empty()
        && !id.starts_with('.')
        && !id.contains('/')
        && !id.contains('\\')
        && Path::new(id).components().count() == 1;

    if !looks_like_a_folder_name {
        return Err(UiError::app(
            "bad-meeting-id",
            format!("{id:?} is not a meeting folder name."),
        ));
    }
    Ok(root()?.join(id))
}

/// Build a summary from a folder without reading the whole transcript into the
/// list. Line counting still streams the file, which is cheap next to the
/// hundreds of megabytes of audio beside it — and that audio is never read,
/// only its two 44-byte headers and `segments.json` ([`classify_audio`]).
fn summarize(dir: &Path, id: String, is_live: bool) -> MeetingSummary {
    let (date, time, slug) = split_folder_name(&id);

    let (line_count, last_timestamp) = count_lines(&dir.join(TRANSCRIPT));

    let title = frontmatter_title(&dir.join(MEETING))
        .or_else(|| slug.map(prettify_slug))
        .unwrap_or_else(|| id.clone());

    let audio = classify_audio(&dir.join(AUDIO));
    let recording_state = if is_live {
        RecordingState::Recording
    } else if audio.ended_cleanly {
        RecordingState::Finished
    } else {
        RecordingState::Interrupted
    };

    MeetingSummary {
        recording_state,
        audio_ms: audio.header_frames.map(|frames| duration_ms(frames) as u64),
        id,
        title,
        date,
        time,
        line_count,
        last_timestamp,
        has_notes: dir
            .join(NOTES)
            .metadata()
            .map(|m| m.len() > 0)
            .unwrap_or(false),
        has_analysis: dir.join(MEETING).is_file(),
    }
}

/// What a meeting's `audio/` folder says about how its recording ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AudioCheck {
    ended_cleanly: bool,
    /// The longer track's header-declared length, in frames. `None` when
    /// there is no WAV to read.
    header_frames: Option<u64>,
}

/// Did this recording stop on purpose?
///
/// SPEC A5 §3 (contract §7): `sum(*_frames) >= wav_header_frames` always,
/// **with equality after a graceful stop** — and `RecordingSession::stop`
/// finalises both headers to every sample written before it writes the last
/// `segments.json`. So a cleanly stopped recording is exactly this, for every
/// WAV present in `audio/`:
///
/// 1. `segments.json` is there and parses. The recorder writes it on every
///    stop and every checkpoint, even when the system tap failed (SPEC §3.4),
///    so a WAV with none beside it was killed before its first checkpoint —
///    which on v0.3.0, where checkpoints never ran, is every killed recording.
/// 2. The header is canonical and declares exactly the bytes on disk. A kill
///    leaves samples appended after the last header patch: up to one
///    checkpoint's worth, or all of them on v0.3.0, whose headers stay at 0.
/// 3. The header declares exactly the frames `segments.json` gives that
///    channel. After a kill it lags them (A5's benign crash window); running
///    ahead of them is something the writer cannot produce at all.
///
/// Any one failing means interrupted. No WAVs at all means there is nothing
/// to judge by, and the meeting is treated as finished: that is a folder from
/// before recording existed, or one whose audio retention (L16) has deleted.
///
/// Cost: one small JSON read, then a 44-byte read and a `stat` per WAV. The
/// samples are never read.
///
/// **The one kill this cannot see** lands after a checkpoint's header patch
/// and before the next sample reaches the file. Header, file and segments
/// then agree exactly — `segments.json` has no stop marker, and a stop's
/// final anchor looks like a checkpoint's — so it reads as finished. The
/// window is the gap between two buffer flushes, and a meeting caught in it
/// loses only the label: everything on disk is declared and timestamped.
fn classify_audio(audio_dir: &Path) -> AudioCheck {
    let wavs: Vec<(Channel, PathBuf)> = [Channel::Mic, Channel::System]
        .into_iter()
        .map(|channel| (channel, audio_dir.join(channel.wav_filename())))
        .filter(|(_, path)| path.is_file())
        .collect();
    if wavs.is_empty() {
        return AudioCheck {
            ended_cleanly: true,
            header_frames: None,
        };
    }

    let segments = fs::read_to_string(audio_dir.join(SEGMENTS))
        .ok()
        .and_then(|raw| Segments::from_json(&raw).ok());
    let mut ended_cleanly = segments.is_some();
    let mut longest: Option<u64> = None;

    for (channel, path) in &wavs {
        let (Ok(header_frames), Ok(meta)) = (read_header_frames(path), fs::metadata(path)) else {
            // Not a header meet-ai wrote, or shorter than one. Either way this
            // is not a recording that was stopped and finalised.
            ended_cleanly = false;
            continue;
        };
        longest = Some(longest.map_or(header_frames, |l| l.max(header_frames)));

        let declared_len = WAV_HEADER_LEN + header_frames * WAV_BYTES_PER_FRAME;
        if meta.len() != declared_len {
            ended_cleanly = false;
        }
        if let Some(segments) = &segments
            && segments.total_frames(*channel) != header_frames
        {
            ended_cleanly = false;
        }
    }

    AudioCheck {
        ended_cleanly,
        header_frames: longest,
    }
}

/// Make the audio of v0.3.0-era interrupted recordings playable. Run once at
/// launch, before anything can start a recording.
///
/// A recording killed before its first checkpoint — every killed recording
/// on v0.3.0, which never checkpointed — leaves WAVs whose headers still say
/// **0 bytes** of audio over minutes of real samples, and no `segments.json`.
/// Every conforming reader, `read_header_frames` included, trusts the header,
/// so that audio is unreachable: QuickTime plays nothing, and the meeting
/// would claim no audio was kept. TUR-97 rules out a repair step the user has
/// to take, so this one is silent.
///
/// It is deliberately narrow. It only touches a meeting that
///
/// * [`classify_audio`] calls interrupted,
/// * is not the one this app is recording (`live`),
/// * has **no** `segments.json` — the only shape where the header can be
///   behind *all* of the audio. A recording that has checkpointed is already
///   playable to within one checkpoint, and the samples past its header were
///   never accounted for in any `segments.json`; `WavWriter::open_append`
///   deliberately discards exactly those, and this does not second-guess it.
///
/// and in such a meeting it only rewrites the two size fields of a canonical
/// 44-byte header, to cover the whole frames already on disk. It never
/// truncates, moves or reorders a sample, never lowers a size, and a second
/// run finds nothing to do. The meeting stays labelled interrupted afterwards,
/// because it still has no `segments.json`.
///
/// Returns how many WAV headers it rewrote.
pub fn recover_interrupted_audio(live: Live<'_>) -> usize {
    match root() {
        Ok(root) => recover_in(&root, live),
        Err(error) => {
            tracing::warn!(message = %error.message, "no meetings folder to check for interrupted recordings");
            0
        }
    }
}

fn recover_in(root: &Path, live: Live<'_>) -> usize {
    if !root.is_dir() {
        return 0;
    }
    let folders = match meeting_folders(root) {
        Ok(folders) => folders,
        Err(error) => {
            tracing::warn!(message = %error.message, "could not list meetings to check for interrupted recordings");
            return 0;
        }
    };
    let live_id = live_id(live);

    let mut rewritten = 0;
    for (path, name) in &folders {
        if live_id.as_deref() == Some(name.as_str()) {
            continue;
        }
        let audio_dir = path.join(AUDIO);
        if audio_dir.join(SEGMENTS).exists() || classify_audio(&audio_dir).ended_cleanly {
            continue;
        }
        for channel in [Channel::Mic, Channel::System] {
            let wav = audio_dir.join(channel.wav_filename());
            if !wav.is_file() {
                continue;
            }
            match expose_unheadered_samples(&wav) {
                Ok(false) => {}
                Ok(true) => {
                    rewritten += 1;
                    tracing::info!(meeting = %name, file = channel.wav_filename(), "made an interrupted recording's audio playable");
                }
                // Left as found. The meeting still opens and still says it
                // was interrupted; the next launch tries again.
                Err(error) => {
                    tracing::warn!(%error, meeting = %name, file = channel.wav_filename(), "could not make an interrupted recording's audio playable")
                }
            }
        }
    }
    rewritten
}

/// Raise a canonical WAV header's declared length to cover every whole frame
/// on disk. `Ok(false)` when it already does, or when the file is not a
/// header this app wrote (which is then never touched).
fn expose_unheadered_samples(path: &Path) -> std::io::Result<bool> {
    let declared = read_header_frames(path)?;
    let on_disk = fs::metadata(path)?.len().saturating_sub(WAV_HEADER_LEN) / WAV_BYTES_PER_FRAME;
    // Both size fields are u32, and RIFF's counts the 36 header bytes after
    // it. Past ~37 hours of audio the header simply cannot say more.
    let most = (u64::from(u32::MAX) - (WAV_HEADER_LEN - 8)) / WAV_BYTES_PER_FRAME;
    let target = on_disk.min(most);
    if target <= declared {
        return Ok(false);
    }

    let data_bytes = target * WAV_BYTES_PER_FRAME;
    let riff_bytes = data_bytes + (WAV_HEADER_LEN - 8);
    let mut file = OpenOptions::new().write(true).open(path)?;
    // RIFF first, `data` last: `read_header_frames` goes by `data`, so a crash
    // between the two writes leaves the header reading as unrepaired and the
    // next launch redoes both, instead of stranding a stale RIFF size.
    file.seek(SeekFrom::Start(WAV_RIFF_SIZE_OFFSET))?;
    file.write_all(&(riff_bytes as u32).to_le_bytes())?;
    file.seek(SeekFrom::Start(WAV_DATA_SIZE_OFFSET))?;
    file.write_all(&(data_bytes as u32).to_le_bytes())?;
    file.sync_all()?;
    Ok(true)
}

/// Count parseable lines and remember the last timestamp seen.
///
/// The last timestamp is the meeting's readable length. SPEC A5 says a
/// recording's true duration is `wav_header_frames / 16000`, never a sum of
/// segment frames — but the WAV may already be deleted by the 7-day retention
/// job (L16) while the transcript stays forever, so the list uses the last
/// transcript timestamp and calls it "last line at", not "duration".
fn count_lines(transcript: &Path) -> (usize, Option<String>) {
    let Ok(raw) = fs::read_to_string(transcript) else {
        return (0, None);
    };
    let mut count = 0;
    let mut last = None;
    for line in raw.lines() {
        if let Some((time, _, _)) = parse_line(line) {
            count += 1;
            last = Some(time);
        }
    }
    (count, last)
}

/// Parse one `transcript.md` line.
///
/// This is SPEC §3.4's `^\[(\d{2}:\d{2}:\d{2})\] (You|Others): (.*)$`, written
/// out rather than compiled, because the prefix is fixed-width and anchored:
/// the regex crate is not in the workspace and this is the whole of it.
///
/// `(.*)$` takes the rest of the line verbatim, so a `]` or a `:` inside speech
/// is safe — which is exactly why §3.4 says no escaping is needed.
fn parse_line(raw: &str) -> Option<(String, &'static str, String)> {
    let rest = raw.strip_prefix('[')?;
    let (timestamp, rest) = rest.split_once("] ")?;
    if !is_hms(timestamp) {
        return None;
    }

    // Anchored on the literal speaker labels rather than "everything up to the
    // first colon", so a line whose speaker was mangled is reported as
    // unparsed instead of inventing a third speaker.
    let (speaker, text) = if let Some(text) = rest.strip_prefix("You:") {
        ("You", text)
    } else {
        ("Others", rest.strip_prefix("Others:")?)
    };

    Some((
        timestamp.to_string(),
        speaker,
        text.strip_prefix(' ').unwrap_or(text).to_string(),
    ))
}

/// `\d{2}:\d{2}:\d{2}`, and nothing longer.
fn is_hms(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 8
        && bytes[2] == b':'
        && bytes[5] == b':'
        && [0, 1, 3, 4, 6, 7]
            .iter()
            .all(|&i| bytes[i].is_ascii_digit())
}

/// Split `2026-09-01-1430-standup` into date, time and slug.
///
/// Returns `None`s rather than failing for a folder someone renamed by hand.
/// The list still shows it; it just sorts to the end and shows no date.
pub(crate) fn split_folder_name(id: &str) -> (Option<String>, Option<String>, Option<String>) {
    // YYYY-MM-DD-HHMM = 4+1+2+1+2+1+4 = 15 characters before the slug.
    let parts: Vec<&str> = id.splitn(5, '-').collect();
    let [year, month, day, hhmm, rest @ ..] = parts.as_slice() else {
        return (None, None, None);
    };
    let dated = year.len() == 4
        && month.len() == 2
        && day.len() == 2
        && hhmm.len() == 4
        && [year, month, day, hhmm]
            .iter()
            .all(|part| part.bytes().all(|b| b.is_ascii_digit()));
    if !dated {
        return (None, None, None);
    }

    (
        Some(format!("{year}-{month}-{day}")),
        Some(format!("{}:{}", &hhmm[..2], &hhmm[2..])),
        rest.first().map(|slug| (*slug).to_string()),
    )
}

/// `platform-standup` -> `Platform standup`.
fn prettify_slug(slug: String) -> String {
    let spaced = slug.replace('-', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => spaced,
    }
}

/// Pull `title:` out of `meeting.md`'s frontmatter (SPEC §3.2).
///
/// A deliberately minimal scan rather than a YAML parse: the app shell only
/// needs one scalar, and `crates/store` owns real frontmatter handling with
/// `yaml-rust2` from Phase 3. Anything this misses falls back to the folder
/// slug, which is never wrong, only less specific.
fn frontmatter_title(meeting_md: &Path) -> Option<String> {
    let raw = fs::read_to_string(meeting_md).ok()?;
    let body = raw.strip_prefix("---\n")?;
    let (frontmatter, _) = body.split_once("\n---")?;
    for line in frontmatter.lines() {
        if let Some(value) = line.strip_prefix("title:") {
            let value = value.trim().trim_matches(['"', '\'']).trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_spec_3_4_example_lines() {
        let (time, speaker, text) =
            parse_line("[00:00:04] Others: Morning everyone, let's start with the API work.")
                .expect("the spec's own example must parse");
        assert_eq!(time, "00:00:04");
        assert_eq!(speaker, "Others");
        assert_eq!(text, "Morning everyone, let's start with the API work.");

        let (_, speaker, text) =
            parse_line("[00:00:11] You: Sessions are still in memory, that's the blocker.")
                .expect("the spec's own example must parse");
        assert_eq!(speaker, "You");
        assert_eq!(text, "Sessions are still in memory, that's the blocker.");
    }

    #[test]
    fn brackets_and_colons_inside_speech_need_no_escaping() {
        // §3.4: the prefix is fixed-width and anchored, so `(.*)$` is safe.
        let (_, _, text) = parse_line("[01:02:03] You: see issue [TUR-17]: it is the shell")
            .expect("punctuation in speech is not a parse failure");
        assert_eq!(text, "see issue [TUR-17]: it is the shell");
    }

    #[test]
    fn rejects_lines_that_are_not_the_contract() {
        assert!(parse_line("").is_none());
        assert!(parse_line("just some prose").is_none());
        assert!(parse_line("## A heading").is_none());
        // Wrong speaker label — reported as unparsed, not read as a speaker.
        assert!(parse_line("[00:00:04] Priya: hello").is_none());
        // Timestamp not HH:MM:SS.
        assert!(parse_line("[0:00:04] You: hello").is_none());
        assert!(parse_line("[00:00:04.5] You: hello").is_none());
    }

    #[test]
    fn empty_text_still_parses_even_though_it_is_never_written() {
        // §3.4 forbids writing one, but a hand-edited file can contain one and
        // the reader must not fall over.
        let (_, speaker, text) = parse_line("[00:00:04] You:").expect("must not fail");
        assert_eq!(speaker, "You");
        assert_eq!(text, "");
    }

    #[test]
    fn seq_is_the_zero_based_line_number_including_skipped_lines() {
        let raw = "not a transcript line\n[00:00:04] You: first real line\n";
        let mut seqs = Vec::new();
        let mut unparsed = 0;
        for (seq, line) in raw.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            match parse_line(line) {
                Some(_) => seqs.push(seq),
                None => unparsed += 1,
            }
        }
        assert_eq!(seqs, vec![1]);
        assert_eq!(unparsed, 1);
    }

    #[test]
    fn splits_a_spec_3_1_folder_name() {
        let (date, time, slug) = split_folder_name("2026-09-01-1430-standup");
        assert_eq!(date.as_deref(), Some("2026-09-01"));
        assert_eq!(time.as_deref(), Some("14:30"));
        assert_eq!(slug.as_deref(), Some("standup"));
    }

    #[test]
    fn a_slug_with_dashes_survives_the_split() {
        let (_, _, slug) = split_folder_name("2026-09-01-1430-platform-standup");
        assert_eq!(slug.as_deref(), Some("platform-standup"));
        assert_eq!(prettify_slug("platform-standup".into()), "Platform standup");
    }

    #[test]
    fn a_hand_renamed_folder_is_listed_without_a_date_rather_than_dropped() {
        assert_eq!(split_folder_name("my-old-notes"), (None, None, None));
        assert_eq!(split_folder_name(""), (None, None, None));
    }

    #[test]
    fn a_meeting_id_cannot_escape_the_meetings_root() {
        for hostile in ["..", "../../etc", "/etc/passwd", ".app", "", "a/b"] {
            assert!(
                meeting_dir(hostile).is_err(),
                "{hostile:?} must be refused before it becomes a path"
            );
        }
    }

    #[test]
    fn reads_the_agent_written_title_out_of_meeting_md() {
        let dir = std::env::temp_dir().join(format!("meet-ai-fm-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("meeting.md");
        fs::write(
            &path,
            "---\nid: 2026-09-01-1430-standup\ntitle: Platform Standup\n---\n\n## Summary\n",
        )
        .unwrap();
        assert_eq!(
            frontmatter_title(&path).as_deref(),
            Some("Platform Standup")
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// A scratch folder unique to this test run, cleaned up by the caller.
    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("meet-ai-move-{name}-{}", std::process::id()))
    }

    #[test]
    fn the_root_pointer_round_trips_through_the_file_format() {
        let written = RootPointer {
            custom_root: Some("/Volumes/Data/Meetings".into()),
        };
        let json = serde_json::to_string(&written).expect("serializes");
        assert!(json.contains("customRoot"), "{json}");
        let read: RootPointer = serde_json::from_str(&json).expect("round-trips");
        assert_eq!(read.custom_root, written.custom_root);
    }

    #[test]
    fn moving_into_a_folder_that_does_not_exist_yet_takes_everything_with_it() {
        let old_root = scratch("plain-old");
        let new_root = scratch("plain-new");
        fs::remove_dir_all(&old_root).ok();
        fs::remove_dir_all(&new_root).ok();

        fs::create_dir_all(old_root.join("2026-09-01-1430-standup")).unwrap();
        fs::write(
            old_root
                .join("2026-09-01-1430-standup")
                .join("transcript.md"),
            "[00:00:04] You: hi\n",
        )
        .unwrap();

        move_contents(&old_root, &new_root).expect("the move must succeed");

        assert!(!old_root.exists(), "the old folder must not linger");
        assert!(
            new_root
                .join("2026-09-01-1430-standup")
                .join("transcript.md")
                .is_file()
        );

        fs::remove_dir_all(&new_root).ok();
    }

    #[test]
    fn moving_into_an_occupied_folder_merges_rather_than_clobbers() {
        let old_root = scratch("merge-old");
        let new_root = scratch("merge-new");
        fs::remove_dir_all(&old_root).ok();
        fs::remove_dir_all(&new_root).ok();

        fs::create_dir_all(old_root.join("2026-09-01-1430-standup")).unwrap();
        fs::create_dir_all(new_root.join("2026-08-01-0900-retro")).unwrap();

        move_contents(&old_root, &new_root).expect("a non-colliding merge must succeed");

        assert!(!old_root.exists());
        assert!(new_root.join("2026-09-01-1430-standup").is_dir());
        assert!(
            new_root.join("2026-08-01-0900-retro").is_dir(),
            "what was already at the destination must survive the merge"
        );

        fs::remove_dir_all(&new_root).ok();
    }

    #[test]
    fn a_name_collision_refuses_the_whole_move_rather_than_guessing_which_copy_wins() {
        let old_root = scratch("conflict-old");
        let new_root = scratch("conflict-new");
        fs::remove_dir_all(&old_root).ok();
        fs::remove_dir_all(&new_root).ok();

        fs::create_dir_all(old_root.join("2026-09-01-1430-standup")).unwrap();
        fs::write(
            old_root.join("2026-09-01-1430-standup").join("notes.md"),
            "the real notes",
        )
        .unwrap();
        fs::create_dir_all(new_root.join("2026-09-01-1430-standup")).unwrap();

        let error =
            move_contents(&old_root, &new_root).expect_err("a same-named folder must refuse");
        assert_eq!(error.kind, "folder-conflict");
        // Nothing was touched: the source is intact and the destination's
        // existing folder was not overwritten with the source's contents.
        assert!(
            old_root
                .join("2026-09-01-1430-standup")
                .join("notes.md")
                .is_file()
        );
        assert!(
            !new_root
                .join("2026-09-01-1430-standup")
                .join("notes.md")
                .exists()
        );

        fs::remove_dir_all(&old_root).ok();
        fs::remove_dir_all(&new_root).ok();
    }

    // --- TUR-97: finished vs interrupted, and the launch-time header fix ---
    //
    // Every fixture is written by the recorder's own `WavWriter` and
    // `SegmentsWriter`, driven through the exact sequence a real stop or a
    // real kill leaves behind, so these tests break if the recorder's on-disk
    // shapes change underneath the classification.

    use audio::segments::{Anchor, SegmentOpen, SegmentsWriter};
    use audio::wav_writer::WavWriter;

    /// One second of audio at the recorder's 16 kHz.
    const SECOND: u64 = 16_000;

    /// A fresh, empty meetings root unique to this test.
    fn meetings_root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("meet-ai-state-{name}-{}", std::process::id()));
        fs::remove_dir_all(&root).ok();
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// A meeting folder the way `recording::create_meeting_folder` makes one.
    /// Returns its `audio/` folder.
    fn meeting(root: &Path, id: &str) -> PathBuf {
        let dir = root.join(id);
        fs::create_dir_all(dir.join(AUDIO)).unwrap();
        fs::write(dir.join(TRANSCRIPT), "[00:00:04] You: hello\n").unwrap();
        fs::write(dir.join(NOTES), "").unwrap();
        dir.join(AUDIO)
    }

    /// A ramp, so a test can tell a sample that moved from one that did not.
    fn samples(frames: u64) -> Vec<i16> {
        (0..frames).map(|n| (n % 30_000) as i16).collect()
    }

    /// Write one track: `checkpointed` frames synced and declared the way a
    /// checkpoint or a stop does it, then `after` more appended with no header
    /// patch — the samples a kill strands past the header.
    fn track(audio_dir: &Path, channel: Channel, checkpointed: u64, after: u64) {
        let mut wav = WavWriter::create(&audio_dir.join(channel.wav_filename())).unwrap();
        if checkpointed > 0 {
            wav.append(&samples(checkpointed)).unwrap();
            wav.fsync_data().unwrap();
            wav.patch_header().unwrap();
        }
        if after > 0 {
            wav.append(&samples(after)).unwrap();
        }
    }

    /// `segments.json` for one segment, written through the recorder's own
    /// atomic write. `sys_frames: None` is a tap that never started.
    fn segments_json(audio_dir: &Path, mic_frames: u64, sys_frames: Option<u64>) {
        let mut writer = SegmentsWriter::new(SegmentOpen {
            start_host_ns: 1_000_000_000,
            start_continuous_ns: Some(1_000_000_000),
            start_unix_ns: None,
            mic_rate: 16_000,
            sys_rate: if sys_frames.is_some() { 16_000 } else { 0 },
            mic_device_rate: None,
            sys_device_rate: None,
            reason: audio::segments::reason::START.into(),
        });
        let sys_frames = sys_frames.unwrap_or(0);
        writer.update_frames(mic_frames, sys_frames);
        writer.checkpoint_anchor(Anchor {
            mic_host_ns: 2_000_000_000,
            mic_frames,
            sys_host_ns: 2_000_000_000,
            sys_frames,
        });
        writer.write_atomic(&audio_dir.join(SEGMENTS)).unwrap();
    }

    fn summary_of(root: &Path, id: &str, live: Live<'_>) -> MeetingSummary {
        list_in(root, live)
            .unwrap()
            .meetings
            .into_iter()
            .find(|m| m.id == id)
            .unwrap_or_else(|| panic!("{id} is missing from the list"))
    }

    fn header_frames(audio_dir: &Path, channel: Channel) -> u64 {
        read_header_frames(&audio_dir.join(channel.wav_filename())).unwrap()
    }

    #[test]
    fn a_recording_stopped_with_the_stop_button_is_finished() {
        let root = meetings_root("clean");
        let audio = meeting(&root, "2026-09-30-1129-meeting");
        // `RecordingSession::stop`: both headers finalised to every sample,
        // then `segments.json` with the same totals — A5's equality.
        track(&audio, Channel::Mic, 10 * SECOND, 0);
        track(&audio, Channel::System, 10 * SECOND + 32, 0);
        segments_json(&audio, 10 * SECOND, Some(10 * SECOND + 32));

        let summary = summary_of(&root, "2026-09-30-1129-meeting", Live::Nothing);

        assert_eq!(summary.recording_state, RecordingState::Finished);
        assert_eq!(
            summary.audio_ms,
            Some(10_002),
            "the longer track, from its header"
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_v0_3_0_kill_with_zero_byte_headers_and_no_segments_json_is_interrupted() {
        // The shape on disk at ~/Meetings/2026-09-30-1140-meeting: v0.3.0
        // never checkpointed, so a kill leaves headers still declaring 0
        // bytes over the real samples, and no `segments.json` at all.
        let root = meetings_root("prefix-kill");
        let audio = meeting(&root, "2026-09-30-1140-meeting");
        track(&audio, Channel::Mic, 0, 16 * SECOND);
        track(&audio, Channel::System, 0, 16 * SECOND);

        let summary = summary_of(&root, "2026-09-30-1140-meeting", Live::Nothing);

        assert_eq!(summary.recording_state, RecordingState::Interrupted);
        assert_eq!(
            summary.audio_ms,
            Some(0),
            "no player can reach audio the header hides"
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_kill_between_checkpoints_is_interrupted() {
        // With 5 s checkpoints: header and segments agree at the last one,
        // and up to one checkpoint of samples sits past the header.
        let root = meetings_root("checkpointed-kill");
        let audio = meeting(&root, "2026-09-30-1200-meeting");
        track(&audio, Channel::Mic, 60 * SECOND, 3 * SECOND);
        track(&audio, Channel::System, 60 * SECOND, 3 * SECOND);
        segments_json(&audio, 60 * SECOND, Some(60 * SECOND));

        let summary = summary_of(&root, "2026-09-30-1200-meeting", Live::Nothing);

        assert_eq!(summary.recording_state, RecordingState::Interrupted);
        assert_eq!(summary.audio_ms, Some(60_000));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_kill_between_the_segments_json_rename_and_the_header_patch_is_interrupted() {
        // A5's benign crash window: the segments describe a checkpoint the
        // headers never got to declare. File length alone misses this one
        // when nothing was appended after the sync — the segment totals do
        // not.
        let root = meetings_root("window-kill");
        let audio = meeting(&root, "2026-09-30-1210-meeting");
        track(&audio, Channel::Mic, 55 * SECOND, 0);
        track(&audio, Channel::System, 55 * SECOND, 0);
        segments_json(&audio, 60 * SECOND, Some(60 * SECOND));

        let summary = summary_of(&root, "2026-09-30-1210-meeting", Live::Nothing);

        assert_eq!(summary.recording_state, RecordingState::Interrupted);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_meeting_being_recorded_is_never_labelled_interrupted() {
        let root = meetings_root("live");
        let older = meeting(&root, "2026-09-30-1140-meeting");
        track(&older, Channel::Mic, 0, SECOND);
        let live = meeting(&root, "2026-09-30-1300-meeting");
        track(&live, Channel::Mic, 0, SECOND);
        let live_id = "2026-09-30-1300-meeting";

        assert_eq!(
            summary_of(&root, live_id, Live::Meeting(live_id)).recording_state,
            RecordingState::Recording
        );
        // Being live covers one meeting, not every meeting.
        assert_eq!(
            summary_of(&root, "2026-09-30-1140-meeting", Live::Meeting(live_id)).recording_state,
            RecordingState::Interrupted
        );
        // And once nobody is recording into them, the same files are what
        // they look like.
        assert_eq!(
            summary_of(&root, live_id, Live::Nothing).recording_state,
            RecordingState::Interrupted
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_recorder_status_maps_onto_the_meeting_being_written() {
        let status = |phase, id: Option<&str>| Status {
            phase,
            meeting_id: id.map(str::to_string),
            started_at_ms: None,
        };
        let id = "2026-09-30-1300-meeting";
        assert_eq!(Live::from_status(&status(Phase::Idle, None)), Live::Nothing);
        assert_eq!(
            Live::from_status(&status(Phase::Recording, Some(id))),
            Live::Meeting(id)
        );
        // `Stopping` is still writing: the headers are being finalised.
        assert_eq!(
            Live::from_status(&status(Phase::Stopping, Some(id))),
            Live::Meeting(id)
        );
        assert_eq!(
            Live::from_status(&status(Phase::Starting, None)),
            Live::Nothing
        );
    }

    #[test]
    fn a_folder_with_no_audio_is_finished_not_interrupted() {
        let root = meetings_root("no-audio");
        // No `audio/` at all — the Phase 2a folders on disk look like this.
        fs::create_dir_all(root.join("2026-09-28-1216-meeting")).unwrap();
        // An empty `audio/`, and one where retention (L16) deleted the WAVs
        // but left `segments.json`.
        meeting(&root, "2026-09-28-1225-meeting");
        let retained = meeting(&root, "2026-09-28-1235-meeting");
        segments_json(&retained, 60 * SECOND, Some(60 * SECOND));

        for id in [
            "2026-09-28-1216-meeting",
            "2026-09-28-1225-meeting",
            "2026-09-28-1235-meeting",
        ] {
            let summary = summary_of(&root, id, Live::Nothing);
            assert_eq!(summary.recording_state, RecordingState::Finished, "{id}");
            assert_eq!(summary.audio_ms, None, "{id}");
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_clean_stop_with_no_system_audio_is_still_finished() {
        // SPEC §3.4: a tap that never started still gets a `segments.json`,
        // with `sys_rate` 0 — and there is no `system.wav` to compare.
        let root = meetings_root("mic-only");
        let audio = meeting(&root, "2026-09-30-1400-meeting");
        track(&audio, Channel::Mic, 30 * SECOND, 0);
        segments_json(&audio, 30 * SECOND, None);

        assert_eq!(
            summary_of(&root, "2026-09-30-1400-meeting", Live::Nothing).recording_state,
            RecordingState::Finished
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn one_odd_folder_never_fails_the_list() {
        let root = meetings_root("odd");
        let fine = meeting(&root, "2026-09-30-1129-meeting");
        track(&fine, Channel::Mic, SECOND, 0);
        track(&fine, Channel::System, SECOND, 0);
        segments_json(&fine, SECOND, Some(SECOND));

        // Not a WAV header at all, and a `segments.json` that is not JSON.
        let odd = meeting(&root, "2026-09-30-1500-meeting");
        fs::write(odd.join("mic.wav"), b"RIF").unwrap();
        fs::write(odd.join(SEGMENTS), "{ not json").unwrap();
        // A stray file beside the meetings.
        fs::write(root.join("stray.txt"), "hi").unwrap();

        let list = list_in(&root, Live::Nothing).expect("one odd folder must not fail the list");

        assert_eq!(list.meetings.len(), 2);
        assert_eq!(
            summary_of(&root, "2026-09-30-1500-meeting", Live::Nothing).recording_state,
            RecordingState::Interrupted,
            "a meeting the app cannot show was stopped must not claim it was"
        );
        assert_eq!(
            summary_of(&root, "2026-09-30-1129-meeting", Live::Nothing).recording_state,
            RecordingState::Finished
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn launch_recovery_makes_a_v0_3_0_kill_playable_without_moving_a_sample() {
        let root = meetings_root("recover");
        let audio = meeting(&root, "2026-09-30-1140-meeting");
        track(&audio, Channel::Mic, 0, 16 * SECOND);
        track(&audio, Channel::System, 0, 15 * SECOND);
        // A trailing half-frame: a kill can land mid-sample.
        let mic_path = audio.join("mic.wav");
        OpenOptions::new()
            .append(true)
            .open(&mic_path)
            .unwrap()
            .write_all(&[0x7f])
            .unwrap();
        let mic_before = fs::read(&mic_path).unwrap();
        let sys_before = fs::read(audio.join("system.wav")).unwrap();

        assert_eq!(recover_in(&root, Live::Nothing), 2);

        assert_eq!(header_frames(&audio, Channel::Mic), 16 * SECOND);
        assert_eq!(header_frames(&audio, Channel::System), 15 * SECOND);
        let mic_after = fs::read(&mic_path).unwrap();
        let sys_after = fs::read(audio.join("system.wav")).unwrap();
        // Only the two size fields changed. Every sample byte, and the file
        // length, is exactly what the recorder left — odd byte included.
        assert_eq!(mic_after.len(), mic_before.len());
        assert_eq!(mic_after[44..], mic_before[44..]);
        assert_eq!(sys_after[44..], sys_before[44..]);
        assert_eq!(mic_after[0..4], mic_before[0..4]);
        assert_eq!(mic_after[8..40], mic_before[8..40]);
        let riff = u32::from_le_bytes(mic_after[4..8].try_into().unwrap());
        assert_eq!(u64::from(riff), 36 + 16 * SECOND * 2);

        // Still honest about what happened, and now says how much it kept.
        let summary = summary_of(&root, "2026-09-30-1140-meeting", Live::Nothing);
        assert_eq!(summary.recording_state, RecordingState::Interrupted);
        assert_eq!(summary.audio_ms, Some(16_000));

        // Idempotent: nothing left to do, nothing touched.
        assert_eq!(recover_in(&root, Live::Nothing), 0);
        assert_eq!(fs::read(&mic_path).unwrap(), mic_after);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn launch_recovery_leaves_everything_else_exactly_as_it_was() {
        let root = meetings_root("recover-scope");
        // A finished meeting.
        let finished = meeting(&root, "2026-09-30-1129-meeting");
        track(&finished, Channel::Mic, 5 * SECOND, 0);
        segments_json(&finished, 5 * SECOND, None);
        // A checkpointed kill: its excess samples were never in any
        // `segments.json`, and `WavWriter::open_append` discards exactly
        // those — this must not resurrect them.
        let checkpointed = meeting(&root, "2026-09-30-1200-meeting");
        track(&checkpointed, Channel::Mic, 60 * SECOND, 3 * SECOND);
        segments_json(&checkpointed, 60 * SECOND, None);
        // A file whose header this app did not write.
        let foreign = meeting(&root, "2026-09-30-1210-meeting");
        let mut junk = vec![0u8; 44];
        junk.extend_from_slice(&[1u8; 4000]);
        fs::write(foreign.join("mic.wav"), &junk).unwrap();
        // The meeting being recorded right now, newest.
        let live = meeting(&root, "2026-09-30-1300-meeting");
        track(&live, Channel::Mic, 0, 4 * SECOND);

        let dirs = [&finished, &checkpointed, &foreign, &live];
        let snapshot = || -> Vec<Vec<u8>> {
            dirs.iter()
                .map(|dir| fs::read(dir.join("mic.wav")).unwrap())
                .collect()
        };
        let before = snapshot();

        assert_eq!(
            recover_in(&root, Live::Meeting("2026-09-30-1300-meeting")),
            0
        );

        assert_eq!(before, snapshot());
        fs::remove_dir_all(&root).ok();
    }
}
