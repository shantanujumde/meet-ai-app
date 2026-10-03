//! Sync with a fake agent: `FakeHarness`, and fake `claude` / `codex`
//! scripts run through the real harnesses. No real CLI, no tracker.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use agent::fake::{FakeBehavior, FakeHarness};
use serde_json::json;

use super::tracker::checked;
use super::*;

const MEETING: &str = "2026-09-01-1430-standup";
const SECRET: &str = "SECRET-TRANSCRIPT-WORDS";
const URL: &str = "https://linear.app/acme/issue/ENG-42";

/// A meetings folder with one meeting (transcript, `meeting.md`) and its
/// `TICK-0001` in the meeting's own `tickets/` folder.
fn meetings_root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join(MEETING);
    fs::create_dir_all(dir.join(store::TICKETS_DIR)).unwrap();
    fs::write(
        dir.join(store::TRANSCRIPT_FILE),
        format!("[00:00:01] You: {SECRET}\n"),
    )
    .unwrap();
    fs::write(
        dir.join(store::MEETING_FILE),
        "---\ntitle: Standup\ndate: 2026-09-01 14:30\n---\n",
    )
    .unwrap();
    let mut made = Ticket::new("TICK-0001", "Fix the login redirect", MEETING);
    made.frontmatter.set_str("assignee", Some("Sam"));
    made.body = "\nUsers land on a blank page.\n\nDue: Friday.\n".to_owned();
    made.write(&ticket_path(root.path())).unwrap();
    root
}

fn ticket_path(root: &Path) -> PathBuf {
    root.join(MEETING)
        .join(store::TICKETS_DIR)
        .join("TICK-0001.md")
}

fn settings() -> RunSettings {
    RunSettings {
        model: Some("haiku".into()),
        timeout: Duration::from_secs(20),
        tickets: TicketsConfig::default(),
    }
}

fn sync_with(root: &Path, harness: &dyn Harness) -> Result<TicketSummary, UiError> {
    let synced = run(
        root,
        "TICK-0001",
        Some(MEETING),
        harness,
        &settings(),
        &CancelHandle::new(),
    )?;
    let path = find_ticket(root, "TICK-0001", Some(MEETING))?;
    record(&path, &settings().tickets.tracker, &synced)
}

fn reply(id: serde_json::Value, url: serde_json::Value) -> FakeHarness {
    FakeHarness::new(FakeBehavior::Reply(
        json!({ "external_id": id, "external_url": url }),
    ))
}

#[test]
fn a_reply_with_a_key_and_link_is_written_to_the_ticket() {
    if !crate::platform::FAKE_CLI_RUNS {
        return;
    }
    let root = meetings_root();
    let summary = sync_with(root.path(), &reply(json!("ENG-42"), json!(URL))).unwrap();
    assert_eq!(summary.synced_to.as_deref(), Some("linear"));
    assert_eq!(summary.external_id.as_deref(), Some("ENG-42"));
    assert_eq!(summary.external_url.as_deref(), Some(URL));

    let back = Ticket::read(&ticket_path(root.path())).unwrap();
    assert_eq!(back.synced_to().as_deref(), Some("linear"));
    assert_eq!(
        back.frontmatter.get_str("external_url").as_deref(),
        Some(URL)
    );
    // The rest of the file is kept.
    assert_eq!(back.assignee().as_deref(), Some("Sam"));
    assert!(back.body.contains("blank page"), "{}", back.body);
}

/// TUR-5: a Codex sync whose MCP call was refused exits 0 and answers with
/// no issue key. That is "not synced": an error, and the file untouched.
#[test]
fn a_reply_with_no_key_is_not_synced_and_writes_nothing() {
    if !crate::platform::FAKE_CLI_RUNS {
        return;
    }
    for (id, url) in [
        (json!(null), json!(null)),
        (json!(""), json!("")),
        (json!("ENG-42"), json!(null)),
        (json!(null), json!(URL)),
        (json!("N/A"), json!(URL)),
        (
            json!("ENG-42"),
            json!("http://linear.app/acme/issue/ENG-42"),
        ),
    ] {
        let root = meetings_root();
        let before = fs::read(ticket_path(root.path())).unwrap();
        let err = sync_with(root.path(), &reply(id.clone(), url.clone())).unwrap_err();
        assert_eq!(err.kind, "sync-not-done", "{id} {url}: {}", err.message);
        assert!(err.message.contains("Linear"), "{}", err.message);
        assert!(err.message.contains("Retry"), "{}", err.message);
        assert_eq!(fs::read(ticket_path(root.path())).unwrap(), before);
    }
}

#[test]
fn a_ticket_already_synced_is_not_synced_again() {
    if !crate::platform::FAKE_CLI_RUNS {
        return;
    }
    let root = meetings_root();
    sync_with(root.path(), &reply(json!("ENG-42"), json!(URL))).unwrap();
    // A second run would make a second issue; it must not start at all.
    let never = FakeHarness::new(FakeBehavior::Fail {
        code: 9,
        stderr: "should not run".into(),
    });
    let err = sync_with(root.path(), &never).unwrap_err();
    assert_eq!(err.kind, "sync-already-synced");
    assert!(err.message.contains("ENG-42"), "{}", err.message);
}

