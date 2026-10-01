//! The TUR-99 gate, run over a real meetings folder on disk.
//!
//! `tests/fixtures/meetings/` holds three meetings, one per failure mode the
//! ticket names:
//!
//! * `2026-09-01-1430-standup` — healthy, but its `meeting.md` and one ticket
//!   carry frontmatter keys this crate has never heard of (a nested map, a
//!   list, strings that look like numbers), plus an extra `## Links` section.
//! * `2026-09-02-1000-retro` — `meeting.md` has no `## Open Questions`, and
//!   one transcript line has a speaker §3.4 does not allow.
//! * `2026-09-03-0900-planning` — plain broken: unparseable YAML in
//!   `meeting.md`, one ticket with `status: maybe`, one with no frontmatter.
//!
//! plus a `.app/` folder that must not be mistaken for a meeting.
//!
//! Every test works on its own copy under the OS temp dir, so the checked-in
//! fixtures are never written to, and only the crate's public API is used.

use std::fs;
use std::path::{Path, PathBuf};

use store::folder::{self, FileProblem, MeetingFolder};
use store::meeting::Meeting;
use store::ticket::Ticket;
use store::transcript::{self, Speaker};
use store::{Error, MEETING_FILE, Problem, TICKETS_DIR, TRANSCRIPT_FILE, notes};

const STANDUP: &str = "2026-09-01-1430-standup";
const RETRO: &str = "2026-09-02-1000-retro";
const PLANNING: &str = "2026-09-03-0900-planning";

/// A throwaway copy of the fixture root, removed on drop.
struct FixtureCopy {
    root: PathBuf,
    _guard: tempfile::TempDir,
}

impl FixtureCopy {
    /// `name` prefixes the temp dir so a leftover is easy to trace.
    fn new(name: &str) -> Self {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("meetings");
        let guard = tempfile::Builder::new()
            .prefix(&format!("meet-ai-store-fixture-{name}-"))
            .tempdir()
            .unwrap();
        let root = guard.path().to_path_buf();
        copy_dir(&source, &root);
        Self {
            root,
            _guard: guard,
        }
    }

    fn dir(&self, id: &str) -> PathBuf {
        folder::meeting_dir(&self.root, id).unwrap()
    }

    fn load(&self, id: &str) -> MeetingFolder {
        folder::load(&self.dir(id)).unwrap()
    }
}

/// Std has no recursive copy for directories.
fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), &dest).unwrap();
        }
    }
}

/// The problems recorded against one file, in order.
fn problems_for<'a>(folder: &'a MeetingFolder, file: &str) -> Vec<&'a Problem> {
    folder
        .problems
        .iter()
        .filter(|p| p.file == file)
        .map(|p| &p.problem)
        .collect()
}

/// A file that loaded like this has nothing trustworthy to write back.
fn is_unwritable(folder: &MeetingFolder, file: &str) -> bool {
    problems_for(folder, file).iter().any(|p| {
        matches!(
            p,
            Problem::BadFrontmatter { .. } | Problem::Unreadable { .. }
        )
    })
}

/// The ticket file names in a meeting, in the order `folder::load` returns
/// its tickets (sorted, dotfiles and non-`.md` skipped).
fn ticket_files(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir.join(TICKETS_DIR)) else {
        return Vec::new();
    };
    let mut names: Vec<_> = entries
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.') && n.ends_with(".md"))
        .collect();
    names.sort();
    names
}

#[test]
fn scan_finds_the_three_meetings_newest_first_and_skips_dot_app() {
    let fixture = FixtureCopy::new("scan");
    let ids: Vec<_> = folder::scan(&fixture.root)
        .unwrap()
        .into_iter()
        .map(|f| f.id)
        .collect();
    assert_eq!(ids, [PLANNING, RETRO, STANDUP]);
}

#[test]
fn the_broken_meeting_loads_flagged_without_stopping_the_other_two() {
    let fixture = FixtureCopy::new("broken");
    let folders = folder::scan(&fixture.root).unwrap();
    assert_eq!(
        folders.len(),
        3,
        "the broken folder must not hide the others"
    );

    let broken = folders.iter().find(|f| f.id == PLANNING).unwrap();
    assert!(broken.needs_attention());
    assert!(broken.meeting.is_some(), "a broken meeting.md still loads");
    assert!(
        problems_for(broken, MEETING_FILE)
            .iter()
            .any(|p| matches!(p, Problem::BadFrontmatter { .. })),
        "meeting.md must be flagged: {:?}",
        broken.problems
    );

    // Its healthy parts load as if nothing were wrong next to them.
    let transcript = broken.transcript.as_ref().unwrap();
    assert_eq!(transcript.lines.len(), 2);
    assert!(transcript.problems.is_empty());
    assert_eq!(broken.notes, "Search before sync.\n");

    // Both broken tickets load too, each flagged against its own file.
    assert_eq!(broken.tickets.len(), 2);
    assert_eq!(broken.tickets[0].id().as_deref(), Some("TICK-0003"));
    assert!(
        problems_for(broken, "tickets/TICK-0003.md")
            .iter()
            .any(|p| matches!(p, Problem::BadField { key, .. } if key == "status"))
    );
    assert!(broken.problems.contains(&FileProblem {
        file: "tickets/TICK-0004.md".into(),
        problem: Problem::NoFrontmatter,
    }));

    for id in [RETRO, STANDUP] {
        let other = folders.iter().find(|f| f.id == id).unwrap();
        assert!(
            other.meeting.is_some(),
            "{id} must still load its meeting.md"
        );
        assert!(
            other.transcript.is_some(),
            "{id} must still load its transcript"
        );
    }
}

