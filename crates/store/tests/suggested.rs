//! TUR-113: approving a meeting's suggested tasks into Tickets, discarding
//! them, and the update step that moves already-synced tickets. Real folders
//! on disk, the crate's public API only.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use prompts::notes::Notes;
use store::agent_notes::{self, Analysis, AnalyzedBy, RETIRED_TICKETS_KEY};
use store::meeting::Meeting;
use store::suggested::{self, ApprovedAll, Moved};
use store::ticket::{Status, Ticket};
use store::watcher::SelfWrites;
use store::{Error, MEETING_FILE, TICKETS_DIR};

const STANDUP: &str = "2026-09-01-1430-standup";
const RETRO: &str = "2026-09-02-1000-retro";

struct Root {
    path: PathBuf,
    _guard: tempfile::TempDir,
}

impl Root {
    fn new(name: &str) -> Self {
        let guard = tempfile::Builder::new()
            .prefix(&format!("meet-ai-store-suggested-{name}-"))
            .tempdir()
            .unwrap();
        Self {
            path: guard.path().to_path_buf(),
            _guard: guard,
        }
    }

    /// A meeting whose notes run suggested the three standup tasks,
    /// TICK-0001 to TICK-0003.
    fn with_notes(name: &str) -> Self {
        let root = Self::new(name);
        root.meeting(STANDUP);
        root.notes(STANDUP, "standup");
        root
    }

    fn meeting(&self, id: &str) {
        fs::create_dir_all(self.path.join(id)).unwrap();
    }

    fn notes(&self, meeting: &str, fixture: &str) -> agent_notes::Outcome {
        let analysis = Analysis {
            by: AnalyzedBy::ClaudeCode,
            model: "opus".to_owned(),
            at: "2026-09-01T15:32:00+05:30".to_owned(),
        };
        agent_notes::write(
            &self.path,
            meeting,
            &fixture_notes(fixture),
            &analysis,
            &SelfWrites::default(),
        )
        .unwrap()
    }

    fn suggestion(&self, meeting: &str, id: &str) -> PathBuf {
        self.path
            .join(meeting)
            .join(TICKETS_DIR)
            .join(format!("{id}.md"))
    }

    fn approved(&self, id: &str) -> PathBuf {
        self.path.join(TICKETS_DIR).join(format!("{id}.md"))
    }

    fn read_meeting(&self, meeting: &str) -> Meeting {
        Meeting::read(&self.path.join(meeting).join(MEETING_FILE))
            .unwrap()
            .expect("meeting.md")
    }

    fn approve(&self, id: &str) -> Result<PathBuf, Error> {
        suggested::approve(&self.path, STANDUP, id, &SelfWrites::default())
    }

    fn discard(&self, id: &str) -> Result<(), Error> {
        suggested::discard(&self.path, STANDUP, id, &SelfWrites::default())
    }

    fn suggested(&self) -> Vec<String> {
        suggested::suggested(&self.path, STANDUP).unwrap()
    }
}

fn fixture_notes(name: &str) -> Notes {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("notes")
        .join(format!("{name}.json"));
    Notes::from_json(&fs::read_to_string(path).unwrap()).unwrap()
}

const RUNBOOK: &str = "Write the cutover runbook";

/// Titles of the files in `<meeting>/tickets/`.
fn suggestion_titles(root: &Root, meeting: &str) -> Vec<String> {
    let dir = root.path.join(meeting).join(TICKETS_DIR);
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| Ticket::read(&e.path()).ok()?.title())
                .collect()
        })
        .unwrap_or_default()
}