#[test]
fn agent_failures_keep_their_own_kind_and_write_nothing() {
    if !crate::platform::FAKE_CLI_RUNS {
        return;
    }
    let cases = [
        (FakeBehavior::NotSignedIn, "agent-not-signed-in"),
        (FakeBehavior::NotInstalled, "agent-not-installed"),
        (
            FakeBehavior::Fail {
                code: 2,
                stderr: "boom".into(),
            },
            "agent-failed",
        ),
        (
            FakeBehavior::Stdout("Sure! Done.".into()),
            "agent-bad-reply",
        ),
        (
            FakeBehavior::Reply(json!({ "external_id": "ENG-42" })),
            "agent-bad-reply",
        ),
    ];
    for (behavior, kind) in cases {
        let root = meetings_root();
        let before = fs::read(ticket_path(root.path())).unwrap();
        let err = sync_with(root.path(), &FakeHarness::new(behavior)).unwrap_err();
        assert_eq!(err.kind, kind, "{}", err.message);
        assert_eq!(fs::read(ticket_path(root.path())).unwrap(), before);
    }
}

#[test]
fn cancel_stops_the_run() {
    if !crate::platform::FAKE_CLI_RUNS {
        return;
    }
    let root = meetings_root();
    let runs = SyncRuns::default();
    let claim = runs.claim("TICK-0001").unwrap();
    runs.cancel("TICK-0001");
    let sleepy = FakeHarness::new(FakeBehavior::Sleep(Duration::from_secs(10)));
    let err = run(
        root.path(),
        "TICK-0001",
        Some(MEETING),
        &sleepy,
        &settings(),
        &claim.cancel,
    )
    .unwrap_err();
    assert_eq!(err.kind, "agent-cancelled");
}

#[test]
fn one_run_per_ticket_at_a_time() {
    let runs = SyncRuns::default();
    let first = runs.claim("TICK-0001").unwrap();
    assert_eq!(runs.claim("TICK-0001").err().unwrap().kind, "sync-busy");
    assert!(runs.claim("TICK-0002").is_ok());
    drop(first);
    assert!(runs.claim("TICK-0001").is_ok());
    // Cancelling a ticket with no run is a no-op.
    runs.cancel("TICK-0009");
}

#[test]
fn a_ticket_id_that_is_a_path_is_refused() {
    let root = meetings_root();
    for id in ["../TICK-0001", "TICK-0001/x", "", "notes"] {
        let err = run(
            root.path(),
            id,
            Some(MEETING),
            &reply(json!("ENG-42"), json!(URL)),
            &settings(),
            &CancelHandle::new(),
        )
        .unwrap_err();
        assert_eq!(err.kind, "bad-ticket-id", "{id}");
    }
    let err = run(
        root.path(),
        "TICK-0099",
        Some(MEETING),
        &reply(json!("ENG-42"), json!(URL)),
        &settings(),
        &CancelHandle::new(),
    )
    .unwrap_err();
    assert_eq!(err.kind, "ticket-missing");
}

#[test]
fn no_tracker_server_means_no_run() {
    let root = meetings_root();
    let mut settings = settings();
    settings.tickets.tracker_mcp = "  ".into();
    let err = run(
        root.path(),
        "TICK-0001",
        Some(MEETING),
        &reply(json!("ENG-42"), json!(URL)),
        &settings,
        &CancelHandle::new(),
    )
    .unwrap_err();
    assert_eq!(err.kind, "sync-no-tracker");
}

// --- fake CLIs through the real harnesses ----------------------------------

/// Writes an executable `name` script into `dir` that saves its arguments
/// (one per line) to `args.txt` and its stdin to `stdin.txt`, then runs
/// `then`.
fn fake_cli(dir: &Path, name: &str, then: &str) -> PathBuf {
    let path = dir.join(name);
    let script = format!(
        "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\"; done > '{args}'\ncat > '{stdin}'\n{then}\n",
        args = dir.join("args.txt").display(),
        stdin = dir.join("stdin.txt").display(),
    );
    fs::write(&path, script).unwrap();
    crate::platform::make_executable(&path).unwrap();
    path
}

fn agent_config(harness: HarnessChoice, binary: PathBuf) -> AgentConfig {
    AgentConfig {
        harness,
        binary_path: Some(binary),
        ..AgentConfig::default()
    }
}

#[test]
fn claude_gets_only_the_task_and_only_the_tracker_tools() {
    if !crate::platform::FAKE_CLI_RUNS {
        return;
    }
    let root = meetings_root();
    let bin = tempfile::tempdir().unwrap();
    let envelope = json!({
        "type": "result", "subtype": "success", "is_error": false,
        "structured_output": { "external_id": "ENG-42", "external_url": URL },
    });
    let script = fake_cli(bin.path(), "claude", &format!("printf '%s' '{envelope}'"));
    let agent = agent_config(HarnessChoice::ClaudeCode, script);
    let harness = harness_for(&agent).unwrap();
    assert_eq!(harness.id(), "claude-code");

    let summary = sync_with(root.path(), harness.as_ref()).unwrap();
    assert_eq!(summary.external_id.as_deref(), Some("ENG-42"));

    let args = fs::read_to_string(bin.path().join("args.txt")).unwrap();
    let args: Vec<&str> = args.lines().collect();
    let at = args.iter().position(|a| *a == "--allowedTools").unwrap();
    assert_eq!(args[at + 1], "mcp__claude_ai_Linear__*", "{args:?}");
    assert!(
        args.windows(2).any(|w| w == ["--model", "haiku"]),
        "{args:?}"
    );
    assert!(!args.contains(&"--strict-mcp-config"), "{args:?}");

    let stdin = fs::read_to_string(bin.path().join("stdin.txt")).unwrap();
    assert!(stdin.contains("Fix the login redirect"), "{stdin}");
    assert!(stdin.contains("Sam"), "{stdin}");
    assert!(stdin.contains("Friday"), "{stdin}");
    assert!(stdin.contains("claude.ai Linear"), "{stdin}");
    assert!(stdin.contains("Meeting: Standup\n"), "{stdin}");
    assert!(
        stdin.contains("Meeting date: 2026-09-01 14:30\n"),
        "{stdin}"
    );
    assert!(!stdin.contains(SECRET), "the transcript leaked: {stdin}");
    assert_no_local_path(root.path(), &stdin);
}

