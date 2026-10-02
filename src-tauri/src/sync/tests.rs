//! Sync with a fake agent: `FakeHarness`, and fake `claude` / `codex`
//! scripts run through the real harnesses. No real CLI, no tracker.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
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
    let (path, synced) = run(
        root,
        "TICK-0001",
        Some(MEETING),
        harness,
        &settings(),
        &CancelHandle::new(),
    )?;
    record(&path, &settings().tickets.tracker, &synced)
}

fn reply(id: serde_json::Value, url: serde_json::Value) -> FakeHarness {
    FakeHarness::new(FakeBehavior::Reply(
        json!({ "external_id": id, "external_url": url }),
    ))
}

#[test]
fn a_reply_with_a_key_and_link_is_written_to_the_ticket() {
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
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
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
    assert!(stdin.contains(store::MEETING_FILE), "{stdin}");
    assert!(!stdin.contains(SECRET), "the transcript leaked: {stdin}");
}

/// What Codex does today when its MCP call is refused (`approval: never`):
/// exit 0, and a reply with no issue key.
#[test]
fn a_refused_codex_sync_exits_0_and_is_not_synced() {
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
