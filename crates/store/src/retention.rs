//! Deleting old meeting audio (SPEC L16, §3.5 `audio.retention_days`).
//!
//! WAVs are big and transcripts are small. Once a meeting's transcript is
//! done, its audio is kept for `retention_days` (7 by default) and then
//! deleted. Only `audio/*.wav` is ever deleted: `segments.json` stays (it is
//! tiny, and the drift checks and re-runs read it), and so do the `audio/`
//! folder, `transcript.md`, `notes.md`, `meeting.md` and `tickets/`.
//!
//! Three steps, so the decision can be checked before anything is deleted:
//!
//! * [`survey`] reads the meetings folder: each meeting's WAVs, whether its
//!   transcript has anything in it, and when it happened.
//! * [`plan`] decides, with no I/O, which WAVs go. That list is the preview.
//! * [`apply`] deletes them and says what happened. A file another program
//!   holds open (Windows refuses to delete it) is skipped and counted, and the
//!   next run tries again.
//!
//! What counts as "busy" (recording, being transcribed, an agent run going)
//! is the app's to know, so the caller passes it in. A meeting whose
//! transcript is missing or empty is never touched: its audio is the only
//! way to make one.
//!
//! Nor is one whose transcript or recording cannot be shown to be whole
//! (TUR-85), since the WAV is the only way to re-transcribe it:
//!
//! * [`INCOMPLETE_MARKER`] (`audio/.incomplete`) is written when a recording
//!   starts ([`mark_incomplete`]) and removed only when its live transcript
//!   ended cleanly ([`mark_complete`]). A failed or timed-out transcription,
//!   a crash or a `kill -9` leaves it in place, on disk, across relaunches.
//! * A recording that did not end cleanly (the meeting list's "Interrupted")
//!   is kept too. Telling that is `audio`'s job, so the caller passes the
//!   check in to [`survey`]. A meeting from before the marker existed counts
//!   as done only when its recording ended cleanly.
//!
//! Plain `std::fs` only, and no `#[cfg(target_os)]`: the same code runs on
//! every OS (SPEC §8.2). The one OS difference, which error codes mean "held
//! by another program", is asked of [`crate::platform`].