/// TUR-20: the prompt ends up in a shared issue, so it names the meeting by
/// title and date and never by where it is on this Mac.
fn assert_no_local_path(root: &Path, prompt: &str) {
    let root = root.display().to_string();
    let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/".into());
    for leak in [root.as_str(), home.as_str(), store::MEETING_FILE, "/Users/"] {
        assert!(!prompt.contains(leak), "{leak:?} leaked: {prompt}");
    }
}

/// What Codex does today when its MCP call is refused (`approval: never`):
/// exit 0, and a reply with no issue key.
#[test]
fn a_refused_codex_sync_exits_0_and_is_not_synced() {
    if !crate::platform::FAKE_CLI_RUNS {
        return;
    }
    let root = meetings_root();
    let before = fs::read(ticket_path(root.path())).unwrap();
    let bin = tempfile::tempdir().unwrap();
    let write_reply = r#"out=""; prev=""
case " $* " in *" mcp list "*) echo '[{"name":"linear"}]'; exit 0;; esac
for a in "$@"; do [ "$prev" = "-o" ] && out="$a"; prev="$a"; done
printf '%s' '{"external_id":null,"external_url":null}' > "$out""#;
    let script = fake_cli(bin.path(), "codex", write_reply);
    let mut settings = settings();
    settings.tickets.tracker_mcp = "linear".into();
    let harness = harness_for(&agent_config(HarnessChoice::Codex, script)).unwrap();
    assert_eq!(harness.id(), "codex");

    let err = run(
        root.path(),
        "TICK-0001",
        Some(MEETING),
        harness.as_ref(),
        &settings,
        &CancelHandle::new(),
    )
    .unwrap_err();
    assert_eq!(err.kind, "sync-not-done", "{}", err.message);
    assert_eq!(fs::read(ticket_path(root.path())).unwrap(), before);

    let args = fs::read_to_string(bin.path().join("args.txt")).unwrap();
    assert!(args.starts_with("exec\n"), "{args}");
    // `mcp__linear__*` is "all of linear's tools": no narrowing setting.
    assert!(!args.contains("enabled_tools"), "{args}");
    let stdin = fs::read_to_string(bin.path().join("stdin.txt")).unwrap();
    assert!(!stdin.contains(SECRET), "the transcript leaked: {stdin}");
    assert_no_local_path(root.path(), &stdin);
}

#[test]
fn no_agent_means_no_sync() {
    let agent = AgentConfig {
        harness: HarnessChoice::None,
        ..AgentConfig::default()
    };
    assert_eq!(harness_for(&agent).err().unwrap().kind, "sync-no-agent");
    let missing = agent_config(HarnessChoice::ClaudeCode, "/nonexistent/claude".into());
    assert_eq!(
        harness_for(&missing).err().unwrap().kind,
        "agent-not-installed"
    );
}

// --- listing, links, settings ----------------------------------------------

#[test]
fn meeting_tasks_lists_its_own_and_shared_tickets_in_order() {
    let root = meetings_root();
    let shared = root.path().join(store::TICKETS_DIR);
    fs::create_dir_all(&shared).unwrap();
    Ticket::new("TICK-0003", "Shared, this meeting", MEETING)
        .write(&shared.join("TICK-0003.md"))
        .unwrap();
    Ticket::new(
        "TICK-0002",
        "Shared, other meeting",
        "2026-09-02-0900-other",
    )
    .write(&shared.join("TICK-0002.md"))
    .unwrap();
    let mut no_meeting = Ticket::new("TICK-0004", "Agent forgot the key", MEETING);
    no_meeting.frontmatter.set_str("meeting", None);
    no_meeting
        .write(&ticket_path(root.path()).with_file_name("TICK-0004.md"))
        .unwrap();

    let tasks = meeting_tasks_in(root.path(), MEETING).unwrap();
    let ids: Vec<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["TICK-0001", "TICK-0003", "TICK-0004"]);
    assert!(tasks.iter().all(|t| t.meeting.as_deref() == Some(MEETING)));
    assert!(tasks.iter().all(|t| t.synced_to.is_none()));

    assert!(
        meeting_tasks_in(root.path(), "2026-01-01-0000-empty")
            .unwrap()
            .is_empty()
    );
    assert!(meeting_tasks_in(root.path(), "../x").is_err());
}

#[test]
fn only_a_checked_https_link_is_opened() {
    let root = meetings_root();
    let err = synced_url(root.path(), "TICK-0001", Some(MEETING)).unwrap_err();
    assert_eq!(err.kind, "sync-no-link");

    let path = ticket_path(root.path());
    let mut found = Ticket::read(&path).unwrap();
    found
        .frontmatter
        .set_str("external_url", Some("javascript:alert(1)"));
    found.write(&path).unwrap();
    assert_eq!(
        synced_url(root.path(), "TICK-0001", Some(MEETING))
            .unwrap_err()
            .kind,
        "sync-no-link"
    );

    record(
        &path,
        "linear",
        &Synced {
            external_id: "ENG-42".into(),
            external_url: URL.into(),
        },
    )
    .unwrap();
    assert_eq!(
        synced_url(root.path(), "TICK-0001", Some(MEETING)).unwrap(),
        URL
    );
}

