//! The fake harness, driven only through `dyn Harness`, the way the app holds
//! the user's pick. Every run goes through the real child-process path.

#![cfg(unix)]

use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use agent::fake::{FakeBehavior, FakeHarness};
use agent::{AgentError, Harness, Job};
use serde_json::{Value, json};

fn notes_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["summary", "decisions", "open_questions", "tasks"],
        "properties": {
            "summary": { "type": "string" },
            "decisions": { "type": "array", "items": { "type": "string" } },
            "open_questions": { "type": "array", "items": { "type": "string" } },
            "tasks": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["title", "details", "owner", "due", "transcript_ref"],
                    "properties": {
                        "title": { "type": "string" },
                        "details": { "type": "string" },
                        "owner": { "type": ["string", "null"] },
                        "due": { "type": ["string", "null"] },
                        "transcript_ref": { "type": "string", "pattern": "^\\d{2}:\\d{2}:\\d{2}$" }
                    }
                }
            }
        }
    })
}

fn good_notes() -> Value {
    json!({
        "summary": "Planned the Q3 launch.",
        "decisions": ["Ship on the 14th"],
        "open_questions": ["Who writes the blog post?"],
        "tasks": [
            {
                "title": "Draft release notes",
                "details": "Cover the new export flow; it's \"done\" now & $HOME stays put.",
                "owner": "Priya",
                "due": null,
                "transcript_ref": "00:12:34"
            },
            {
                "title": "Book the room",
                "details": "",
                "owner": null,
                "due": "2026-10-09",
                "transcript_ref": "01:02:03"
            }
        ]
    })
}

/// A notes job whose working folder is made inside `root`.
fn notes_job(root: &Path) -> Job {
    let mut job = Job::notes(
        "Transcript: [00:12:34] Priya: I'll draft the release notes.",
        notes_schema(),
    );
    job.work_root = root.to_path_buf();
    job
}

fn harness(behavior: FakeBehavior) -> Box<dyn Harness> {
    Box::new(FakeHarness::new(behavior))
}

fn assert_empty(root: &Path) {
    let left: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert!(left.is_empty(), "the run left files behind: {left:?}");
}

#[test]
fn a_reply_that_matches_the_schema_comes_back_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let fake = harness(FakeBehavior::Reply(good_notes()));
    assert_eq!(fake.id(), "fake");
    assert_eq!(fake.run(&notes_job(root.path())).unwrap(), good_notes());
}

#[test]
fn a_reply_that_breaks_the_schema_is_rejected_without_quoting_it() {
    let root = tempfile::tempdir().unwrap();
    let mut reply = good_notes();
    reply["tasks"][0]["transcript_ref"] = json!("SECRET-MINUTE");
    let err = harness(FakeBehavior::Reply(reply))
        .run(&notes_job(root.path()))
        .unwrap_err();
    let AgentError::SchemaMismatch { errors } = &err else {
        panic!("expected SchemaMismatch, got {err:?}")
    };
    let all = errors.join("\n");
    assert!(all.contains("/tasks/0/transcript_ref"), "{all}");
    assert!(!all.contains("SECRET"), "{all}");
    assert!(!err.to_string().contains("SECRET"), "{err}");
}

#[test]
fn a_reply_that_is_not_json_is_invalid_json() {
    let root = tempfile::tempdir().unwrap();
    let err = harness(FakeBehavior::Stdout(
        "Sure! Here are your notes: ...".into(),
    ))
    .run(&notes_job(root.path()))
    .unwrap_err();
    assert!(matches!(err, AgentError::InvalidJson { .. }), "{err:?}");
}

#[test]
fn an_empty_reply_is_invalid_json() {
    let root = tempfile::tempdir().unwrap();
    let err = harness(FakeBehavior::Stdout(String::new()))
        .run(&notes_job(root.path()))
        .unwrap_err();
    assert!(matches!(err, AgentError::InvalidJson { .. }), "{err:?}");
}

#[test]
fn a_cli_that_fails_reports_its_exit_code_and_stderr() {
    let root = tempfile::tempdir().unwrap();
    let fake = harness(FakeBehavior::Fail {
        code: 3,
        stderr: "login expired".into(),
    });
    let err = fake.run(&notes_job(root.path())).unwrap_err();
    let AgentError::CliFailed { status, stderr } = &err else {
        panic!("expected CliFailed, got {err:?}")
    };
    assert_eq!(*status, Some(3));
    assert!(stderr.contains("login expired"), "{stderr:?}");
}

#[test]
fn a_cli_that_is_not_installed_is_not_found_and_does_not_run() {
    let root = tempfile::tempdir().unwrap();
    let fake = harness(FakeBehavior::NotInstalled);
    assert!(fake.detect().is_none());
    let err = fake.run(&notes_job(root.path())).unwrap_err();
    assert!(
        matches!(&err, AgentError::NotInstalled { harness } if harness == "fake"),
        "{err:?}"
    );
}