fn ids(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

fn action_items(meeting: &Meeting) -> String {
    meeting.section("Action Items").unwrap().trim().to_owned()
}

fn io_kind(error: Error) -> io::ErrorKind {
    match error {
        Error::Io(error) => error.kind(),
        other => panic!("expected an io error, got {other:?}"),
    }
}

// --- approve ----------------------------------------------------------------

#[test]
fn approve_moves_the_file_to_tickets_and_keeps_its_id_and_meeting() {
    let root = Root::with_notes("approve");
    let before = Ticket::read(&root.suggestion(STANDUP, "TICK-0002")).unwrap();

    let path = root.approve("TICK-0002").unwrap();

    assert_eq!(path, root.approved("TICK-0002"));
    assert!(!root.suggestion(STANDUP, "TICK-0002").exists());
    let after = Ticket::read(&path).unwrap();
    assert_eq!(after.id().as_deref(), Some("TICK-0002"));
    assert_eq!(after.meeting().as_deref(), Some(STANDUP));
    assert_eq!(after.status(), Some(Status::Open));
    // Nothing else in the file changes.
    assert_eq!(after, before);
    assert_eq!(root.suggested(), ids(&["TICK-0001", "TICK-0003"]));
}

#[test]
fn approve_fills_in_a_missing_meeting_and_status() {
    let root = Root::new("approve-fill");
    root.meeting(STANDUP);
    let path = root.suggestion(STANDUP, "TICK-0004");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        "---\nid: TICK-0004\ntitle: Hand written\n---\n\nBody.\n",
    )
    .unwrap();

    let moved = root.approve("TICK-0004").unwrap();

    let after = Ticket::read(&moved).unwrap();
    assert_eq!(after.meeting().as_deref(), Some(STANDUP));
    assert_eq!(after.status(), Some(Status::Open));
    assert_eq!(after.body, "\nBody.\n");
}

#[test]
fn an_approved_task_is_not_brought_back_by_a_notes_rerun_and_stays_in_action_items() {
    let root = Root::with_notes("approve-rerun");
    root.approve("TICK-0003").unwrap();

    let outcome = root.notes(STANDUP, "standup");

    assert!(
        !outcome.written.contains(&"TICK-0003".to_owned()),
        "{outcome:?}"
    );
    assert!(!root.suggestion(STANDUP, "TICK-0003").exists());
    assert!(root.approved("TICK-0003").exists());
    // Not back under a new number either: only the two untouched are rewritten.
    assert!(
        !suggestion_titles(&root, STANDUP).contains(&RUNBOOK.to_owned()),
        "{:?}",
        suggestion_titles(&root, STANDUP)
    );
    assert_eq!(outcome.written.len(), 2, "{outcome:?}");
    // The approved one is still this meeting's task.
    let items = action_items(&root.read_meeting(STANDUP));
    assert!(
        items.contains("TICK-0003: Write the cutover runbook"),
        "{items}"
    );
}

#[test]
fn approving_a_task_that_is_not_there_is_not_found() {
    let root = Root::with_notes("approve-missing");
    assert_eq!(
        io_kind(root.approve("TICK-0009").unwrap_err()),
        io::ErrorKind::NotFound
    );
    root.approve("TICK-0001").unwrap();
    // Twice: the second finds no suggestion left.
    assert_eq!(
        io_kind(root.approve("TICK-0001").unwrap_err()),
        io::ErrorKind::NotFound
    );
}

#[test]
fn approve_never_overwrites_a_ticket_already_in_tickets() {
    let root = Root::with_notes("approve-taken");
    fs::create_dir_all(root.path.join(TICKETS_DIR)).unwrap();
    fs::write(root.approved("TICK-0001"), "theirs").unwrap();

    let error = root.approve("TICK-0001").unwrap_err();

    assert_eq!(io_kind(error), io::ErrorKind::AlreadyExists);
    assert_eq!(
        fs::read_to_string(root.approved("TICK-0001")).unwrap(),
        "theirs"
    );
    assert!(root.suggestion(STANDUP, "TICK-0001").exists());
}

#[test]
fn ids_that_leave_the_folder_are_refused() {
    let root = Root::with_notes("approve-hostile");
    for hostile in ["../TICK-0001", "TICK-0001/..", "notes", ""] {
        assert!(
            matches!(root.approve(hostile), Err(Error::BadId(_))),
            "{hostile:?}"
        );
        assert!(
            matches!(root.discard(hostile), Err(Error::BadId(_))),
            "{hostile:?}"
        );
    }
    let outside = suggested::approve(&root.path, "..", "TICK-0001", &SelfWrites::default());
    assert!(matches!(outside, Err(Error::BadId(_))));
}