#[test]
fn tracker_settings_are_checked() {
    let ok = checked(" jira ", " claude.ai Atlassian ").unwrap();
    assert_eq!(ok.tracker, "jira");
    assert_eq!(ok.tracker_mcp, "claude.ai Atlassian");
    assert_eq!(checked("trello", "x").unwrap_err().kind, "bad-tracker");
    assert_eq!(
        checked("linear", "  ").unwrap_err().kind,
        "bad-tracker-server"
    );
    assert_eq!(
        checked("linear", "a\nb").unwrap_err().kind,
        "bad-tracker-server"
    );
    assert_eq!(
        checked("github", &"x".repeat(201)).unwrap_err().kind,
        "bad-tracker-server"
    );
}

#[test]
fn due_is_read_from_the_body() {
    assert_eq!(
        due_in("\nDo it\n\nDue: Friday.\n").as_deref(),
        Some("Friday")
    );
    assert_eq!(due_in("Due: .\n"), None);
    assert_eq!(due_in("no due here"), None);
}

#[test]
fn tracker_names_read_well() {
    assert_eq!(tracker_name("linear"), "Linear");
    assert_eq!(tracker_name("github"), "GitHub");
    assert_eq!(tracker_name("jira"), "Jira");
    assert_eq!(tracker_name("other"), "other");
}

// --- saving the result (TUR-20) ---------------------------------------------

use super::save::{SYNC_KEPT_UNREADABLE, SYNC_NOT_ATTACHED, SYNC_NOT_SAVED};

const OTHER_MEETING: &str = "2026-09-02-0900-planning";
const NOT_ATTACHED: &str = "Created in Linear but couldn't attach it to this task: ";

/// Adds `meeting` to `root` with its own `TICK-0001`: ticket numbers are
/// only unique within one meeting plus the shared folder.
fn add_meeting(root: &Path, meeting: &str) {
    let dir = root.join(meeting).join(store::TICKETS_DIR);
    fs::create_dir_all(&dir).unwrap();
    Ticket::new("TICK-0001", "Plan the launch", meeting)
        .write(&dir.join("TICK-0001.md"))
        .unwrap();
}

fn ticket_in(root: &Path, meeting: &str) -> Ticket {
    let path = root
        .join(meeting)
        .join(store::TICKETS_DIR)
        .join("TICK-0001.md");
    Ticket::read(&path).unwrap()
}

/// A user editing the task's details in their own editor.
fn edit_the_details(root: &Path) {
    let path = ticket_path(root);
    let mut edited = Ticket::read(&path).unwrap();
    edited.body.push_str("\nAlso check Safari.\n");
    edited.write(&path).unwrap();
}

/// A notes re-run ("Make notes now") rewriting the meeting's tickets, which
/// can give an untouched number to a different task. Written the way the
/// notes run writes a ticket.
fn give_the_number_to_another_task(root: &Path) {
    Ticket::new("TICK-0001", "Book the offsite", MEETING)
        .write(&ticket_path(root))
        .unwrap();
}

/// Answers with a new issue each run, `ENG-42` first, and counts its runs.
/// With `move_to`, it first moves the meetings folder there and points
/// `root` at the new place, like a user changing the folder in Settings
/// while the agent works. With `meanwhile`, it first changes the ticket
/// under the current root, like an edit made while the agent works.
#[derive(Clone)]
struct MovingHarness {
    root: Arc<Mutex<PathBuf>>,
    move_to: Option<PathBuf>,
    meanwhile: Option<fn(&Path)>,
    runs: Arc<AtomicUsize>,
}

impl MovingHarness {
    fn new(root: &Arc<Mutex<PathBuf>>, move_to: Option<PathBuf>) -> Self {
        Self {
            root: Arc::clone(root),
            move_to,
            meanwhile: None,
            runs: Arc::default(),
        }
    }

    fn changing_the_ticket(root: &Arc<Mutex<PathBuf>>, change: fn(&Path)) -> Self {
        Self {
            meanwhile: Some(change),
            ..Self::new(root, None)
        }
    }

    fn runs(&self) -> usize {
        self.runs.load(Ordering::SeqCst)
    }
}

impl Harness for MovingHarness {
    fn id(&self) -> &'static str {
        "claude-code"
    }
    fn detect(&self) -> Option<agent::Install> {
        None
    }
    fn models(&self) -> Vec<String> {
        Vec::new()
    }
    fn run(&self, _job: &Job) -> Result<serde_json::Value, AgentError> {
        let number = 42 + self.runs.fetch_add(1, Ordering::SeqCst);
        let mut root = self.root.lock().unwrap();
        if let Some(to) = &self.move_to {
            fs::rename(&*root, to).unwrap();
            *root = to.clone();
        }
        if let Some(change) = self.meanwhile {
            change(&root);
        }
        Ok(json!({
            "external_id": format!("ENG-{number}"),
            "external_url": format!("https://linear.app/acme/issue/ENG-{number}"),
        }))
    }
}

/// `sync_in` on `meeting`'s `TICK-0001` with `harness`, the root read from
/// `root` each time it is asked.
fn sync_through(
    runs: &SyncRuns,
    gate: &FolderGate,
    root: &Arc<Mutex<PathBuf>>,
    harness: &MovingHarness,
    meeting: &str,
) -> Result<TicketSummary, UiError> {
    let lookup = || Ok(root.lock().unwrap().clone());
    sync_in(runs, Some(gate), lookup, "TICK-0001", Some(meeting), || {
        Ok((Box::new(harness.clone()) as Box<dyn Harness>, settings()))
    })
}

/// A sync whose save is refused because a folder move holds the root: the
/// issue exists in the tracker, the ticket has no link.
fn sync_during_a_move(
    runs: &SyncRuns,
    gate: &FolderGate,
    root: &Arc<Mutex<PathBuf>>,
    harness: &MovingHarness,
    meeting: &str,
) -> UiError {
    let _moving = gate.begin_move().unwrap();
    sync_through(runs, gate, root, harness, meeting).unwrap_err()
}