#[test]
fn a_missing_section_and_a_bad_transcript_line_are_flagged_and_the_rest_reads() {
    let fixture = FixtureCopy::new("missing-section");
    let retro = fixture.load(RETRO);

    assert_eq!(
        retro.problems,
        [
            FileProblem {
                file: MEETING_FILE.into(),
                problem: Problem::MissingSection {
                    heading: "Open Questions".into()
                },
            },
            FileProblem {
                file: TRANSCRIPT_FILE.into(),
                problem: Problem::UnparsedLines { count: 1 },
            },
        ]
    );

    let meeting = retro.meeting.as_ref().unwrap();
    assert!(meeting.section("Summary").unwrap().contains("flaky tests"));
    assert!(meeting.section("Decisions").unwrap().contains("Quarantine"));
    assert!(meeting.section("Action Items").unwrap().contains("Priya"));
    assert_eq!(meeting.section("Open Questions"), None);
    assert_eq!(meeting.title().as_deref(), Some("Sprint Retro"));

    // The bad line is skipped, not half-parsed, and `seq` still counts it.
    let transcript = retro.transcript.as_ref().unwrap();
    let seqs: Vec<_> = transcript.lines.iter().map(|l| l.seq).collect();
    assert_eq!(seqs, [0, 2]);

    assert_eq!(retro.notes, "", "no notes.md is an empty page");
    assert!(retro.tickets.is_empty(), "no tickets/ folder is no tickets");
}

#[test]
fn the_healthy_meeting_needs_no_attention_despite_unknown_keys() {
    let fixture = FixtureCopy::new("healthy");
    let standup = fixture.load(STANDUP);

    assert!(!standup.needs_attention(), "{:?}", standup.problems);
    let meeting = standup.meeting.as_ref().unwrap();
    assert_eq!(meeting.id().as_deref(), Some(STANDUP));
    assert_eq!(meeting.duration_sec(), Some(2714));
    assert_eq!(meeting.attendees(), ["Shantanu", "Priya", "Dev"]);
    assert!(meeting.is_analyzed());
    assert!(meeting.section("Links").unwrap().contains("api-runbook"));
    assert_eq!(standup.transcript.as_ref().unwrap().lines.len(), 3);
    assert_eq!(standup.tickets.len(), 2);
    assert!(standup.notes.starts_with("# My notes"));
}

#[test]
fn every_well_formed_file_survives_a_write_and_reload() {
    let fixture = FixtureCopy::new("round-trip");
    let mut meetings_written = 0;
    let mut tickets_written = 0;

    for loaded in folder::scan(&fixture.root).unwrap() {
        if let Some(meeting) = &loaded.meeting
            && !is_unwritable(&loaded, MEETING_FILE)
        {
            let path = loaded.path.join(MEETING_FILE);
            meeting.write(&path).unwrap();
            let again = Meeting::read(&path).unwrap().unwrap();
            let file = format!("{}/{MEETING_FILE}", loaded.id);
            assert_eq!(
                again.frontmatter.keys(),
                meeting.frontmatter.keys(),
                "{file}"
            );
            for key in meeting.frontmatter.keys() {
                assert_eq!(
                    again.frontmatter.get(&key),
                    meeting.frontmatter.get(&key),
                    "{file}: {key}"
                );
            }
            assert_eq!(again.frontmatter, meeting.frontmatter, "{file}");
            assert_eq!(again.preamble, meeting.preamble, "{file}");
            assert_eq!(again.sections, meeting.sections, "{file}");
            meetings_written += 1;
        }

        let names = ticket_files(&loaded.path);
        assert_eq!(names.len(), loaded.tickets.len(), "{}", loaded.id);
        for (name, ticket) in names.iter().zip(&loaded.tickets) {
            if is_unwritable(&loaded, &format!("{TICKETS_DIR}/{name}")) {
                continue;
            }
            let path = loaded.path.join(TICKETS_DIR).join(name);
            ticket.write(&path).unwrap();
            let again = Ticket::read(&path).unwrap();
            let file = format!("{}/{TICKETS_DIR}/{name}", loaded.id);
            assert_eq!(
                again.frontmatter.keys(),
                ticket.frontmatter.keys(),
                "{file}"
            );
            assert_eq!(again.frontmatter, ticket.frontmatter, "{file}");
            assert_eq!(again.body, ticket.body, "{file}");
            tickets_written += 1;
        }
    }

    // Standup and retro; planning's meeting.md is refused. All four tickets,
    // including the one with a bad status and the one with no frontmatter.
    assert_eq!(meetings_written, 2);
    assert_eq!(tickets_written, 4);

    // Writing everything back must not have made the healthy meeting sick.
    assert!(!fixture.load(STANDUP).needs_attention());
}

