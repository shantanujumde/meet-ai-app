//! TUR-12: the per-meeting "Make notes for this meeting" switch (SPEC A11),
//! checked on a real folder on disk.
//!
//! Off is `agent_notes: off` in `meeting.md`, so it has to survive everything
//! the markdown survives: other keys, the sections, a rescan, a rebuilt
//! index. Every test builds its own meetings root under the OS temp dir and
//! uses only the crate's public API.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use prompts::notes::Notes;
use store::agent_notes::{self, AGENT_NOTES_KEY, Analysis, AnalyzedBy};
use store::index::{Index, index_path};
use store::meeting::Meeting;
use store::notes_switch;
use store::watcher::SelfWrites;
use store::{Error, MEETING_FILE, NOTES_FILE, TICKETS_DIR, TRANSCRIPT_FILE};

const STANDUP: &str = "2026-09-01-1430-standup";

const TRANSCRIPT: &str = "\
[00:00:04] Others: Morning everyone, let's start with the API work.
[00:14:22] You: Sessions are still in memory, that's the blocker.
";
const NOTES: &str = "# My notes\n\n- Redis first.\n";

/// A `meeting.md` with a key this code has never heard of, a preamble, an
/// extra section and the user's own words in a section.
const WRAPPED_UP: &str = "\
---
id: 2026-09-01-1430-standup
title: Platform Standup
date: 2026-09-01T14:30:00+05:30
attendees: [Shantanu, Priya]
x_custom: { keep: me, list: [1, 2] }
---

Written before any heading.

## Summary

The user's own summary.

## Decisions

## Action Items

## Open Questions

## Follow-up

Call Priya on Friday.
";

/// A throwaway meetings root, removed on drop.
struct Root {
    path: PathBuf,
    _guard: tempfile::TempDir,
}

impl Root {
    fn new(name: &str) -> Self {
        let guard = tempfile::Builder::new()
            .prefix(&format!("meet-ai-store-notes-switch-{name}-"))
            .tempdir()
            .unwrap();
        Self {
            path: guard.path().to_path_buf(),
            _guard: guard,
        }
    }

    /// A meeting folder with a transcript and notes, and no `meeting.md`.
    fn meeting(&self, id: &str) -> PathBuf {
        let dir = self.path.join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(TRANSCRIPT_FILE), TRANSCRIPT).unwrap();
        fs::write(dir.join(NOTES_FILE), NOTES).unwrap();
        dir
    }

    fn set(&self, on: bool) {
        notes_switch::set(&self.path, STANDUP, on, &SelfWrites::default()).unwrap();
    }

    fn read_meeting(&self) -> Meeting {
        Meeting::read(&self.path.join(STANDUP).join(MEETING_FILE))
            .unwrap()
            .expect("meeting.md is there")
    }

    fn write_notes(&self) -> agent_notes::Outcome {
        agent_notes::write(
            &self.path,
            STANDUP,
            &notes(),
            &Analysis {
                by: AnalyzedBy::ClaudeCode,
                model: "opus".to_owned(),
                at: "2026-09-01T15:32:00+05:30".to_owned(),
            },
            &SelfWrites::default(),
        )
        .unwrap()
    }
}

fn notes() -> Notes {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("notes")
        .join("standup.json");
    Notes::from_json(&fs::read_to_string(path).unwrap()).unwrap()
}

fn agent_notes_value(meeting: &Meeting) -> Option<String> {
    meeting.frontmatter.get_str(AGENT_NOTES_KEY)
}

#[test]
fn off_creates_meeting_md_with_the_key_when_there_was_none() {
    let root = Root::new("off-fresh");
    let dir = root.meeting(STANDUP);
    let writes = SelfWrites::default();

    notes_switch::set(&root.path, STANDUP, false, &writes).unwrap();

    let meeting = root.read_meeting();
    assert!(meeting.problems.is_empty(), "{:?}", meeting.problems);
    assert_eq!(agent_notes_value(&meeting).as_deref(), Some("off"));
    assert!(notes_switch::is_off(&meeting));
    assert_eq!(meeting.id().as_deref(), Some(STANDUP));
    assert_eq!(meeting.title().as_deref(), Some("Standup"));
    assert!(!meeting.is_analyzed());
    for heading in store::meeting::SECTIONS {
        assert_eq!(
            meeting.section(heading).map(str::trim),
            Some(""),
            "{heading}"
        );
    }
    let raw = fs::read_to_string(dir.join(MEETING_FILE)).unwrap();
    assert!(raw.contains("\nagent_notes: \"off\"\n"), "{raw}");
    // The rest of the folder is untouched, and the write is the app's own.
    assert_eq!(fs::read_to_string(dir.join(NOTES_FILE)).unwrap(), NOTES);
    assert_eq!(
        fs::read_to_string(dir.join(TRANSCRIPT_FILE)).unwrap(),
        TRANSCRIPT
    );
    let meeting_md = dunce::canonicalize(dir.join(MEETING_FILE)).unwrap();
    assert!(writes.is_suppressed(&meeting_md, Instant::now()));
}