/// `err` is the "couldn't attach" error for the first issue, `ENG-42`.
fn assert_not_attached(err: &UiError) {
    assert_eq!(err.kind, SYNC_NOT_ATTACHED, "{}", err.message);
    assert!(
        err.message.starts_with(&format!("{NOT_ATTACHED}{URL} ")),
        "{}",
        err.message
    );
}

#[test]
fn a_folder_moved_during_the_sync_gets_the_link_in_its_new_place() {
    let old = meetings_root();
    let elsewhere = tempfile::tempdir().unwrap();
    let new = elsewhere.path().join("Moved Meetings");
    let root = Arc::new(Mutex::new(old.path().to_path_buf()));
    let harness = MovingHarness::new(&root, Some(new.clone()));

    let summary = sync_through(
        &SyncRuns::default(),
        &FolderGate::default(),
        &root,
        &harness,
        MEETING,
    )
    .unwrap();
    assert_eq!(summary.external_url.as_deref(), Some(URL));

    assert!(!ticket_path(old.path()).exists(), "the folder really moved");
    let back = Ticket::read(&ticket_path(&new)).unwrap();
    assert_eq!(
        back.frontmatter.get_str("external_id").as_deref(),
        Some("ENG-42")
    );
    assert_eq!(
        back.frontmatter.get_str("external_url").as_deref(),
        Some(URL)
    );
    assert_eq!(back.synced_to().as_deref(), Some("linear"));
}

#[test]
fn a_save_that_fails_shows_the_link_and_retry_does_not_make_a_second_issue() {
    let meetings = meetings_root();
    let root = Arc::new(Mutex::new(meetings.path().to_path_buf()));
    let harness = MovingHarness::new(&root, None);
    let runs = SyncRuns::default();
    let gate = FolderGate::default();
    let before = fs::read(ticket_path(meetings.path())).unwrap();

    let err = sync_during_a_move(&runs, &gate, &root, &harness, MEETING);
    assert_eq!(err.kind, SYNC_NOT_SAVED, "{}", err.message);
    assert!(
        err.message
            .starts_with("Created in Linear as ENG-42 but could not save the link"),
        "{}",
        err.message
    );
    assert!(err.message.contains(URL), "{}", err.message);
    assert!(err.message.contains("Retry"), "{}", err.message);
    assert_eq!(fs::read(ticket_path(meetings.path())).unwrap(), before);
    assert_eq!(harness.runs(), 1);

    // Retry saves the issue already made; the agent does not run again.
    let summary = sync_through(&runs, &gate, &root, &harness, MEETING).unwrap();
    assert_eq!(summary.external_id.as_deref(), Some("ENG-42"));
    assert_eq!(summary.external_url.as_deref(), Some(URL));
    assert_eq!(harness.runs(), 1, "a second issue");
    let back = Ticket::read(&ticket_path(meetings.path())).unwrap();
    assert_eq!(
        back.frontmatter.get_str("external_url").as_deref(),
        Some(URL)
    );

    // Saved, so it is forgotten: another press is the usual "already synced".
    let again = sync_through(&runs, &gate, &root, &harness, MEETING).unwrap_err();
    assert_eq!(again.kind, ALREADY_SYNCED, "{}", again.message);
    assert_eq!(harness.runs(), 1);
}

#[test]
fn a_kept_issue_is_only_saved_to_its_own_meetings_ticket() {
    let meetings = meetings_root();
    add_meeting(meetings.path(), OTHER_MEETING);
    let root = Arc::new(Mutex::new(meetings.path().to_path_buf()));
    let harness = MovingHarness::new(&root, None);
    let runs = SyncRuns::default();
    let gate = FolderGate::default();

    let err = sync_during_a_move(&runs, &gate, &root, &harness, MEETING);
    assert_eq!(err.kind, SYNC_NOT_SAVED, "{}", err.message);

    // The other meeting's TICK-0001 is another task: it gets its own issue.
    let other = sync_through(&runs, &gate, &root, &harness, OTHER_MEETING).unwrap();
    assert_eq!(harness.runs(), 2);
    assert_eq!(other.external_id.as_deref(), Some("ENG-43"));
    assert!(ticket_in(meetings.path(), MEETING).synced_to().is_none());

    // And the first meeting's Retry still saves its own issue.
    let first = sync_through(&runs, &gate, &root, &harness, MEETING).unwrap();
    assert_eq!(first.external_id.as_deref(), Some("ENG-42"));
    assert_eq!(harness.runs(), 2);
    assert_eq!(
        ticket_in(meetings.path(), OTHER_MEETING)
            .frontmatter
            .get_str("external_id")
            .as_deref(),
        Some("ENG-43")
    );
}

/// An edit in the user's own editor while the agent runs: the first save
/// refuses, keeps the user's edit, shows the link, and Retry never runs the
/// agent again.
#[test]
fn an_edit_during_the_sync_shows_the_link_and_never_makes_a_second_issue() {
    let meetings = meetings_root();
    let root = Arc::new(Mutex::new(meetings.path().to_path_buf()));
    let harness = MovingHarness::changing_the_ticket(&root, edit_the_details);
    let runs = SyncRuns::default();
    let gate = FolderGate::default();

    let err = sync_through(&runs, &gate, &root, &harness, MEETING).unwrap_err();
    assert_not_attached(&err);
    assert!(err.message.contains("TICK-0001 changed"), "{}", err.message);
    let back = Ticket::read(&ticket_path(meetings.path())).unwrap();
    assert!(back.body.contains("Also check Safari."), "{}", back.body);
    assert!(back.synced_to().is_none());
    assert_eq!(back.frontmatter.get_str("external_url"), None);

    let again = sync_through(&runs, &gate, &root, &harness, MEETING).unwrap_err();
    assert_not_attached(&again);
    assert_eq!(harness.runs(), 1, "a second issue");
}