use std::collections::{BTreeMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::{Local, NaiveDateTime, TimeZone as _};
use meeting_format::layout;

use crate::Error;
use crate::folder::meeting_dirs;
use crate::folder_name::split_folder_name;
use crate::meeting::Meeting;

/// One day, the unit of `retention_days`.
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// The extension of the files retention deletes.
const WAV_EXTENSION: &str = "wav";

/// `audio/.incomplete`: this meeting's transcript is not known to be whole,
/// so its audio is never deleted. A dot-file, so the folder watcher and the
/// meeting list ignore it.
pub const INCOMPLETE_MARKER: &str = ".incomplete";

/// `<meeting>/audio/.incomplete`.
pub fn incomplete_marker(meeting_dir: &Path) -> PathBuf {
    layout::audio_dir(meeting_dir).join(INCOMPLETE_MARKER)
}

/// Mark a meeting's transcript as not (yet) complete. Called when its
/// recording starts; creates `audio/` if needed and syncs the file, so a
/// crash a moment later still leaves it.
pub fn mark_incomplete(meeting_dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(layout::audio_dir(meeting_dir))?;
    let marker = std::fs::File::create(incomplete_marker(meeting_dir))?;
    marker.sync_all()
}

/// The meeting's transcript finished cleanly: its audio may be deleted once
/// it is old enough. A marker already gone is not an error.
pub fn mark_complete(meeting_dir: &Path) -> io::Result<()> {
    match std::fs::remove_file(incomplete_marker(meeting_dir)) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

/// How long a meeting's audio is kept, from `audio.retention_days`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retention {
    /// `-1`: never delete audio.
    KeepForever,
    /// Delete audio once the meeting is more than this many days old. `0`
    /// deletes it as soon as the transcript is done.
    Days(u32),
}

impl Retention {
    /// SPEC §3.5's default, `"retention_days": 7`.
    pub const DEFAULT: Retention = Retention::Days(7);

    /// The config value as written: `-1` keeps audio forever, `0` or more is
    /// a number of days. `None` for any other negative number, which is a
    /// config error.
    pub fn from_days(days: i64) -> Option<Self> {
        match days {
            -1 => Some(Retention::KeepForever),
            0.. => u32::try_from(days).ok().map(Retention::Days),
            _ => None,
        }
    }

    /// Back to the config value: `-1` for forever, else the days.
    pub fn as_days(self) -> i64 {
        match self {
            Retention::KeepForever => -1,
            Retention::Days(days) => i64::from(days),
        }
    }
}

impl Default for Retention {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// What [`plan`] needs to know about one meeting folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingAudio {
    /// The folder name, which is the meeting id.
    pub id: String,
    /// When the meeting ended, or else started, from `meeting.md` or the
    /// folder name; failing both, when its newest WAV was last written.
    /// `None` when there is nothing to tell by.
    pub happened_at: Option<SystemTime>,
    /// `transcript.md` exists and has more than whitespace in it.
    pub has_transcript: bool,
    /// [`INCOMPLETE_MARKER`] is there: the transcript is not known to be
    /// whole.
    pub transcript_incomplete: bool,
    /// The recording was stopped, not cut short (not "Interrupted"), as the
    /// caller's check of `audio/` said.
    pub ended_cleanly: bool,
    /// Every `audio/*.wav` in the folder.
    pub wavs: Vec<PathBuf>,
}

/// Every meeting folder under `root`, read for [`plan`]. A missing root has
/// none. One unreadable folder or file does not stop the rest: it is read as
/// best it can be, and an unreadable transcript counts as no transcript.
///
/// `ended_cleanly` is given a meeting's `audio/` folder and says whether its
/// recording was stopped rather than cut short (the app passes the meeting
/// list's classifier).
pub fn survey(
    root: &Path,
    ended_cleanly: &dyn Fn(&Path) -> bool,
) -> Result<Vec<MeetingAudio>, Error> {
    Ok(meeting_dirs(root)?
        .iter()
        .map(|dir| survey_meeting(dir, ended_cleanly))
        .collect())
}

/// One meeting folder, read for [`plan`].
pub fn survey_meeting(dir: &Path, ended_cleanly: &dyn Fn(&Path) -> bool) -> MeetingAudio {
    let id = dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let wavs = wavs_in(&layout::audio_dir(dir));
    let happened_at = written_time(dir, &id).or_else(|| newest_mtime(&wavs));
    MeetingAudio {
        has_transcript: has_text(&layout::transcript_path(dir)),
        // A marker that cannot be checked counts as there.
        transcript_incomplete: !matches!(incomplete_marker(dir).try_exists(), Ok(false)),
        ended_cleanly: ended_cleanly(&layout::audio_dir(dir)),
        happened_at,
        wavs,
        id,
    }
}

/// The WAVs to delete, given the meetings, the time now, the retention
/// setting and the ids of the meetings the app is busy with. No I/O: this is
/// the preview of what [`apply`] would do.
///
/// A meeting's WAVs are listed only when all of these hold:
///
/// * retention is not [`Retention::KeepForever`],
/// * the meeting is not in `busy` (recording, being transcribed, an agent
///   run going),
/// * its transcript has text in it, and is not marked incomplete,
/// * its recording ended cleanly (not Interrupted),
/// * it is more than `days × 24 h` old. With `0` days, age does not matter:
///   the transcript being done is enough. With more, a meeting whose age
///   cannot be told is kept.
pub fn plan(
    meetings: &[MeetingAudio],
    now: SystemTime,
    retention: Retention,
    busy: &HashSet<String>,
) -> Vec<PathBuf> {
    let Retention::Days(days) = retention else {
        return Vec::new();
    };
    meetings
        .iter()
        .filter(|meeting| meeting.transcript_done() && !busy.contains(&meeting.id))
        .filter(|meeting| days == 0 || is_older_than(meeting.happened_at, now, days))
        .flat_map(|meeting| meeting.wavs.iter().cloned())
        .collect()
}

impl MeetingAudio {
    /// The transcript is there and known to be whole, and so is the
    /// recording: the audio is no longer the only copy of anything.
    pub fn transcript_done(&self) -> bool {
        self.has_transcript && !self.transcript_incomplete && self.ended_cleanly
    }
}

/// `happened_at` is more than `days` whole days before `now`.
fn is_older_than(happened_at: Option<SystemTime>, now: SystemTime, days: u32) -> bool {
    let Some(happened_at) = happened_at else {
        return false;
    };
    // A meeting dated in the future (a wrong clock) is not old.
    now.duration_since(happened_at)
        .is_ok_and(|age| age > DAY * days)
}

/// One deleted WAV.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deleted {
    pub path: PathBuf,
    /// Its size just before it was deleted.
    pub bytes: u64,
}

/// One WAV that could not be deleted for a reason other than a lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failed {
    pub path: PathBuf,
    pub error: String,
}

/// What [`apply`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub deleted: Vec<Deleted>,
    /// In use by another program, or not ours to delete right now. The next
    /// run tries again.
    pub skipped_locked: Vec<PathBuf>,
    pub errors: Vec<Failed>,
}

/// Files and bytes freed in one meeting, for its log line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Freed {
    /// The deleted files' names, such as `mic.wav`.
    pub files: Vec<String>,
    pub bytes: u64,
}