#[test]
fn approve_all_moves_every_suggestion_in_number_order() {
    let root = Root::with_notes("approve-all");
    root.approve("TICK-0002").unwrap();

    let outcome = suggested::approve_all(&root.path, STANDUP, &SelfWrites::default()).unwrap();

    assert_eq!(
        outcome,
        ApprovedAll {
            approved: ids(&["TICK-0001", "TICK-0003"]),
            skipped: Vec::new(),
        }
    );
    assert!(root.suggested().is_empty());
    for id in ["TICK-0001", "TICK-0002", "TICK-0003"] {
        assert!(root.approved(id).exists(), "{id}");
    }
    // Nothing left: a second press does nothing.
    let again = suggested::approve_all(&root.path, STANDUP, &SelfWrites::default()).unwrap();
    assert_eq!(again, ApprovedAll::default());
}

#[test]
fn approve_all_skips_a_broken_file_and_moves_the_rest() {
    let root = Root::with_notes("approve-all-broken");
    fs::write(
        root.suggestion(STANDUP, "TICK-0002"),
        "---\ntitle: [unclosed\n---\nKeep.\n",
    )
    .unwrap();

    let outcome = suggested::approve_all(&root.path, STANDUP, &SelfWrites::default()).unwrap();

    assert_eq!(outcome.approved, ids(&["TICK-0001", "TICK-0003"]));
    assert_eq!(outcome.skipped, ids(&["TICK-0002"]));
    assert_eq!(
        fs::read_to_string(root.suggestion(STANDUP, "TICK-0002")).unwrap(),
        "---\ntitle: [unclosed\n---\nKeep.\n"
    );
}

// --- discard ----------------------------------------------------------------

#[test]
fn discard_deletes_the_file_and_takes_it_out_of_action_items() {
    let root = Root::with_notes("discard");

    root.discard("TICK-0002").unwrap();

    assert!(!root.suggestion(STANDUP, "TICK-0002").exists());
    assert!(!root.approved("TICK-0002").exists());
    assert_eq!(root.suggested(), ids(&["TICK-0001", "TICK-0003"]));
    let meeting = root.read_meeting(STANDUP);
    let items = action_items(&meeting);
    assert!(!items.contains("TICK-0002"), "{items}");
    assert!(items.contains("TICK-0001"), "{items}");
    assert!(items.contains("TICK-0003"), "{items}");
    assert_eq!(
        meeting.frontmatter.get_str_list(RETIRED_TICKETS_KEY),
        Some(ids(&["TICK-0002"]))
    );
}

#[test]
fn discarding_the_last_task_leaves_none_in_action_items() {
    let root = Root::with_notes("discard-all");
    for id in ["TICK-0001", "TICK-0002", "TICK-0003"] {
        root.discard(id).unwrap();
    }
    assert_eq!(action_items(&root.read_meeting(STANDUP)), "None.");
}

/// The discarded task held the highest number under the root, so the files
/// left alone would hand it straight back out.
#[test]
fn discarding_the_highest_task_retires_its_number() {
    let root = Root::with_notes("discard-highest");
    root.discard("TICK-0003").unwrap();

    // What a hand-made ticket uses (`tickets::create_in` in the app).
    let next = agent_notes::highest_ticket_number(&root.path)
        .unwrap()
        .max(agent_notes::highest_recorded_ticket_number(&root.path).unwrap());
    assert_eq!(next, 3, "TICK-0003 must stay taken");

    // A notes run in another meeting does not take it either.
    root.meeting(RETRO);
    let retro = root.notes(RETRO, "retro");
    assert!(
        retro.written.iter().all(|id| id.as_str() > "TICK-0003"),
        "{retro:?}"
    );
}

/// The `agent_tickets` record drops a deleted ticket on the next re-run; the
/// retired list does not, so even a re-run with fewer tasks keeps the number.
#[test]
fn a_discarded_task_is_not_recreated_by_notes_reruns() {
    let root = Root::with_notes("discard-rerun");
    root.discard("TICK-0003").unwrap();

    let first = root.notes(STANDUP, "standup-rerun");
    let second = root.notes(STANDUP, "standup");

    for outcome in [&first, &second] {
        assert!(
            !outcome.written.contains(&"TICK-0003".to_owned()),
            "{outcome:?}"
        );
    }
    assert!(!root.suggestion(STANDUP, "TICK-0003").exists());
    assert!(
        !suggestion_titles(&root, STANDUP).contains(&RUNBOOK.to_owned()),
        "{:?}",
        suggestion_titles(&root, STANDUP)
    );
    assert_eq!(second.written.len(), 2, "{second:?}");
    assert!(
        agent_notes::highest_recorded_ticket_number(&root.path).unwrap() >= 3,
        "the number stays retired"
    );
}