/// A notes re-run while the agent runs gives the number to a different
/// task: the first save does not write the link onto it.
#[test]
fn a_number_given_to_another_task_during_the_sync_does_not_get_the_link() {
    let meetings = meetings_root();
    let root = Arc::new(Mutex::new(meetings.path().to_path_buf()));
    let harness = MovingHarness::changing_the_ticket(&root, give_the_number_to_another_task);
    let runs = SyncRuns::default();

    let err = sync_through(&runs, &FolderGate::default(), &root, &harness, MEETING).unwrap_err();
    assert_not_attached(&err);
    let back = ticket_in(meetings.path(), MEETING);
    assert_eq!(back.title().as_deref(), Some("Book the offsite"));
    assert!(back.synced_to().is_none());
    assert!(runs.unsaved.get("TICK-0001", Some(MEETING)).is_some());
    assert_eq!(harness.runs(), 1);
}

/// The same after a failed save: Retry shows the link, never runs the
/// agent, and keeps doing so until the user dismisses the issue.
#[test]
fn a_number_given_to_another_task_keeps_the_issue_until_dismissed() {
    let meetings = meetings_root();
    let root = Arc::new(Mutex::new(meetings.path().to_path_buf()));
    let harness = MovingHarness::new(&root, None);
    let runs = SyncRuns::default();
    let gate = FolderGate::default();
    sync_during_a_move(&runs, &gate, &root, &harness, MEETING);

    give_the_number_to_another_task(meetings.path());
    for _ in 0..2 {
        let err = sync_through(&runs, &gate, &root, &harness, MEETING).unwrap_err();
        assert_not_attached(&err);
        assert_eq!(harness.runs(), 1, "a second issue");
    }
    assert!(ticket_in(meetings.path(), MEETING).synced_to().is_none());

    // Dismissed: the next Sync is a fresh run for the new task.
    dismiss(&runs, &gate, &root);
    let summary = sync_through(&runs, &gate, &root, &harness, MEETING).unwrap();
    assert_eq!(harness.runs(), 2);
    assert_eq!(summary.external_id.as_deref(), Some("ENG-43"));
    assert_eq!(
        ticket_in(meetings.path(), MEETING).title().as_deref(),
        Some("Book the offsite")
    );
}

#[test]
fn a_kept_issue_never_overwrites_a_ticket_synced_meanwhile() {
    let meetings = meetings_root();
    let root = Arc::new(Mutex::new(meetings.path().to_path_buf()));
    let harness = MovingHarness::new(&root, None);
    let runs = SyncRuns::default();
    let gate = FolderGate::default();
    sync_during_a_move(&runs, &gate, &root, &harness, MEETING);

    let path = ticket_path(meetings.path());
    let mut synced = Ticket::read(&path).unwrap();
    synced.frontmatter.set_str("synced_to", Some("linear"));
    synced.frontmatter.set_str("external_id", Some("ENG-7"));
    synced
        .frontmatter
        .set_str("external_url", Some("https://linear.app/acme/issue/ENG-7"));
    synced.write(&path).unwrap();
    let before = fs::read(&path).unwrap();

    let err = sync_through(&runs, &gate, &root, &harness, MEETING).unwrap_err();
    assert_not_attached(&err);
    assert!(err.message.contains("as ENG-7"), "{}", err.message);
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(runs.unsaved.get("TICK-0001", Some(MEETING)).is_some());
    assert_eq!(harness.runs(), 1);
}

#[test]
fn a_kept_issue_waits_while_its_ticket_is_missing() {
    let meetings = meetings_root();
    let root = Arc::new(Mutex::new(meetings.path().to_path_buf()));
    let harness = MovingHarness::new(&root, None);
    let runs = SyncRuns::default();
    let gate = FolderGate::default();
    sync_during_a_move(&runs, &gate, &root, &harness, MEETING);

    let path = ticket_path(meetings.path());
    let aside = meetings.path().join("TICK-0001.md.aside");
    fs::rename(&path, &aside).unwrap();
    let err = sync_through(&runs, &gate, &root, &harness, MEETING).unwrap_err();
    assert_eq!(err.kind, SYNC_NOT_SAVED, "{}", err.message);
    assert!(err.message.contains(URL), "{}", err.message);
    assert!(err.message.contains("Retry"), "{}", err.message);
    assert!(runs.unsaved.get("TICK-0001", Some(MEETING)).is_some());
    assert_eq!(harness.runs(), 1);

    // The file is back as it was: Retry saves the kept issue.
    fs::rename(&aside, &path).unwrap();
    let summary = sync_through(&runs, &gate, &root, &harness, MEETING).unwrap();
    assert_eq!(summary.external_id.as_deref(), Some("ENG-42"));
    assert_eq!(harness.runs(), 1);
}

#[test]
fn a_kept_issue_is_never_dropped_on_its_own_even_if_the_meeting_is_deleted() {
    let meetings = meetings_root();
    let root = Arc::new(Mutex::new(meetings.path().to_path_buf()));
    let harness = MovingHarness::new(&root, None);
    let runs = SyncRuns::default();
    let gate = FolderGate::default();
    sync_during_a_move(&runs, &gate, &root, &harness, MEETING);

    fs::remove_dir_all(meetings.path().join(MEETING)).unwrap();
    let err = sync_through(&runs, &gate, &root, &harness, MEETING).unwrap_err();
    assert_eq!(err.kind, SYNC_NOT_SAVED, "{}", err.message);
    assert!(err.message.contains(URL), "{}", err.message);
    assert!(runs.unsaved.get("TICK-0001", Some(MEETING)).is_some());
    assert_eq!(harness.runs(), 1);

    dismiss(&runs, &gate, &root);
    assert_eq!(runs.unsaved.get("TICK-0001", Some(MEETING)), None);
}

