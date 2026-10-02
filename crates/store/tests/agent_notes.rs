//! TUR-7: `meeting.md` sections and `tickets/TICK-NNNN.md` files written from
//! the agent's notes JSON, checked on a real folder on disk.
//!
//! The notes come from `tests/fixtures/notes/*.json` and go through
//! `Notes::from_json`, the same strict check the app runs, so a fixture that
//! drifts from the schema fails here first. Every test builds its own meetings
//! root under the OS temp dir and uses only the crate's public API.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use prompts::notes::Notes;
use store::agent_notes::{self, AGENT_TICKETS_KEY, Analysis, AnalyzedBy, Outcome};
use store::meeting::Meeting;
use store::ticket::{Status, Ticket};
use store::watcher::SelfWrites;
use store::{Error, MEETING_FILE, NOTES_FILE, TICKETS_DIR, TRANSCRIPT_FILE};
use yaml_rust2::Yaml;

const STANDUP: &str = "2026-09-01-1430-standup";
const RETRO: &str = "2026-09-02-1000-retro";
const PLANNING: &str = "2026-09-03-0900-planning";

const TRANSCRIPT: &str = "\
[00:00:04] Others: Morning everyone, let's start with the API work.
[00:14:22] You: Sessions are still in memory, that's the blocker.
[00:21:05] Others: Then Redis it is: I'll take the load test.
";
const NOTES: &str = "# My notes\n\n- Redis first, then the load test.\n";

const FIRST_AT: &str = "2026-09-01T15:32:00+05:30";
const SECOND_AT: &str = "2026-09-02T09:10:00+05:30";

/// A throwaway meetings root, removed on drop.
struct Root {
    path: PathBuf,
    _guard: tempfile::TempDir,
}

impl Root {
    /// `name` prefixes the temp dir so a leftover is easy to trace.
    fn new(name: &str) -> Self {
        let guard = tempfile::Builder::new()
            .prefix(&format!("meet-ai-store-agent-notes-{name}-"))
            .tempdir()
            .unwrap();
        Self {
            path: guard.path().to_path_buf(),
            _guard: guard,
        }
    }

    /// A meeting folder with a `transcript.md` and a `notes.md`, and no
    /// `meeting.md` yet (nobody has wrapped it up).
    fn meeting(&self, id: &str) -> PathBuf {
        let dir = self.path.join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(TRANSCRIPT_FILE), TRANSCRIPT).unwrap();
        fs::write(dir.join(NOTES_FILE), NOTES).unwrap();
        dir
    }

    fn ticket_path(&self, meeting: &str, id: &str) -> PathBuf {
        self.path
            .join(meeting)
            .join(TICKETS_DIR)
            .join(format!("{id}.md"))
    }

    fn ticket(&self, meeting: &str, id: &str) -> Ticket {
        Ticket::read(&self.ticket_path(meeting, id)).unwrap()
    }

    fn read_meeting(&self, meeting: &str) -> Meeting {
        Meeting::read(&self.path.join(meeting).join(MEETING_FILE))
            .unwrap()
            .expect("meeting.md was written")
    }

    /// Ticket file names in one meeting's `tickets/`, sorted.
    fn ticket_files(&self, meeting: &str) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.path.join(meeting).join(TICKETS_DIR))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| !name.starts_with('.'))
            .collect();
        names.sort();
        names
    }

    fn write(&self, meeting: &str, notes: &Notes, analysis: &Analysis) -> Outcome {
        agent_notes::write(&self.path, meeting, notes, analysis, &SelfWrites::default()).unwrap()
    }
}

fn notes(name: &str) -> Notes {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("notes")
        .join(format!("{name}.json"));
    Notes::from_json(&fs::read_to_string(path).unwrap()).unwrap()
}

fn analysis(by: AnalyzedBy, at: &str) -> Analysis {
    Analysis {
        by,
        model: "opus".to_owned(),
        at: at.to_owned(),
    }
}

fn first() -> Analysis {
    analysis(AnalyzedBy::ClaudeCode, FIRST_AT)
}

fn second() -> Analysis {
    analysis(AnalyzedBy::ClaudeCode, SECOND_AT)
}