#[test]
fn a_cli_that_is_not_signed_in_says_so() {
    let root = tempfile::tempdir().unwrap();
    let fake = harness(FakeBehavior::NotSignedIn);
    let install = fake.detect().unwrap();
    assert!(!install.signed_in);
    let err = fake.run(&notes_job(root.path())).unwrap_err();
    assert!(
        matches!(&err, AgentError::NotSignedIn { harness } if harness == "fake"),
        "{err:?}"
    );
}

#[test]
fn an_installed_signed_in_cli_is_detected() {
    let install = harness(FakeBehavior::Reply(good_notes())).detect().unwrap();
    assert!(install.signed_in);
    assert_eq!(install.version.as_deref(), Some("fake 1.0"));
}

#[test]
fn a_broken_schema_fails_before_anything_runs() {
    let root = tempfile::tempdir().unwrap();
    let mut job = notes_job(root.path());
    job.schema = json!({ "type": 12 });
    let start = Instant::now();
    let err = harness(FakeBehavior::Sleep(Duration::from_secs(30)))
        .run(&job)
        .unwrap_err();
    assert!(matches!(err, AgentError::CouldNotStart { .. }), "{err:?}");
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
    assert_empty(root.path());
}

#[test]
fn a_run_past_its_time_limit_is_killed() {
    let root = tempfile::tempdir().unwrap();
    let mut job = notes_job(root.path());
    job.timeout = Duration::from_millis(300);
    let start = Instant::now();
    let err = harness(FakeBehavior::Sleep(Duration::from_secs(30)))
        .run(&job)
        .unwrap_err();
    let elapsed = start.elapsed();
    assert!(
        matches!(err, AgentError::TimedOut { after } if after == Duration::from_millis(300)),
        "{err:?}"
    );
    assert!(elapsed < Duration::from_secs(5), "took {elapsed:?}");
}

#[test]
fn cancel_from_another_thread_kills_the_run() {
    let root = tempfile::tempdir().unwrap();
    let job = notes_job(root.path());
    let cancel = job.cancel.clone();
    let canceller = thread::spawn(move || {
        thread::sleep(Duration::from_millis(200));
        cancel.cancel();
    });
    let start = Instant::now();
    let err = harness(FakeBehavior::Sleep(Duration::from_secs(30)))
        .run(&job)
        .unwrap_err();
    let elapsed = start.elapsed();
    canceller.join().unwrap();
    assert!(matches!(err, AgentError::Cancelled), "{err:?}");
    assert!(elapsed < Duration::from_secs(5), "took {elapsed:?}");
}

#[test]
fn a_job_cancelled_before_it_starts_returns_at_once() {
    let root = tempfile::tempdir().unwrap();
    let job = notes_job(root.path());
    job.cancel.cancel();
    let start = Instant::now();
    let err = harness(FakeBehavior::Sleep(Duration::from_secs(30)))
        .run(&job)
        .unwrap_err();
    assert!(matches!(err, AgentError::Cancelled), "{err:?}");
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn the_working_folder_is_deleted_after_every_run() {
    let root = tempfile::tempdir().unwrap();
    let mut bad = good_notes();
    bad["summary"] = json!(42);
    let behaviors = [
        FakeBehavior::Reply(good_notes()),
        FakeBehavior::Reply(bad),
        FakeBehavior::Stdout("not json".into()),
        FakeBehavior::Fail {
            code: 1,
            stderr: "boom".into(),
        },
    ];
    for behavior in behaviors {
        let result = harness(behavior.clone()).run(&notes_job(root.path()));
        assert_empty(root.path());
        if let FakeBehavior::Reply(value) = &behavior
            && value == &good_notes()
        {
            assert!(result.is_ok(), "{result:?}");
        } else {
            assert!(result.is_err(), "{behavior:?} -> {result:?}");
        }
    }

    let mut job = notes_job(root.path());
    job.timeout = Duration::from_millis(200);
    let result = harness(FakeBehavior::Sleep(Duration::from_secs(30))).run(&job);
    assert!(
        matches!(result, Err(AgentError::TimedOut { .. })),
        "{result:?}"
    );
    assert_empty(root.path());
}

#[test]
fn a_sync_job_runs_the_same_way() {
    let root = tempfile::tempdir().unwrap();
    let schema = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["issue_key", "url"],
        "properties": {
            "issue_key": { "type": "string" },
            "url": { "type": "string" }
        }
    });
    let mut job = Job::sync(
        "Create an issue: Draft release notes",
        schema,
        vec!["mcp__linear__create_issue".into()],
    );
    job.work_root = root.path().to_path_buf();
    let reply = json!({ "issue_key": "ENG-42", "url": "https://linear.app/acme/issue/ENG-42" });
    assert_eq!(
        harness(FakeBehavior::Reply(reply.clone()))
            .run(&job)
            .unwrap(),
        reply
    );
    assert_empty(root.path());
}