/// The window's Dismiss on `MEETING`'s `TICK-0001`.
fn dismiss(runs: &SyncRuns, gate: &FolderGate, root: &Arc<Mutex<PathBuf>>) {
    let lookup = || Ok(root.lock().unwrap().clone());
    runs.unsaved
        .dismiss(Some(gate), &lookup, "TICK-0001", Some(MEETING));
}

/// The kinds the window switches on (`useTicketSync.ts`), and the ones the
/// save is built from, by name.
#[test]
fn error_kinds_keep_their_names() {
    assert_eq!(SYNC_NOT_SAVED, "sync-not-saved");
    assert_eq!(SYNC_NOT_ATTACHED, "sync-not-attached");
    assert_eq!(SYNC_KEPT_UNREADABLE, "sync-kept-unreadable");
    assert_eq!(ALREADY_SYNCED, "sync-already-synced");
    assert_eq!(TICKET_MISSING, "ticket-missing");
    let root = meetings_root();
    let err = find_ticket(root.path(), "TICK-0009", Some(MEETING)).unwrap_err();
    assert_eq!(err.kind, TICKET_MISSING);
}

#[test]
fn a_run_with_nothing_unsaved_still_needs_an_agent() {
    let meetings = meetings_root();
    let err = sync_in(
        &SyncRuns::default(),
        None,
        || Ok(meetings.path().to_path_buf()),
        "TICK-0001",
        Some(MEETING),
        || Err(UiError::app("sync-no-agent", "no agent")),
    )
    .unwrap_err();
    assert_eq!(err.kind, "sync-no-agent");
}

// --- a kept issue across an app restart (TUR-21) ----------------------------

/// A save refused by a folder move, then the end of that move, which writes
/// the kept issue the move held back. The `SyncRuns` is dropped afterwards,
/// as when the app quits.
fn fail_a_save_then_quit(gate: &FolderGate, root: &Arc<Mutex<PathBuf>>, harness: &MovingHarness) {
    let runs = SyncRuns::default();
    let err = sync_during_a_move(&runs, gate, root, harness, MEETING);
    assert_eq!(err.kind, SYNC_NOT_SAVED, "{}", err.message);
    let refused = kept::path(&root.lock().unwrap());
    assert!(!refused.exists(), "written into a folder that is moving");
    runs.unsaved.flush(&|| Ok(root.lock().unwrap().clone()));
    assert!(refused.is_file(), "the kept issue is not on disk");
}

#[test]
fn a_failed_save_survives_a_restart_and_retry_saves_it_without_a_second_run() {
    let meetings = meetings_root();
    let root = Arc::new(Mutex::new(meetings.path().to_path_buf()));
    let harness = MovingHarness::new(&root, None);
    let gate = FolderGate::default();
    fail_a_save_then_quit(&gate, &root, &harness);
    assert!(ticket_in(meetings.path(), MEETING).synced_to().is_none());

    // The app starts again with nothing in memory; Retry saves the issue.
    let runs = SyncRuns::default();
    let summary = sync_through(&runs, &gate, &root, &harness, MEETING).unwrap();
    assert_eq!(summary.external_id.as_deref(), Some("ENG-42"));
    assert_eq!(summary.external_url.as_deref(), Some(URL));
    assert_eq!(harness.runs(), 1, "a second issue");
    let back = ticket_in(meetings.path(), MEETING);
    assert_eq!(
        back.frontmatter.get_str("external_url").as_deref(),
        Some(URL)
    );
    assert!(!kept::path(meetings.path()).exists(), "saved, so forgotten");

    // And after another restart it is the usual "already synced".
    let again = sync_through(&SyncRuns::default(), &gate, &root, &harness, MEETING).unwrap_err();
    assert_eq!(again.kind, ALREADY_SYNCED, "{}", again.message);
    assert_eq!(harness.runs(), 1);
}

#[test]
fn a_kept_issue_is_on_disk_at_once_when_only_the_ticket_is_missing() {
    let meetings = meetings_root();
    let root = Arc::new(Mutex::new(meetings.path().to_path_buf()));
    let harness = MovingHarness::new(&root, None);
    let gate = FolderGate::default();
    let path = ticket_path(meetings.path());
    let aside = meetings.path().join("TICK-0001.md.aside");
    let moves_it_aside = MovingHarness::changing_the_ticket(&root, |root| {
        fs::rename(ticket_path(root), root.join("TICK-0001.md.aside")).unwrap();
    });

    let err =
        sync_through(&SyncRuns::default(), &gate, &root, &moves_it_aside, MEETING).unwrap_err();
    assert_eq!(err.kind, SYNC_NOT_SAVED, "{}", err.message);
    assert!(kept::path(meetings.path()).is_file());

    // Restarted, with the file back as it was: Retry saves without a run.
    fs::rename(&aside, &path).unwrap();
    let summary = sync_through(&SyncRuns::default(), &gate, &root, &harness, MEETING).unwrap();
    assert_eq!(summary.external_id.as_deref(), Some("ENG-42"));
    assert_eq!(moves_it_aside.runs() + harness.runs(), 1, "a second issue");
}