fn ids(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

/// A section's body without the blank lines around it.
fn section(meeting: &Meeting, heading: &str) -> String {
    meeting
        .section(heading)
        .unwrap_or_else(|| panic!("no ## {heading}"))
        .trim()
        .to_owned()
}

// ---------------------------------------------------------------------------
// 1. A fresh write
// ---------------------------------------------------------------------------

#[test]
fn a_fresh_write_creates_meeting_md_with_the_four_sections_filled() {
    let root = Root::new("fresh-sections");
    let dir = root.meeting(STANDUP);
    assert!(!dir.join(MEETING_FILE).exists());

    let outcome = root.write(STANDUP, &notes("standup"), &first());

    assert_eq!(outcome.meeting, dir.join(MEETING_FILE));
    let meeting = root.read_meeting(STANDUP);
    assert!(meeting.problems.is_empty(), "{:?}", meeting.problems);
    assert_eq!(meeting.id().as_deref(), Some(STANDUP));
    assert_eq!(meeting.title().as_deref(), Some("Standup"));
    assert_eq!(
        section(&meeting, "Summary"),
        "Sessions still live in memory, which blocks the second API instance. \
         The team agreed to move them to Redis and load test the login path first."
    );
    assert_eq!(
        section(&meeting, "Decisions"),
        "- Move sessions to Redis.\n- Keep the old login path until the load test passes."
    );
    assert_eq!(
        section(&meeting, "Action Items"),
        "- TICK-0001: Redis session store (Shantanu, due Friday)\n\
         - TICK-0002: Load test the login path (Priya)\n\
         - TICK-0003: Write the cutover runbook"
    );
    assert_eq!(
        section(&meeting, "Open Questions"),
        "- Do we need sticky sessions during the cutover?\n\
         - Who owns the Redis cluster after launch?"
    );
}

#[test]
fn a_fresh_write_records_who_analyzed_the_meeting_with_what_and_when() {
    let root = Root::new("fresh-analysis");
    root.meeting(STANDUP);
    root.write(STANDUP, &notes("standup"), &first());

    let meeting = root.read_meeting(STANDUP);
    let fm = &meeting.frontmatter;
    assert_eq!(fm.get_str("analyzed_by").as_deref(), Some("claude-code"));
    assert_eq!(fm.get_str("analyzed_model").as_deref(), Some("opus"));
    assert_eq!(fm.get_str("analyzed_at").as_deref(), Some(FIRST_AT));
    assert!(meeting.is_analyzed());
}

#[test]
fn a_fresh_write_by_codex_says_codex() {
    let root = Root::new("fresh-codex");
    root.meeting(STANDUP);
    let codex = Analysis {
        by: AnalyzedBy::Codex,
        model: "gpt-5-codex".to_owned(),
        at: FIRST_AT.to_owned(),
    };
    root.write(STANDUP, &notes("standup"), &codex);

    let meeting = root.read_meeting(STANDUP);
    assert_eq!(
        meeting.frontmatter.get_str("analyzed_by").as_deref(),
        Some("codex")
    );
    assert_eq!(
        meeting.frontmatter.get_str("analyzed_model").as_deref(),
        Some("gpt-5-codex")
    );
}

#[test]
fn a_fresh_write_makes_one_open_ticket_per_task_numbered_in_task_order() {
    let root = Root::new("fresh-tickets");
    root.meeting(STANDUP);
    let notes = notes("standup");

    let outcome = root.write(STANDUP, &notes, &first());

    assert_eq!(
        outcome.written,
        ids(&["TICK-0001", "TICK-0002", "TICK-0003"])
    );
    assert!(outcome.kept.is_empty());
    assert!(outcome.removed.is_empty());
    assert_eq!(
        root.ticket_files(STANDUP),
        ["TICK-0001.md", "TICK-0002.md", "TICK-0003.md"]
    );

    for (id, task) in outcome.written.iter().zip(&notes.tasks) {
        let ticket = root.ticket(STANDUP, id);
        assert!(ticket.problems.is_empty(), "{id}: {:?}", ticket.problems);
        assert_eq!(ticket.id().as_ref(), Some(id));
        assert_eq!(ticket.title(), Some(task.title.clone()));
        assert_eq!(ticket.meeting().as_deref(), Some(STANDUP));
        assert_eq!(ticket.status(), Some(Status::Open));
        assert_eq!(ticket.assignee(), task.owner);
        assert_eq!(ticket.transcript_ref(), Some(task.transcript_ref.clone()));
        assert_eq!(ticket.synced_to(), None);
        assert!(
            ticket.body.contains(&task.details),
            "{id} body {:?} lacks the details",
            ticket.body
        );
    }
}

#[test]
fn a_task_with_no_owner_gets_a_null_assignee() {
    let root = Root::new("fresh-null-owner");
    root.meeting(STANDUP);
    root.write(STANDUP, &notes("standup"), &first());

    let ticket = root.ticket(STANDUP, "TICK-0003");
    assert_eq!(ticket.frontmatter.get("assignee"), Some(&Yaml::Null));
}

#[test]
fn the_due_date_goes_in_the_ticket_body_not_the_frontmatter() {
    let root = Root::new("fresh-due");
    root.meeting(STANDUP);
    root.write(STANDUP, &notes("standup"), &first());

    let with_due = root.ticket(STANDUP, "TICK-0001");
    assert_eq!(
        with_due.body,
        "\nReplace the in-memory session map with a store backed by Redis.\n\nDue: Friday.\n"
    );
    assert_eq!(with_due.frontmatter.get("due"), None);

    let without_due = root.ticket(STANDUP, "TICK-0002");
    assert_eq!(
        without_due.body,
        "\nRun the login load test against two API instances behind the balancer.\n"
    );
    assert_eq!(without_due.frontmatter.get("due"), None);
}

#[test]
fn a_fresh_write_leaves_notes_and_transcript_byte_identical() {
    let root = Root::new("fresh-untouched");
    let dir = root.meeting(STANDUP);
    let notes_before = fs::read(dir.join(NOTES_FILE)).unwrap();
    let transcript_before = fs::read(dir.join(TRANSCRIPT_FILE)).unwrap();

    root.write(STANDUP, &notes("standup"), &first());
    root.write(STANDUP, &notes("standup-rerun"), &second());

    assert_eq!(fs::read(dir.join(NOTES_FILE)).unwrap(), notes_before);
    assert_eq!(
        fs::read(dir.join(TRANSCRIPT_FILE)).unwrap(),
        transcript_before
    );
}

/// A `meeting.md` that is already there, with keys and a section this crate
/// has never heard of.
const EXISTING_MEETING: &str = "\
---
id: 2026-09-01-1430-standup
title: Platform Standup
date: 2026-09-01T14:30:00+05:30
attendees: [Shantanu, Priya, Dev]
calendar_event_id: \"123\"
agent_run:
  model: claude-opus
  prompt_version: 3
  tools: [read, write]
follow_ups:
  - 2026-09-08
  - 2026-09-15
---

## Summary

An old summary that the notes run replaces.

## Decisions

## Action Items

## Open Questions

## Links

- https://example.com/api-runbook
";

#[test]
fn an_existing_meeting_md_keeps_its_unknown_keys_and_extra_sections() {
    let root = Root::new("existing-meeting");
    let dir = root.meeting(STANDUP);
    fs::write(dir.join(MEETING_FILE), EXISTING_MEETING).unwrap();

    root.write(STANDUP, &notes("standup"), &first());

    let meeting = root.read_meeting(STANDUP);
    assert!(meeting.problems.is_empty(), "{:?}", meeting.problems);
    let fm = &meeting.frontmatter;
    // The user's title is not replaced by one made from the folder name.
    assert_eq!(meeting.title().as_deref(), Some("Platform Standup"));
    assert_eq!(fm.get_str("calendar_event_id").as_deref(), Some("123"));
    assert_eq!(meeting.attendees(), ["Shantanu", "Priya", "Dev"]);
    match fm.get("agent_run") {
        Some(Yaml::Hash(map)) => {
            assert_eq!(
                map.get(&Yaml::String("model".into())),
                Some(&Yaml::String("claude-opus".into()))
            );
            assert_eq!(
                map.get(&Yaml::String("prompt_version".into())),
                Some(&Yaml::Integer(3))
            );
            assert_eq!(
                map.get(&Yaml::String("tools".into())),
                Some(&Yaml::Array(vec![
                    Yaml::String("read".into()),
                    Yaml::String("write".into())
                ]))
            );
        }
        other => panic!("agent_run is no longer a map: {other:?}"),
    }
    assert_eq!(
        fm.get_str_list("follow_ups").unwrap(),
        ["2026-09-08", "2026-09-15"]
    );
    assert_eq!(fm.get_str("analyzed_by").as_deref(), Some("claude-code"));

    assert_eq!(
        section(&meeting, "Links"),
        "- https://example.com/api-runbook"
    );
    assert!(
        !section(&meeting, "Summary").contains("old summary"),
        "the summary was not replaced"
    );
    let headings: Vec<&str> = meeting
        .sections
        .iter()
        .map(|s| s.heading.as_str())
        .collect();
    assert_eq!(
        headings,
        [
            "Summary",
            "Decisions",
            "Action Items",
            "Open Questions",
            "Links"
        ]
    );
}

#[test]
fn empty_lists_are_written_as_none() {
    let root = Root::new("empty-lists");
    root.meeting(STANDUP);
    let mut notes = notes("standup-rerun");
    notes.decisions.clear();
    notes.tasks.clear();

    let outcome = root.write(STANDUP, &notes, &first());

    assert!(outcome.written.is_empty());
    let meeting = root.read_meeting(STANDUP);
    assert_eq!(section(&meeting, "Decisions"), "None.");
    assert_eq!(section(&meeting, "Action Items"), "None.");
    assert_eq!(section(&meeting, "Open Questions"), "None.");
    assert_eq!(meeting.frontmatter.get(AGENT_TICKETS_KEY), None);
    assert!(!root.path.join(STANDUP).join(TICKETS_DIR).exists());
}

// ---------------------------------------------------------------------------
// 2. Re-running with untouched tickets
// ---------------------------------------------------------------------------

#[test]
fn a_rerun_replaces_untouched_tickets_in_place_and_the_sections() {
    let root = Root::new("rerun-replace");
    root.meeting(STANDUP);
    let mut three = notes("standup-rerun");
    three.tasks.push(notes("standup").tasks[2].clone());
    root.write(STANDUP, &notes("standup"), &first());

    let outcome = root.write(STANDUP, &three, &second());

    assert_eq!(
        outcome.written,
        ids(&["TICK-0001", "TICK-0002", "TICK-0003"])
    );
    assert!(outcome.kept.is_empty());
    assert!(outcome.removed.is_empty());
    let ticket = root.ticket(STANDUP, "TICK-0001");
    assert_eq!(
        ticket.title().as_deref(),
        Some("Ship the Redis session store")
    );
    assert_eq!(ticket.assignee().as_deref(), Some("Dev"));
    assert_eq!(ticket.transcript_ref().as_deref(), Some("00:15:00"));
    assert!(ticket.body.contains("Due: Thursday."));
    assert_eq!(
        root.ticket(STANDUP, "TICK-0002").title().as_deref(),
        Some("Load test with two instances")
    );
    assert_eq!(root.ticket(STANDUP, "TICK-0002").assignee(), None);

    let meeting = root.read_meeting(STANDUP);
    assert_eq!(
        meeting.frontmatter.get_str("analyzed_at").as_deref(),
        Some(SECOND_AT)
    );
    assert_eq!(
        section(&meeting, "Summary"),
        "Second pass: sessions move to Redis this week, and the load test gates the switch."
    );
    assert_eq!(section(&meeting, "Decisions"), "- Redis goes in this week.");
    assert_eq!(
        section(&meeting, "Action Items"),
        "- TICK-0001: Ship the Redis session store (Dev, due Thursday)\n\
         - TICK-0002: Load test with two instances\n\
         - TICK-0003: Write the cutover runbook"
    );
    assert_eq!(section(&meeting, "Open Questions"), "None.");
}

#[test]
fn a_rerun_with_fewer_tasks_deletes_the_leftover_untouched_ticket() {
    let root = Root::new("rerun-fewer");
    root.meeting(STANDUP);
    root.write(STANDUP, &notes("standup"), &first());

    let outcome = root.write(STANDUP, &notes("standup-rerun"), &second());

    assert_eq!(outcome.written, ids(&["TICK-0001", "TICK-0002"]));
    assert_eq!(outcome.removed, ids(&["TICK-0003"]));
    assert!(outcome.kept.is_empty());
    assert_eq!(root.ticket_files(STANDUP), ["TICK-0001.md", "TICK-0002.md"]);
    let meeting = root.read_meeting(STANDUP);
    assert!(!section(&meeting, "Action Items").contains("TICK-0003"));
}

#[test]
fn a_rerun_with_more_tasks_gives_the_new_ones_fresh_numbers() {
    let root = Root::new("rerun-more");
    root.meeting(STANDUP);
    root.write(STANDUP, &notes("standup-rerun"), &first());

    let outcome = root.write(STANDUP, &notes("standup"), &second());

    assert_eq!(
        outcome.written,
        ids(&["TICK-0001", "TICK-0002", "TICK-0003"])
    );
    assert!(outcome.removed.is_empty());
    assert_eq!(
        root.ticket(STANDUP, "TICK-0003").title().as_deref(),
        Some("Write the cutover runbook")
    );
}

#[test]
fn a_rerun_with_no_tasks_deletes_every_untouched_ticket() {
    let root = Root::new("rerun-none");
    root.meeting(STANDUP);
    root.write(STANDUP, &notes("standup"), &first());
    let mut empty = notes("standup-rerun");
    empty.tasks.clear();

    let outcome = root.write(STANDUP, &empty, &second());

    assert!(outcome.written.is_empty());
    assert_eq!(
        outcome.removed,
        ids(&["TICK-0001", "TICK-0002", "TICK-0003"])
    );
    assert!(root.ticket_files(STANDUP).is_empty());
    let meeting = root.read_meeting(STANDUP);
    assert_eq!(section(&meeting, "Action Items"), "None.");
    assert_eq!(meeting.frontmatter.get(AGENT_TICKETS_KEY), None);
}

// ---------------------------------------------------------------------------
// 3 & 4. Re-running with a ticket the user touched
// ---------------------------------------------------------------------------

/// Write the standup notes, let `touch` change TICK-0002 by hand, then re-run
/// with the two-task notes. Returns the outcome and TICK-0002's bytes as the
/// user left them.
fn rerun_after_touching_tick_2(root: &Root, touch: impl FnOnce(&Path)) -> (Outcome, Vec<u8>) {
    root.meeting(STANDUP);
    root.write(STANDUP, &notes("standup"), &first());
    let path = root.ticket_path(STANDUP, "TICK-0002");
    touch(&path);
    let touched = fs::read(&path).unwrap();
    let outcome = root.write(STANDUP, &notes("standup-rerun"), &second());
    (outcome, touched)
}

/// TICK-0002 was kept as the user left it; the two new tasks took TICK-0001
/// and TICK-0003, the untouched ones.
fn assert_tick_2_kept(root: &Root, outcome: &Outcome, touched: &[u8]) {
    assert_eq!(outcome.kept, ids(&["TICK-0002"]));
    assert_eq!(outcome.written, ids(&["TICK-0001", "TICK-0003"]));
    assert!(outcome.removed.is_empty());
    assert_eq!(
        fs::read(root.ticket_path(STANDUP, "TICK-0002")).unwrap(),
        touched,
        "a touched ticket must be left byte for byte"
    );
    assert_eq!(
        root.ticket(STANDUP, "TICK-0003").title().as_deref(),
        Some("Load test with two instances")
    );
}

#[test]
fn a_rerun_keeps_a_ticket_whose_body_was_edited_by_hand() {
    let root = Root::new("kept-body");
    let (outcome, touched) = rerun_after_touching_tick_2(&root, |path| {
        let mut raw = fs::read_to_string(path).unwrap();
        raw.push_str("\nTried it on staging first: p95 was 180 ms.\n");
        fs::write(path, raw).unwrap();
    });
    assert_tick_2_kept(&root, &outcome, &touched);

    // Kept tickets are listed after the new ones, title and assignee only.
    let meeting = root.read_meeting(STANDUP);
    assert_eq!(
        section(&meeting, "Action Items"),
        "- TICK-0001: Ship the Redis session store (Dev, due Thursday)\n\
         - TICK-0003: Load test with two instances\n\
         - TICK-0002: Load test the login path (Priya)"
    );
}

#[test]
fn a_rerun_keeps_a_ticket_whose_title_was_edited_by_hand() {
    let root = Root::new("kept-title");
    let (outcome, touched) = rerun_after_touching_tick_2(&root, |path| {
        let mut ticket = Ticket::read(path).unwrap();
        ticket
            .frontmatter
            .set_str("title", Some("Load test login with 2x traffic"));
        ticket.write(path).unwrap();
    });
    assert_tick_2_kept(&root, &outcome, &touched);
    let meeting = root.read_meeting(STANDUP);
    assert!(
        section(&meeting, "Action Items")
            .contains("- TICK-0002: Load test login with 2x traffic (Priya)"),
        "the action item shows the user's title"
    );
}

#[test]
fn a_rerun_keeps_a_ticket_moved_to_in_progress() {
    let root = Root::new("kept-status");
    let (outcome, touched) = rerun_after_touching_tick_2(&root, |path| {
        let mut ticket = Ticket::read(path).unwrap();
        ticket.set_status(Status::InProgress);
        ticket.write(path).unwrap();
    });
    assert_tick_2_kept(&root, &outcome, &touched);
    assert_eq!(
        root.ticket(STANDUP, "TICK-0002").status(),
        Some(Status::InProgress)
    );
}

#[test]
fn a_rerun_keeps_a_ticket_synced_to_linear() {
    let root = Root::new("kept-synced");
    let (outcome, touched) = rerun_after_touching_tick_2(&root, |path| {
        let mut ticket = Ticket::read(path).unwrap();
        ticket.frontmatter.set_str("synced_to", Some("linear"));
        ticket.frontmatter.set_str("external_id", Some("ENG-42"));
        ticket
            .frontmatter
            .set_str("external_url", Some("https://linear.app/acme/issue/ENG-42"));
        ticket.write(path).unwrap();
    });
    assert_tick_2_kept(&root, &outcome, &touched);
    assert_eq!(
        root.ticket(STANDUP, "TICK-0002").synced_to().as_deref(),
        Some("linear")
    );
}

#[test]
fn a_kept_ticket_is_not_claimed_back_by_a_later_run() {
    let root = Root::new("kept-stays-kept");
    let (_, touched) = rerun_after_touching_tick_2(&root, |path| {
        let mut ticket = Ticket::read(path).unwrap();
        ticket.set_status(Status::Done);
        ticket.write(path).unwrap();
    });
    // The user puts the status back. Even though the file now looks like an
    // open, unsynced ticket again, it is the user's from the first touch on.
    let path = root.ticket_path(STANDUP, "TICK-0002");
    let mut ticket = Ticket::read(&path).unwrap();
    ticket.set_status(Status::Open);
    ticket.write(&path).unwrap();
    let reopened = fs::read(&path).unwrap();
    assert_ne!(reopened, touched);

    let outcome = root.write(STANDUP, &notes("standup"), &second());

    assert!(!outcome.written.contains(&"TICK-0002".to_owned()));
    assert!(!outcome.removed.contains(&"TICK-0002".to_owned()));
    assert_eq!(fs::read(&path).unwrap(), reopened);
}

// ---------------------------------------------------------------------------
// 5. A ticket the user deleted
// ---------------------------------------------------------------------------

#[test]
fn a_ticket_the_user_deleted_is_not_recreated_by_a_rerun() {
    let root = Root::new("deleted");
    root.meeting(STANDUP);
    root.write(STANDUP, &notes("standup"), &first());
    fs::remove_file(root.ticket_path(STANDUP, "TICK-0002")).unwrap();

    let outcome = root.write(STANDUP, &notes("standup-rerun"), &second());

    assert_eq!(outcome.written, ids(&["TICK-0001", "TICK-0003"]));
    assert!(outcome.kept.is_empty());
    assert!(outcome.removed.is_empty());
    assert!(!root.ticket_path(STANDUP, "TICK-0002").exists());
    let meeting = root.read_meeting(STANDUP);
    assert!(!section(&meeting, "Action Items").contains("TICK-0002"));
}

/// The deleted ticket held the highest number under the root, so the files
/// left on disk alone would hand that number straight back out. The record in
/// `meeting.md` keeps it retired.
#[test]
fn a_deleted_ticket_with_the_highest_number_is_not_recreated_by_a_rerun() {
    let root = Root::new("deleted-highest");
    root.meeting(STANDUP);
    root.write(STANDUP, &notes("standup"), &first());
    fs::remove_file(root.ticket_path(STANDUP, "TICK-0003")).unwrap();

    let outcome = root.write(STANDUP, &notes("standup"), &second());

    assert!(
        !root.ticket_path(STANDUP, "TICK-0003").exists(),
        "TICK-0003 was deleted by the user but came back: {outcome:?}"
    );
    assert_eq!(
        outcome.written,
        ids(&["TICK-0001", "TICK-0002", "TICK-0004"])
    );
}

// ---------------------------------------------------------------------------
// 6. Numbering across the whole root
// ---------------------------------------------------------------------------

#[test]
fn ticket_numbers_continue_across_meetings() {
    let root = Root::new("numbers-across");
    root.meeting(RETRO);
    root.meeting(STANDUP);

    let a = root.write(RETRO, &notes("retro"), &first());
    let b = root.write(STANDUP, &notes("standup"), &first());

    assert_eq!(a.written, ids(&["TICK-0001", "TICK-0002"]));
    assert_eq!(b.written, ids(&["TICK-0003", "TICK-0004", "TICK-0005"]));
    assert_eq!(root.ticket_files(RETRO), ["TICK-0001.md", "TICK-0002.md"]);
    assert_eq!(
        root.ticket(STANDUP, "TICK-0003").meeting().as_deref(),
        Some(STANDUP)
    );
}

#[test]
fn hand_made_and_broken_tickets_still_hold_their_numbers() {
    let root = Root::new("numbers-held");
    root.meeting(STANDUP);
    root.meeting(PLANNING);
    let hand_made = root.path.join(TICKETS_DIR).join("TICK-0007.md");
    Ticket::new("TICK-0007", "Rotate the API keys", "")
        .write(&hand_made)
        .unwrap();
    let broken = root.ticket_path(PLANNING, "TICK-0009");
    fs::create_dir_all(broken.parent().unwrap()).unwrap();
    fs::write(&broken, "no frontmatter at all\n").unwrap();
    let hand_made_before = fs::read(&hand_made).unwrap();

    let outcome = root.write(STANDUP, &notes("standup"), &first());

    assert_eq!(
        outcome.written,
        ids(&["TICK-0010", "TICK-0011", "TICK-0012"])
    );
    assert_eq!(fs::read(&hand_made).unwrap(), hand_made_before);
    assert_eq!(fs::read(&broken).unwrap(), b"no frontmatter at all\n");
}

// ---------------------------------------------------------------------------
// 7. A meeting.md that cannot be written back
// ---------------------------------------------------------------------------

/// Run `write` over a `meeting.md` holding `bad`, and check that it failed
/// and that the folder is exactly as it was.
fn assert_refused(name: &str, bad: &[u8]) -> Error {
    let root = Root::new(name);
    let dir = root.meeting(STANDUP);
    fs::write(dir.join(MEETING_FILE), bad).unwrap();
    let writes = SelfWrites::default();

    let error = agent_notes::write(&root.path, STANDUP, &notes("standup"), &first(), &writes)
        .expect_err("a meeting.md that cannot be written back must be refused");

    assert_eq!(fs::read(dir.join(MEETING_FILE)).unwrap(), bad);
    assert!(!dir.join(TICKETS_DIR).exists(), "no ticket may land");
    assert_eq!(fs::read_to_string(dir.join(NOTES_FILE)).unwrap(), NOTES);
    assert_eq!(
        fs::read_to_string(dir.join(TRANSCRIPT_FILE)).unwrap(),
        TRANSCRIPT
    );
    let meeting_md = fs::canonicalize(dir.join(MEETING_FILE)).unwrap();
    assert!(!writes.is_suppressed(&meeting_md, Instant::now()));
    error
}

#[test]
fn a_meeting_md_with_broken_frontmatter_is_refused_and_nothing_is_written() {
    let error = assert_refused(
        "refused-yaml",
        b"---\nid: [unclosed\n---\n\n## Summary\n\nThe user's own words.\n",
    );
    assert!(
        matches!(error, Error::Frontmatter { .. }),
        "expected Error::Frontmatter, got {error:?}"
    );
}

#[test]
fn a_meeting_md_that_is_not_utf8_is_refused_and_left_alone() {
    let error = assert_refused(
        "refused-utf8",
        b"---\nid: 2026-09-01-1430-standup\n---\n\n## Summary\n\n\xff\xfe broken\n",
    );
    match error {
        Error::Io(io) => assert_eq!(io.kind(), std::io::ErrorKind::InvalidData),
        other => panic!("expected an InvalidData Io error, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 8. Self-writes
// ---------------------------------------------------------------------------

#[test]
fn every_file_written_is_noted_as_a_self_write_and_nothing_else_is() {
    let root = Root::new("self-writes");
    let dir = root.meeting(STANDUP);
    let writes = SelfWrites::default();

    let outcome =
        agent_notes::write(&root.path, STANDUP, &notes("standup"), &first(), &writes).unwrap();

    let now = Instant::now();
    let canonical = |path: PathBuf| fs::canonicalize(path).unwrap();
    assert!(writes.is_suppressed(&canonical(dir.join(MEETING_FILE)), now));
    for id in &outcome.written {
        assert!(
            writes.is_suppressed(&canonical(root.ticket_path(STANDUP, id)), now),
            "{id} was not noted"
        );
    }
    assert!(!writes.is_suppressed(&canonical(dir.join(NOTES_FILE)), now));
    assert!(!writes.is_suppressed(&canonical(dir.join(TRANSCRIPT_FILE)), now));
}

#[test]
fn a_ticket_removed_by_a_rerun_is_noted_as_a_self_write() {
    let root = Root::new("self-writes-removed");
    root.meeting(STANDUP);
    root.write(STANDUP, &notes("standup"), &first());
    let tickets_dir = fs::canonicalize(root.path.join(STANDUP).join(TICKETS_DIR)).unwrap();
    let writes = SelfWrites::default();

    let outcome = agent_notes::write(
        &root.path,
        STANDUP,
        &notes("standup-rerun"),
        &second(),
        &writes,
    )
    .unwrap();

    assert_eq!(outcome.removed, ids(&["TICK-0003"]));
    // The file is gone, so its canonical form is the folder's plus the name.
    assert!(writes.is_suppressed(&tickets_dir.join("TICK-0003.md"), Instant::now()));
}

// ---------------------------------------------------------------------------
// 9. Bad ids and missing folders
// ---------------------------------------------------------------------------

#[test]
fn a_meeting_id_that_leaves_the_root_is_refused() {
    let root = Root::new("bad-id");
    root.meeting(STANDUP);
    for hostile in ["..", "../elsewhere", "/etc", ".app"] {
        let result = agent_notes::write(
            &root.path,
            hostile,
            &notes("standup"),
            &first(),
            &SelfWrites::default(),
        );
        assert!(
            matches!(&result, Err(Error::BadId(id)) if id == hostile),
            "{hostile:?}: expected BadId, got {result:?}"
        );
    }
    assert!(!root.path.join(TICKETS_DIR).exists());
    assert!(!root.path.join(MEETING_FILE).exists());
}

#[test]
fn a_meeting_folder_that_does_not_exist_is_an_error() {
    let root = Root::new("missing-folder");
    let result = agent_notes::write(
        &root.path,
        STANDUP,
        &notes("standup"),
        &first(),
        &SelfWrites::default(),
    );
    match result {
        Err(Error::Io(io)) => assert_eq!(io.kind(), std::io::ErrorKind::NotFound),
        other => panic!("expected a NotFound Io error, got {other:?}"),
    }
    assert!(
        !root.path.join(STANDUP).exists(),
        "the folder is not created"
    );
}

// ---------------------------------------------------------------------------
// 10. Text that looks like YAML or markdown structure
// ---------------------------------------------------------------------------

#[test]
fn a_yaml_hostile_title_round_trips_through_the_ticket_file() {
    let root = Root::new("hostile-title");
    root.meeting(STANDUP);
    let hostile = r#"--- "Fix: the #1 bug" in 'auth' & {login}: [now] | > ~ null"#;
    let mut notes = notes("standup");
    notes.tasks.truncate(1);
    notes.tasks[0].title = hostile.to_owned();
    notes.tasks[0].owner = Some("O'Brien: lead".to_owned());
    notes.tasks[0].details = "First line.\n---\n## Not a section\nLast line.".to_owned();

    root.write(STANDUP, &notes, &first());

    let ticket = root.ticket(STANDUP, "TICK-0001");
    assert!(ticket.problems.is_empty(), "{:?}", ticket.problems);
    assert_eq!(ticket.title().as_deref(), Some(hostile));
    assert_eq!(ticket.assignee().as_deref(), Some("O'Brien: lead"));
    assert_eq!(ticket.status(), Some(Status::Open));
    assert!(
        ticket
            .body
            .contains("First line.\n---\n## Not a section\nLast line."),
        "{:?}",
        ticket.body
    );

    let meeting = root.read_meeting(STANDUP);
    assert!(meeting.problems.is_empty(), "{:?}", meeting.problems);
    assert_eq!(
        section(&meeting, "Action Items"),
        format!("- TICK-0001: {hostile} (O'Brien: lead, due Friday)")
    );
}