#[test]
fn discarding_a_task_that_is_not_there_is_not_found() {
    let root = Root::with_notes("discard-missing");
    assert_eq!(
        io_kind(root.discard("TICK-0009").unwrap_err()),
        io::ErrorKind::NotFound
    );
}

#[test]
fn discard_refuses_a_broken_meeting_md_and_keeps_the_task() {
    let root = Root::with_notes("discard-broken");
    let meeting_md = root.path.join(STANDUP).join(MEETING_FILE);
    fs::write(&meeting_md, "---\ntitle: [unclosed\n---\n").unwrap();

    let error = root.discard("TICK-0001").unwrap_err();

    assert!(matches!(error, Error::Frontmatter { .. }), "{error:?}");
    assert!(root.suggestion(STANDUP, "TICK-0001").exists());
    assert_eq!(
        fs::read_to_string(&meeting_md).unwrap(),
        "---\ntitle: [unclosed\n---\n"
    );
}

#[test]
fn every_path_discard_and_approve_touch_is_noted_as_a_self_write() {
    let root = Root::with_notes("self-writes");
    let noted = SelfWrites::default();
    suggested::discard(&root.path, STANDUP, "TICK-0001", &noted).unwrap();
    suggested::approve(&root.path, STANDUP, "TICK-0002", &noted).unwrap();
    for path in [
        root.path.join(STANDUP).join(MEETING_FILE),
        root.approved("TICK-0002"),
    ] {
        // Stored in canonical form: on macOS `/var` is `/private/var`.
        let path = dunce::canonicalize(&path).unwrap();
        assert!(
            noted.is_suppressed(&path, std::time::Instant::now()),
            "{}",
            path.display()
        );
    }
}

// --- the update step -------------------------------------------------------------

fn mark_synced(path: &Path) {
    let mut ticket = Ticket::read(path).unwrap();
    ticket.frontmatter.set_str("synced_to", Some("linear"));
    ticket.frontmatter.set_str("external_id", Some("ENG-42"));
    ticket
        .frontmatter
        .set_str("external_url", Some("https://linear.app/acme/issue/ENG-42"));
    ticket.write(path).unwrap();
}

#[test]
fn the_update_step_moves_synced_tickets_only_and_is_idempotent() {
    let root = Root::with_notes("migrate");
    root.meeting(RETRO);
    root.notes(RETRO, "retro");
    mark_synced(&root.suggestion(STANDUP, "TICK-0002"));
    let retro_first = suggested::suggested(&root.path, RETRO).unwrap()[0].clone();
    mark_synced(&root.suggestion(RETRO, &retro_first));

    let moved = suggested::approve_synced(&root.path, &SelfWrites::default()).unwrap();

    // Meetings in folder-name order: the standup sorts before the retro.
    assert_eq!(
        moved,
        [
            Moved {
                meeting_id: STANDUP.to_owned(),
                ticket_id: "TICK-0002".to_owned(),
            },
            Moved {
                meeting_id: RETRO.to_owned(),
                ticket_id: retro_first,
            },
        ]
    );
    let moved_ticket = Ticket::read(&root.approved("TICK-0002")).unwrap();
    assert_eq!(moved_ticket.meeting().as_deref(), Some(STANDUP));
    assert_eq!(moved_ticket.synced_to().as_deref(), Some("linear"));
    assert_eq!(root.suggested(), ids(&["TICK-0001", "TICK-0003"]));

    // A second launch finds nothing left to move.
    let again = suggested::approve_synced(&root.path, &SelfWrites::default()).unwrap();
    assert!(again.is_empty(), "{again:?}");
}

#[test]
fn the_update_step_with_no_meetings_folder_moves_nothing() {
    let root = Root::new("migrate-none");
    let missing = root.path.join("not-there");
    assert!(
        suggested::approve_synced(&missing, &SelfWrites::default())
            .unwrap()
            .is_empty()
    );
}