#[test]
fn off_keeps_every_other_key_and_the_sections() {
    let root = Root::new("off-keeps");
    let dir = root.meeting(STANDUP);
    fs::write(dir.join(MEETING_FILE), WRAPPED_UP).unwrap();
    let before = Meeting::parse(WRAPPED_UP);

    root.set(false);

    let after = root.read_meeting();
    assert!(notes_switch::is_off(&after));
    let mut expected_keys = before.frontmatter.keys();
    expected_keys.push(AGENT_NOTES_KEY.to_owned());
    assert_eq!(after.frontmatter.keys(), expected_keys);
    for key in before.frontmatter.keys() {
        assert_eq!(
            after.frontmatter.get(&key),
            before.frontmatter.get(&key),
            "{key}"
        );
    }
    assert_eq!(after.preamble, before.preamble);
    assert_eq!(after.sections, before.sections);
}

#[test]
fn on_removes_the_key_and_gives_back_the_file_it_was() {
    let root = Root::new("on-removes");
    let dir = root.meeting(STANDUP);
    fs::write(dir.join(MEETING_FILE), WRAPPED_UP).unwrap();
    let before = Meeting::parse(WRAPPED_UP);

    root.set(false);
    root.set(true);

    let after = root.read_meeting();
    assert!(!notes_switch::is_off(&after));
    assert_eq!(agent_notes_value(&after), None);
    assert_eq!(after.frontmatter, before.frontmatter);
    assert_eq!(after.preamble, before.preamble);
    assert_eq!(after.sections, before.sections);
}

#[test]
fn on_also_clears_an_agent_notes_false_written_by_hand() {
    let root = Root::new("on-false");
    let dir = root.meeting(STANDUP);
    let raw = WRAPPED_UP.replacen("x_custom:", "agent_notes: false\nx_custom:", 1);
    fs::write(dir.join(MEETING_FILE), &raw).unwrap();
    assert!(notes_switch::is_off(&root.read_meeting()));

    root.set(true);

    assert!(!notes_switch::is_off(&root.read_meeting()));
}

#[test]
fn on_with_no_meeting_md_writes_nothing() {
    let root = Root::new("on-nothing");
    let dir = root.meeting(STANDUP);
    let writes = SelfWrites::default();

    notes_switch::set(&root.path, STANDUP, true, &writes).unwrap();

    assert!(!dir.join(MEETING_FILE).exists());
    let mut names: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(names, [NOTES_FILE, TRANSCRIPT_FILE]);
}

#[test]
fn on_when_already_on_leaves_meeting_md_alone() {
    let root = Root::new("on-again");
    let dir = root.meeting(STANDUP);
    fs::write(dir.join(MEETING_FILE), WRAPPED_UP).unwrap();
    let writes = SelfWrites::default();

    notes_switch::set(&root.path, STANDUP, true, &writes).unwrap();

    assert_eq!(
        fs::read_to_string(dir.join(MEETING_FILE)).unwrap(),
        WRAPPED_UP
    );
    let meeting_md = dunce::canonicalize(dir.join(MEETING_FILE)).unwrap();
    assert!(!writes.is_suppressed(&meeting_md, Instant::now()));
}

#[test]
fn off_twice_writes_once() {
    let root = Root::new("off-twice");
    let dir = root.meeting(STANDUP);
    root.set(false);
    let first = fs::read(dir.join(MEETING_FILE)).unwrap();
    let writes = SelfWrites::default();

    notes_switch::set(&root.path, STANDUP, false, &writes).unwrap();

    assert_eq!(fs::read(dir.join(MEETING_FILE)).unwrap(), first);
    let meeting_md = dunce::canonicalize(dir.join(MEETING_FILE)).unwrap();
    assert!(
        !writes.is_suppressed(&meeting_md, Instant::now()),
        "nothing changed, so nothing was written"
    );
}

