//! Hook runner tests (TUR-63). The shell commands differ per OS; the
//! behaviour checked is the same on all three CI runners.

use std::path::Path;
use std::time::{Duration, Instant};

use super::app::{meeting_ended, notes_done};
use super::*;
use crate::config::HooksConfig;

fn windows() -> bool {
    platform::is_windows()
}

#[test]
fn the_hook_gets_the_meeting_folder_as_env_and_last_argument() {
    let dir = tempfile::tempdir().unwrap();
    let meeting = dir.path().join("2026-10-04 Standup");
    std::fs::create_dir(&meeting).unwrap();
    let command = if windows() {
        "echo env=%MEETAI_MEETING_DIR% arg="
    } else {
        "printf 'env=%s arg=%s' \"$MEETAI_MEETING_DIR\""
    };
    let report = run_hook(command, &meeting, Duration::from_secs(20));
    assert_eq!(report.outcome, Outcome::Ok, "{report:?}");
    let shown = meeting.to_string_lossy().into_owned();
    assert!(
        report.stdout.contains(&format!("env={shown} arg=")),
        "{report:?}"
    );
    let arg = report.stdout.split("arg=").nth(1).unwrap_or_default();
    assert!(
        arg.contains(&shown),
        "folder not the last argument: {report:?}"
    );
}

#[test]
fn a_timeout_kills_the_hook_and_the_child_it_started() {
    let dir = tempfile::tempdir().unwrap();
    let late = dir.path().join("late.txt");
    let late_s = late.to_string_lossy();
    // The child writes `late.txt` after ~2 s, unless it was killed with the
    // hook at 1 s. The trailing comment swallows the folder argument.
    let command = if windows() {
        format!(
            "start /b cmd /c \"%SystemRoot%\\System32\\ping.exe -n 4 127.0.0.1 >nul & echo late> {late_s}\" & %SystemRoot%\\System32\\ping.exe -n 10 127.0.0.1 >nul & rem"
        )
    } else {
        format!("(sleep 2; echo late > '{late_s}') & sleep 10; wait #")
    };
    let started = Instant::now();
    let report = run_hook(&command, dir.path(), Duration::from_secs(1));
    assert_eq!(report.outcome, Outcome::TimedOut, "{report:?}");
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "{:?}",
        started.elapsed()
    );
    std::thread::sleep(Duration::from_secs(5));
    assert!(!late.exists(), "the hook's child outlived the timeout");
}

#[test]
fn a_failing_hook_is_reported_and_the_meeting_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let meeting_md = dir.path().join("meeting.md");
    std::fs::write(&meeting_md, "# Standup\n").unwrap();
    let config = HooksConfig {
        on_meeting_end: Some(
            if windows() {
                "exit /b 3 & rem"
            } else {
                "exit 3 #"
            }
            .to_owned(),
        ),
        ..HooksConfig::default()
    };
    let failed = run_moment(&config, Moment::MeetingEnd, dir.path());
    let report = failed.expect("a failing hook is reported");
    assert_eq!(report.outcome, Outcome::Failed { code: Some(3) });
    assert_eq!(report.outcome.describe(), "exited with status 3");
    // The meeting is as it was, and the other moments run nothing.
    assert_eq!(std::fs::read_to_string(&meeting_md).unwrap(), "# Standup\n");
    assert!(run_moment(&config, Moment::TranscriptReady, dir.path()).is_none());
}

#[test]
fn a_hook_that_cannot_start_is_a_failure_not_a_panic() {
    let missing = Path::new("/no/such/meeting/folder");
    let report = run_hook("echo hi", missing, Duration::from_secs(5));
    assert!(!report.outcome.is_ok(), "{report:?}");
}

#[test]
fn output_is_capped() {
    let big = vec![b'x'; OUTPUT_CAP * 2 + 5];
    assert_eq!(capped(big.as_slice(), OUTPUT_CAP).len(), OUTPUT_CAP);
    assert_eq!(capped(&b"short"[..], OUTPUT_CAP), "short");
}

#[test]
fn home_is_expanded() {
    let home = Some(Path::new("/home/u"));
    assert_eq!(expand_home("~/bin/x.sh", home), "/home/u/bin/x.sh");
    assert_eq!(expand_home("$HOME/x $HOME", home), "/home/u/x /home/u");
    assert_eq!(expand_home("~other/x", home), "~other/x");
    assert_eq!(expand_home("echo ~/x", home), "echo ~/x");
    assert_eq!(expand_home("~/x", None), "~/x");
}

#[test]
fn moments_name_their_config_key() {
    let config = HooksConfig {
        on_transcript_ready: Some("a".into()),
        on_analysis_complete: Some("b".into()),
        on_meeting_end: Some("c".into()),
        timeout_secs: 30,
    };
    for (moment, name, command) in [
        (Moment::TranscriptReady, "on_transcript_ready", "a"),
        (Moment::AnalysisComplete, "on_analysis_complete", "b"),
        (Moment::MeetingEnd, "on_meeting_end", "c"),
    ] {
        assert_eq!(moment.name(), name);
        assert_eq!(moment.command(&config), Some(command));
    }
}

#[test]
fn the_meeting_ends_when_the_recorder_goes_from_a_meeting_to_none() {
    let mut last = None;
    assert_eq!(meeting_ended(&mut last, None), None);
    assert_eq!(meeting_ended(&mut last, Some("m".into())), None);
    assert_eq!(meeting_ended(&mut last, Some("m".into())), None);
    assert_eq!(meeting_ended(&mut last, None), Some("m".into()));
    assert_eq!(meeting_ended(&mut last, None), None);
}

#[test]
fn analysis_completes_only_on_a_done_notes_run() {
    let done = r#"{"meetingId":"m","state":{"state":"done","tasks":2}}"#;
    let running = r#"{"meetingId":"m","state":{"state":"running"}}"#;
    assert_eq!(notes_done(done), Some("m".into()));
    assert_eq!(notes_done(running), None);
    assert_eq!(notes_done("not json"), None);
    // The payload the notes run really sends.
    let real = serde_json::to_string(&crate::agent_run::Status {
        meeting_id: "m".into(),
        state: crate::agent_run::State::Done { tasks: 1 },
    })
    .unwrap();
    assert_eq!(notes_done(&real), Some("m".into()));
}