#[test]
fn unknown_frontmatter_keys_are_still_in_the_file_after_a_rewrite() {
    let fixture = FixtureCopy::new("unknown-keys");
    let standup = fixture.load(STANDUP);

    let meeting_path = standup.path.join(MEETING_FILE);
    standup
        .meeting
        .as_ref()
        .unwrap()
        .write(&meeting_path)
        .unwrap();
    let text = fs::read_to_string(&meeting_path).unwrap();
    for key in [
        "calendar_event_id:",
        "agent_run:",
        "prompt_version:",
        "follow_ups:",
        "transcript_ref:",
    ] {
        assert!(text.contains(key), "{key} was dropped:\n{text}");
    }
    let again = Meeting::read(&meeting_path).unwrap().unwrap();
    // Strings that look like numbers must stay strings, or `"123"` comes back
    // as the integer 123 and a calendar lookup silently stops matching.
    assert_eq!(
        again.frontmatter.get_str("calendar_event_id").as_deref(),
        Some("123")
    );
    assert_eq!(again.frontmatter.get_i64("calendar_event_id"), None);
    assert_eq!(
        again.frontmatter.get_str("transcript_ref").as_deref(),
        Some("00:14:22")
    );

    let ticket_path = standup.path.join(TICKETS_DIR).join("TICK-0002.md");
    standup.tickets[1].write(&ticket_path).unwrap();
    let text = fs::read_to_string(&ticket_path).unwrap();
    for key in ["priority:", "labels:"] {
        assert!(text.contains(key), "{key} was dropped:\n{text}");
    }
    let again = Ticket::read(&ticket_path).unwrap();
    assert_eq!(
        again.frontmatter.get_str("priority").as_deref(),
        Some("high")
    );
    assert_eq!(
        again.frontmatter.get_str_list("labels"),
        Some(vec!["backend".to_string(), "auth".to_string()])
    );
}

#[test]
fn notes_survive_a_write_and_reload() {
    let fixture = FixtureCopy::new("notes");
    let body = "# Retro notes\n\n- Flaky: `login_flow`, `sync_retry`.\n";
    for id in [STANDUP, RETRO] {
        // Standup overwrites an existing notes.md; retro has none yet.
        let dir = fixture.dir(id);
        notes::write(&dir, body).unwrap();
        assert_eq!(notes::read(&dir).unwrap(), body, "{id}");
        assert_eq!(fixture.load(id).notes, body, "{id}");
    }
}

#[test]
fn a_line_the_stt_sink_appends_reads_back_and_leaves_earlier_lines_alone() {
    use stt::TranscriptSink;

    // stt's sink is the only writer of transcript.md (SPEC §3.4); what this
    // crate owes it is reading the result, fixture lines and new line alike.
    let fixture = FixtureCopy::new("append");
    let path = fixture.dir(STANDUP).join(TRANSCRIPT_FILE);
    let before_text = fs::read_to_string(&path).unwrap();
    let before = transcript::read(&path).unwrap().unwrap();

    let mut sink = stt::MarkdownSink::create(&path).unwrap();
    sink.write(&stt::Utterance {
        start_sec: 900,
        speaker: stt::Speaker::You,
        text: stt::collapse_whitespace("  Sounds   good,\tship it. ").unwrap(),
    })
    .unwrap();
    sink.flush().unwrap();
    drop(sink);

    let after_text = fs::read_to_string(&path).unwrap();
    assert!(after_text.starts_with(&before_text), "append-only (§3.4)");
    let after = transcript::read(&path).unwrap().unwrap();
    assert!(after.problems.is_empty());
    assert_eq!(after.lines.len(), before.lines.len() + 1);
    assert_eq!(after.lines[..before.lines.len()], before.lines[..]);
    let last = after.lines.last().unwrap();
    assert_eq!(last.time, "00:15:00");
    assert_eq!(last.start_sec, 900);
    assert_eq!(last.speaker, Speaker::You);
    assert_eq!(last.text, "Sounds good, ship it.");
}

#[test]
fn broken_frontmatter_is_refused_on_write_and_the_file_is_untouched() {
    let fixture = FixtureCopy::new("refuse");
    let planning = fixture.load(PLANNING);
    let path = planning.path.join(MEETING_FILE);
    let before = fs::read(&path).unwrap();

    let meeting = planning.meeting.as_ref().unwrap();
    assert!(matches!(meeting.render(), Err(Error::Frontmatter { .. })));
    assert!(matches!(
        meeting.write(&path),
        Err(Error::Frontmatter { .. })
    ));
    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "meeting.md must be byte-identical"
    );

    let leftovers: Vec<_> = fs::read_dir(&planning.path)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with('.'))
        .collect();
    assert!(
        leftovers.is_empty(),
        "a refused write left {leftovers:?} behind"
    );
}
