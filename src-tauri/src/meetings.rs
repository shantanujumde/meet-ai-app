//! Reading the meetings folder for the app shell.
//!
//! A thin adapter over `crates/store`, which owns markdown read/write,
//! frontmatter and (from Phase 3c) the FTS index (SPEC §5). This module turns
//! a loaded [`store::folder::MeetingFolder`] into the shapes the webview
//! renders, and keeps the one thing `store` deliberately does not know about:
//! *where* the meetings root is, and moving it.
//!
//! Two rules carried over from the spec, because getting them wrong here is
//! invisible until a recording is lost:
//!
//! * **`transcript.md` is read-only to the app shell.** L7 makes markdown the
//!   source of truth and §3.4 makes it append-only. Nothing in this file opens
//!   it for writing.
//! * **A malformed file is a badge, not an error (SPEC §7).** `store` loads
//!   every file it can and reports the rest as problems; the unparsed-line
//!   count below is one of them.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::UiError;

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
    list()
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
pub fn list() -> Result<MeetingList, UiError> {
    let root = root()?;
    if !root.is_dir() {
        return Ok(MeetingList {
            root: root.display().to_string(),
            root_exists: false,
            meetings: Vec::new(),
        });
    }

    // `scan` skips `.app` and every other dot-folder, sorts newest first, and
    // never lets one unreadable folder hide the rest.
    let meetings = store::folder::scan(&root)?.iter().map(summarize).collect();

    Ok(MeetingList {
        root: root.display().to_string(),
        root_exists: true,
        meetings,
    })
}

/// Open one meeting: its transcript and its notes.
pub fn detail(id: &str) -> Result<MeetingDetail, UiError> {
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
                    speaker: line.speaker.label().to_string(),
                    text: line.text.clone(),
                })
                .collect(),
            unparsed_lines(transcript),
        ),
        None => (Vec::new(), 0),
    };

    Ok(MeetingDetail {
        summary: summarize(&folder),
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

/// Resolve a meeting id to its folder, refusing anything that is not a plain
/// folder name.
///
/// The id arrives from the webview, so `..` or an absolute path would otherwise
/// let a compromised page read and write anywhere the app can reach.
fn meeting_dir(id: &str) -> Result<PathBuf, UiError> {
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

/// Build a list row from a loaded folder.
fn summarize(folder: &store::folder::MeetingFolder) -> MeetingSummary {
    let (date, time, slug) = split_folder_name(&folder.id);

    let (line_count, last_timestamp) = match &folder.transcript {
        Some(transcript) => (
            transcript.lines.len(),
            transcript.lines.last().map(|line| line.time.clone()),
        ),
        None => (0, None),
    };

    // The agent-written title wins. A meeting.md with broken frontmatter has
    // none and falls back to the folder slug, which is never wrong, only less
    // specific.
    let title = folder
        .meeting
        .as_ref()
        .and_then(|meeting| meeting.title())
        .or_else(|| slug.map(prettify_slug))
        .unwrap_or_else(|| folder.id.clone());

    MeetingSummary {
        id: folder.id.clone(),
        title,
        date,
        time,
        line_count,
        last_timestamp,
        has_notes: !folder.notes.is_empty(),
        has_analysis: folder.meeting.is_some(),
    }
}

/// The §3.4 lines `store` skipped, as the single count the review view shows.
fn unparsed_lines(transcript: &store::transcript::Transcript) -> usize {
    transcript
        .problems
        .iter()
        .map(|problem| match problem {
            store::Problem::UnparsedLines { count } => *count,
            _ => 0,
        })
        .sum()
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

#[cfg(test)]
mod tests {
    use super::*;

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

    /// A meeting folder under a scratch root, loaded the way `list` and
    /// `detail` load one.
    fn load_fixture(name: &str, files: &[(&str, &[u8])]) -> store::folder::MeetingFolder {
        let dir = scratch(name).join("2026-09-01-1430-platform-standup");
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        for (file, body) in files {
            fs::write(dir.join(file), body).unwrap();
        }
        let folder = store::folder::load(&dir).expect("the folder itself is readable");
        fs::remove_dir_all(dir.parent().unwrap()).ok();
        folder
    }

    #[test]
    fn the_agent_written_title_wins_over_the_folder_slug() {
        let folder = load_fixture(
            "title",
            &[(
                "meeting.md",
                b"---\nid: 2026-09-01-1430-standup\ntitle: Platform Standup\n---\n\n## Summary\n",
            )],
        );
        let summary = summarize(&folder);
        assert_eq!(summary.title, "Platform Standup");
        assert!(summary.has_analysis);
    }

    #[test]
    fn broken_frontmatter_falls_back_to_the_slug_instead_of_failing_the_list() {
        // SPEC §7: an agent writing sloppy YAML is expected. The row still
        // shows, under the name the folder already gives it.
        let folder = load_fixture(
            "broken-title",
            &[("meeting.md", b"---\ntitle: \"unclosed\n---\n")],
        );
        let summary = summarize(&folder);
        assert_eq!(summary.title, "Platform standup");
        assert!(folder.needs_attention());
    }

    #[test]
    fn line_counts_and_the_unparsed_count_come_from_the_store_parser() {
        let folder = load_fixture(
            "counts",
            &[
                (
                    "transcript.md",
                    b"[00:00:04] Others: Morning.\nnot a transcript line\n[00:01:10] You: Hi.\n",
                ),
                ("notes.md", b"remember the Redis ticket"),
            ],
        );
        let summary = summarize(&folder);
        assert_eq!(summary.line_count, 2);
        assert_eq!(summary.last_timestamp.as_deref(), Some("00:01:10"));
        assert!(summary.has_notes);
        assert!(!summary.has_analysis);
        assert_eq!(unparsed_lines(folder.transcript.as_ref().unwrap()), 1);
    }

    #[test]
    fn an_empty_folder_is_a_meeting_with_nothing_in_it_yet() {
        let folder = load_fixture("empty", &[]);
        let summary = summarize(&folder);
        assert_eq!(summary.line_count, 0);
        assert_eq!(summary.last_timestamp, None);
        assert!(!summary.has_notes);
        assert!(folder.transcript.is_none(), "missing, not empty");
    }

    #[test]
    fn store_errors_keep_the_kinds_the_ui_already_branches_on() {
        let bad_id: UiError = store::Error::BadId("..".into()).into();
        assert_eq!((bad_id.domain, bad_id.kind), ("app", "bad-meeting-id"));
        let io: UiError = store::Error::Io(std::io::Error::other("disk gone")).into();
        assert_eq!((io.domain, io.kind), ("app", "io"));
        assert!(io.message.contains("disk gone"), "{}", io.message);
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
}
