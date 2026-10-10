//! "Send a test ticket" with a fake agent: `FakeHarness`, and a fake
//! `claude` (`test_support::FakeCli`) through the real harness.

use std::fs;
use std::time::Duration;

use agent::AgentError;
use agent::fake::{FakeBehavior, FakeHarness};
use serde_json::json;

use super::*;
use crate::config::{AgentConfig, Harness as HarnessChoice};
use crate::sync::harness_for;

fn settings() -> RunSettings {
    RunSettings {
        model: None,
        timeout: Duration::from_secs(20),
        tickets: TicketsConfig::default(),
        sign_in: None,
    }
}

fn check_with(behavior: FakeBehavior) -> Result<TrackerCheck, UiError> {
    check(
        &FakeHarness::new(behavior),
        &settings(),
        &CancelHandle::new(),
    )
}

#[test]
fn the_schema_parses() {
    let schema = schema();
    assert_eq!(schema["required"], json!(["project", "refused_reason"]));
}

#[test]
fn claude_reads_the_project_with_only_the_tracker_tools_and_no_task() {
    let bin = tempfile::tempdir().unwrap();
    let cli = test_support::FakeCli::install(bin.path(), "claude");
    cli.set("log_dir", bin.path().display().to_string());
    let envelope = json!({
        "type": "result", "subtype": "success", "is_error": false,
        "structured_output": { "project": "Engineering", "refused_reason": null },
    });
    cli.set("stdout", envelope.to_string());
    let agent = AgentConfig {
        harness: HarnessChoice::ClaudeCode,
        binary_path: Some(cli.path().to_path_buf()),
        ..AgentConfig::default()
    };
    let harness = harness_for(&agent).unwrap().harness;

    let found = check(harness.as_ref(), &settings(), &CancelHandle::new()).unwrap();

    assert_eq!(found.project, "Engineering");
    assert_eq!(
        found.message,
        "Claude Code reached Linear. New tickets will go to Engineering."
    );
    let args = fs::read_to_string(bin.path().join("argv.log")).unwrap();
    let args: Vec<&str> = args.lines().collect();
    let at = args.iter().position(|a| *a == "--allowedTools").unwrap();
    assert_eq!(args[at + 1], "mcp__claude_ai_Linear__*", "{args:?}");
    let stdin = fs::read_to_string(bin.path().join("stdin.log")).unwrap();
    assert!(stdin.contains("read-only check"), "{stdin}");
    assert!(
        stdin.contains("Do not create, change, comment on, move or delete anything."),
        "{stdin}"
    );
    assert!(stdin.contains("\"claude.ai Linear\""), "{stdin}");
    assert!(!stdin.contains("TICK-"), "{stdin}");
}

#[test]
fn a_tracker_that_said_no_is_refused_with_its_reason() {
    let error = check_with(FakeBehavior::Reply(json!({
        "project": null,
        "refused_reason": "no access to team ENG",
    })))
    .unwrap_err();
    assert_eq!(error.kind, SYNC_REFUSED);
    assert_eq!(
        error.message,
        "Linear said no: no access to team ENG. Check the project and your access in Linear, then send the test ticket again."
    );
}

#[test]
fn no_project_and_no_reason_is_unreachable_and_names_the_connection() {
    for reply in [
        json!({ "project": null, "refused_reason": null }),
        json!({ "project": "  ", "refused_reason": "N/A" }),
        json!({ "project": "null", "refused_reason": null }),
    ] {
        let error = check_with(FakeBehavior::Reply(reply.clone())).unwrap_err();
        assert_eq!(error.kind, SYNC_UNREACHABLE, "{reply}");
        assert_eq!(
            error.message,
            "Your agent couldn't reach Linear. Check that \"claude.ai Linear\" is connected and signed in in your agent, then send the test ticket again."
        );
    }
}

#[test]
fn agent_failures_keep_their_kind_and_say_to_check_again() {
    let signed_out = check_with(FakeBehavior::NotSignedIn).unwrap_err();
    assert_eq!(signed_out.kind, "agent-not-signed-in");
    assert!(
        signed_out.message.starts_with("Couldn't check: "),
        "{}",
        signed_out.message
    );
    assert!(
        signed_out
            .message
            .ends_with("then send the test ticket again."),
        "{}",
        signed_out.message
    );

    let cancel = CancelHandle::new();
    cancel.cancel();
    let sleepy = FakeHarness::new(FakeBehavior::Sleep(Duration::from_secs(10)));
    let stopped = check(&sleepy, &settings(), &cancel).unwrap_err();
    assert_eq!(stopped.kind, "agent-cancelled");
    assert_eq!(
        stopped.message,
        "The check stopped before it finished. Send the test ticket again."
    );

    let missing = again(errors::send_error(
        AgentError::NotInstalled {
            harness: "Codex".to_owned(),
        },
        None,
    ));
    assert_eq!(missing.kind, "agent-not-installed");
    assert!(
        missing
            .message
            .starts_with("Couldn't check: Codex isn't installed"),
        "{}",
        missing.message
    );
}

#[test]
fn no_tracker_server_means_no_run() {
    let mut settings = settings();
    settings.tickets.tracker_mcp = " ".to_owned();
    let never = FakeHarness::new(FakeBehavior::Fail {
        code: 9,
        stderr: "should not run".to_owned(),
    });
    let error = check(&never, &settings, &CancelHandle::new()).unwrap_err();
    assert_eq!(error.kind, "sync-no-tracker");
}

#[test]
fn a_project_name_is_one_short_line() {
    let reply = json!({ "project": "  Web\n\tPlatform  " });
    assert_eq!(project_in(&reply).as_deref(), Some("Web Platform"));
    let long = json!({ "project": "x".repeat(300) });
    let cut = project_in(&long).unwrap();
    assert_eq!(cut.chars().count(), MAX_PROJECT_CHARS);
    assert!(cut.ends_with('…'));
    assert_eq!(project_in(&json!({ "project": 7 })), None);
    assert_eq!(capitalized("your agent"), "Your agent");
    assert_eq!(capitalized(""), "");
}