#[test]
fn a_kept_issue_whose_ticket_changed_before_the_restart_is_dropped() {
    let meetings = meetings_root();
    let root = Arc::new(Mutex::new(meetings.path().to_path_buf()));
    let harness = MovingHarness::new(&root, None);
    let gate = FolderGate::default();
    fail_a_save_then_quit(&gate, &root, &harness);

    // While the app was closed, a notes re-run gave the number to another
    // task: the kept issue is not this task's, so the Sync runs as usual.
    give_the_number_to_another_task(meetings.path());
    let runs = SyncRuns::default();
    let summary = sync_through(&runs, &gate, &root, &harness, MEETING).unwrap();
    assert_eq!(harness.runs(), 2);
    assert_eq!(summary.external_id.as_deref(), Some("ENG-43"));
    let back = ticket_in(meetings.path(), MEETING);
    assert_eq!(back.title().as_deref(), Some("Book the offsite"));
    assert_eq!(runs.unsaved.get("TICK-0001", Some(MEETING)), None);
    assert!(
        !kept::path(meetings.path()).exists(),
        "the stale entry stayed"
    );
}

/// The link was saved, but the app quit before the file forgot the issue:
/// the next Sync drops the entry and does not run again.
#[test]
fn a_kept_issue_already_saved_before_the_restart_is_dropped() {
    let meetings = meetings_root();
    let root = Arc::new(Mutex::new(meetings.path().to_path_buf()));
    let harness = MovingHarness::new(&root, None);
    let gate = FolderGate::default();
    fail_a_save_then_quit(&gate, &root, &harness);
    let synced = Synced {
        external_id: "ENG-42".into(),
        external_url: URL.into(),
    };
    record(&ticket_path(meetings.path()), "linear", &synced).unwrap();

    let err = sync_through(&SyncRuns::default(), &gate, &root, &harness, MEETING).unwrap_err();
    assert_eq!(err.kind, ALREADY_SYNCED, "{}", err.message);
    assert_eq!(harness.runs(), 1);
    assert!(!kept::path(meetings.path()).exists());
}

#[test]
fn a_dismissed_issue_stays_dismissed_after_a_restart() {
    let meetings = meetings_root();
    let root = Arc::new(Mutex::new(meetings.path().to_path_buf()));
    let harness = MovingHarness::new(&root, None);
    let gate = FolderGate::default();
    fail_a_save_then_quit(&gate, &root, &harness);

    dismiss(&SyncRuns::default(), &gate, &root);
    assert!(!kept::path(meetings.path()).exists());
    let summary = sync_through(&SyncRuns::default(), &gate, &root, &harness, MEETING).unwrap();
    assert_eq!(harness.runs(), 2, "the dismissed issue was saved");
    assert_eq!(summary.external_id.as_deref(), Some("ENG-43"));
}

/// After a restart, a Sync pressed while a move is running still finds the
/// kept issue (reading needs no gate) and does not run the agent.
#[test]
fn a_kept_issue_is_found_after_a_restart_even_during_a_move() {
    let meetings = meetings_root();
    let root = Arc::new(Mutex::new(meetings.path().to_path_buf()));
    let harness = MovingHarness::new(&root, None);
    let gate = FolderGate::default();
    fail_a_save_then_quit(&gate, &root, &harness);

    let runs = SyncRuns::default();
    let err = sync_during_a_move(&runs, &gate, &root, &harness, MEETING);
    assert_eq!(err.kind, SYNC_NOT_SAVED, "{}", err.message);
    assert!(err.message.contains(URL), "{}", err.message);
    assert_eq!(harness.runs(), 1, "a second issue");
}

/// A file of kept issues that cannot be read refuses the Sync instead of
/// running it, and is never written over.
#[test]
fn an_unreadable_kept_file_refuses_the_sync_and_is_kept() {
    let meetings = meetings_root();
    let root = Arc::new(Mutex::new(meetings.path().to_path_buf()));
    let harness = MovingHarness::new(&root, None);
    let gate = FolderGate::default();
    let file = kept::path(meetings.path());
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, "{ half written").unwrap();

    let runs = SyncRuns::default();
    let err = sync_through(&runs, &gate, &root, &harness, MEETING).unwrap_err();
    assert_eq!(err.kind, SYNC_KEPT_UNREADABLE, "{}", err.message);
    assert!(
        err.message.contains(&file.display().to_string()),
        "{}",
        err.message
    );
    assert_eq!(harness.runs(), 0);
    dismiss(&runs, &gate, &root);
    assert_eq!(fs::read_to_string(&file).unwrap(), "{ half written");

    // Deleted by the user: Sync runs as usual.
    fs::remove_file(&file).unwrap();
    sync_through(&runs, &gate, &root, &harness, MEETING).unwrap();
    assert_eq!(harness.runs(), 1);
}

/// Kept issues are filed by meeting on disk too: the other meeting's
/// `TICK-0001` never gets this one's link after a restart.
#[test]
fn a_kept_issue_read_back_is_only_saved_to_its_own_meetings_ticket() {
    let meetings = meetings_root();
    add_meeting(meetings.path(), OTHER_MEETING);
    let root = Arc::new(Mutex::new(meetings.path().to_path_buf()));
    let harness = MovingHarness::new(&root, None);
    let gate = FolderGate::default();
    fail_a_save_then_quit(&gate, &root, &harness);

    let runs = SyncRuns::default();
    let other = sync_through(&runs, &gate, &root, &harness, OTHER_MEETING).unwrap();
    assert_eq!(other.external_id.as_deref(), Some("ENG-43"));
    let first = sync_through(&runs, &gate, &root, &harness, MEETING).unwrap();
    assert_eq!(first.external_id.as_deref(), Some("ENG-42"));
    assert_eq!(harness.runs(), 2);
}