#[test]
fn a_meeting_md_with_broken_frontmatter_is_refused_and_left_byte_identical() {
    for on in [false, true] {
        let root = Root::new("broken");
        let dir = root.meeting(STANDUP);
        let bad = b"---\nid: [unclosed\n---\n\n## Summary\n\nThe user's own words.\n";
        fs::write(dir.join(MEETING_FILE), bad).unwrap();

        let error = notes_switch::set(&root.path, STANDUP, on, &SelfWrites::default())
            .expect_err("a meeting.md that cannot be written back must be refused");

        assert!(
            matches!(error, Error::Frontmatter { .. }),
            "on: {on}: expected Error::Frontmatter, got {error:?}"
        );
        assert_eq!(fs::read(dir.join(MEETING_FILE)).unwrap(), bad);
    }
}

#[test]
fn a_meeting_md_that_is_not_utf8_is_refused_and_left_byte_identical() {
    let root = Root::new("not-utf8");
    let dir = root.meeting(STANDUP);
    let bad = b"---\nid: 2026-09-01-1430-standup\n---\n\n## Summary\n\n\xff\xfe broken\n";
    fs::write(dir.join(MEETING_FILE), bad).unwrap();

    let error = notes_switch::set(&root.path, STANDUP, false, &SelfWrites::default())
        .expect_err("a meeting.md that is not UTF-8 must be refused");

    match error {
        Error::Io(io) => assert_eq!(io.kind(), std::io::ErrorKind::InvalidData),
        other => panic!("expected an InvalidData Io error, got {other:?}"),
    }
    assert_eq!(fs::read(dir.join(MEETING_FILE)).unwrap(), bad);
}

#[test]
fn a_missing_folder_or_a_bad_id_is_an_error_and_creates_nothing() {
    let root = Root::new("missing");
    match notes_switch::set(&root.path, STANDUP, false, &SelfWrites::default()) {
        Err(Error::Io(io)) => assert_eq!(io.kind(), std::io::ErrorKind::NotFound),
        other => panic!("expected a NotFound Io error, got {other:?}"),
    }
    assert!(!root.path.join(STANDUP).exists());
    for hostile in ["..", "../elsewhere", ".app"] {
        let result = notes_switch::set(&root.path, hostile, false, &SelfWrites::default());
        assert!(
            matches!(&result, Err(Error::BadId(id)) if id == hostile),
            "{hostile:?}: expected BadId, got {result:?}"
        );
    }
    assert!(!root.path.join(MEETING_FILE).exists());
}

#[test]
fn after_off_a_notes_write_writes_nothing() {
    let root = Root::new("off-then-write");
    let dir = root.meeting(STANDUP);
    root.set(false);
    let before = fs::read(dir.join(MEETING_FILE)).unwrap();

    let outcome = root.write_notes();

    assert!(outcome.notes_off);
    assert!(outcome.written.is_empty());
    assert_eq!(fs::read(dir.join(MEETING_FILE)).unwrap(), before);
    assert!(!dir.join(TICKETS_DIR).exists());
}

#[test]
fn off_then_on_lets_the_notes_be_written() {
    let root = Root::new("off-on-write");
    let dir = root.meeting(STANDUP);
    root.set(false);
    root.set(true);

    let outcome = root.write_notes();

    assert!(!outcome.notes_off);
    assert!(!outcome.written.is_empty());
    let meeting = root.read_meeting();
    assert!(meeting.is_analyzed());
    assert!(!meeting.section("Summary").unwrap().trim().is_empty());
    assert!(dir.join(TICKETS_DIR).is_dir());
}

#[test]
fn off_survives_a_rescan_and_a_rebuilt_index() {
    let root = Root::new("rebuild");
    let dir = root.meeting(STANDUP);
    fs::write(dir.join(MEETING_FILE), WRAPPED_UP).unwrap();
    drop(Index::open(&root.path).unwrap());
    root.set(false);
    let written = fs::read(dir.join(MEETING_FILE)).unwrap();

    // L7: the index is derived. Delete it and build it again from markdown.
    let index = index_path(&root.path);
    assert!(index.is_file());
    fs::remove_file(&index).unwrap();
    let mut rebuilt = Index::open(&root.path).unwrap();
    rebuilt.rescan(&root.path).unwrap();
    drop(rebuilt);

    assert_eq!(fs::read(dir.join(MEETING_FILE)).unwrap(), written);
    let scanned = store::folder::scan(&root.path).unwrap();
    let folder = scanned.iter().find(|f| f.id == STANDUP).unwrap();
    assert!(notes_switch::is_off(folder.meeting.as_ref().unwrap()));
    let loaded = store::folder::load(&dir).unwrap();
    assert!(notes_switch::is_off(loaded.meeting.as_ref().unwrap()));
}