impl Report {
    /// What was freed, by meeting id.
    pub fn by_meeting(&self) -> BTreeMap<String, Freed> {
        let mut meetings: BTreeMap<String, Freed> = BTreeMap::new();
        for deleted in &self.deleted {
            let freed = meetings.entry(meeting_of(&deleted.path)).or_default();
            freed.files.push(file_name(&deleted.path));
            freed.bytes += deleted.bytes;
        }
        meetings
    }

    /// Bytes freed in all.
    pub fn bytes(&self) -> u64 {
        self.deleted.iter().map(|deleted| deleted.bytes).sum()
    }
}

// Adapted from github.com/silverstein/minutes/crates/core/src/retention.rs @ c1e236acf3a3aea0729976cfc6959dcceb5cd75f (MIT)
// (`apply_audio_retention`: delete each planned file, collect what went and
// what failed.)
/// Delete every file in `plan` and say what happened.
///
/// Refuses anything that is not a `.wav` directly inside an `audio/` folder,
/// whatever the plan says, so a wrong plan cannot reach a transcript. A file
/// already gone is not an error. A locked file ([`is_locked`]) is skipped.
pub fn apply(plan: &[PathBuf]) -> Report {
    let mut report = Report::default();
    for path in plan {
        if !is_meeting_wav(path) {
            report.errors.push(Failed {
                path: path.clone(),
                error: "not a WAV inside a meeting's audio folder; left alone".to_owned(),
            });
            continue;
        }
        let bytes = std::fs::symlink_metadata(path).map_or(0, |metadata| metadata.len());
        match std::fs::remove_file(path) {
            Ok(()) => report.deleted.push(Deleted {
                path: path.clone(),
                bytes,
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) if is_locked(&error) => report.skipped_locked.push(path.clone()),
            Err(error) => report.errors.push(Failed {
                path: path.clone(),
                error: error.to_string(),
            }),
        }
    }
    report
}

/// The file is held by someone else, so try again later rather than report
/// it: `PermissionDenied` on any OS, or an OS's own "held open" codes
/// ([`crate::platform`]: Windows' sharing and lock violations, which a player
/// or a backup tool holding the WAV open causes).
pub fn is_locked(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::PermissionDenied
        || error
            .raw_os_error()
            .is_some_and(crate::platform::is_lock_violation)
}

/// `<meeting>/audio/<name>.wav`.
fn is_meeting_wav(path: &Path) -> bool {
    let in_audio = path
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|dir| dir == layout::AUDIO_DIR);
    in_audio && has_wav_extension(path)
}

fn has_wav_extension(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(WAV_EXTENSION))
}

/// The meeting folder name of `<meeting>/audio/<file>`.
fn meeting_of(wav: &Path) -> String {
    wav.parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The regular `.wav` files directly in `audio`, sorted. Symlinks and
/// folders are not ours to delete and are left out.
fn wavs_in(audio: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(audio) else {
        return Vec::new();
    };
    let mut wavs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.path())
        .filter(|path| has_wav_extension(path))
        .collect();
    wavs.sort();
    wavs
}

/// Whether the file at `path` has anything but whitespace in it.
fn has_text(path: &Path) -> bool {
    std::fs::read(path).is_ok_and(|bytes| bytes.iter().any(|byte| !byte.is_ascii_whitespace()))
}

/// When the meeting ended, or started: `meeting.md`'s `date` (plus
/// `duration_sec`), else the folder name's `YYYY-MM-DD-HHMM` in local time
/// (plus `duration_sec`).
fn written_time(dir: &Path, id: &str) -> Option<SystemTime> {
    let meeting = Meeting::read(&dir.join(layout::MEETING_FILE))
        .ok()
        .flatten();
    let duration = meeting
        .as_ref()
        .and_then(Meeting::duration_sec)
        .and_then(|seconds| u64::try_from(seconds).ok())
        .map_or(Duration::ZERO, Duration::from_secs);
    let started = meeting
        .as_ref()
        .and_then(Meeting::date)
        .and_then(|date| chrono::DateTime::parse_from_rfc3339(date.trim()).ok())
        .map(SystemTime::from)
        .or_else(|| folder_name_time(id))?;
    started.checked_add(duration)
}

/// `2026-09-01-1430-standup` → 2026-09-01 14:30 local time.
fn folder_name_time(id: &str) -> Option<SystemTime> {
    let (Some(date), Some(time), _) = split_folder_name(id) else {
        return None;
    };
    let naive = NaiveDateTime::parse_from_str(&format!("{date} {time}"), "%Y-%m-%d %H:%M").ok()?;
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(SystemTime::from)
}

/// When the newest of `wavs` was last written.
fn newest_mtime(wavs: &[PathBuf]) -> Option<SystemTime> {
    wavs.iter()
        .filter_map(|wav| std::fs::metadata(wav).and_then(|m| m.modified()).ok())
        .max()
}

#[cfg(test)]
mod tests;
