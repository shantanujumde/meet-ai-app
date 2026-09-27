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

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::UiError;

/// Files inside a meeting folder (SPEC §3.1).
const TRANSCRIPT: &str = "transcript.md";
const NOTES: &str = "notes.md";
const MEETING: &str = "meeting.md";

/// Where meetings live.
///
/// `~/Meetings` per SPEC §3.1, built with `dirs` + `PathBuf::join` and no
/// literal `~` (the Windows seam, SPEC §8.2).
///
/// `MEET_AI_MEETINGS_ROOT` overrides it. That exists so this screen can be
/// driven against a fixture folder without writing into the developer's real
/// meetings — `config.jsonc`'s `meetings_root` is Phase 6 and replaces it.
pub fn root() -> Result<PathBuf, UiError> {
    if let Some(custom) = std::env::var_os("MEET_AI_MEETINGS_ROOT") {
        return Ok(PathBuf::from(custom));
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

    let mut meetings = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        // `.app` holds config, models, logs and the derived index — it is not a
        // meeting. Skipping every dotfile also covers `.DS_Store` directories
        // and anything else the OS leaves lying around.
        if name.starts_with('.') {
            continue;
        }
        meetings.push(summarize(&entry.path(), name));
    }

    // Folder names start with `YYYY-MM-DD-HHMM`, so a reverse string sort is a
    // reverse chronological sort. Names that do not match sort to the end,
    // which is where anything unrecognised belongs.
    meetings.sort_by(|a, b| b.id.cmp(&a.id));

    Ok(MeetingList {
        root: root.display().to_string(),
        root_exists: true,
        meetings,
    })
}

/// Open one meeting: its transcript and its notes.
pub fn detail(id: &str) -> Result<MeetingDetail, UiError> {
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

    Ok(MeetingDetail {
        summary: summarize(&dir, name),
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
/// hundreds of megabytes of audio beside it.
fn summarize(dir: &Path, id: String) -> MeetingSummary {
    let (date, time, slug) = split_folder_name(&id);

    let (line_count, last_timestamp) = count_lines(&dir.join(TRANSCRIPT));

    let title = frontmatter_title(&dir.join(MEETING))
        .or_else(|| slug.map(prettify_slug))
        .unwrap_or_else(|| id.clone());

    MeetingSummary {
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
}
