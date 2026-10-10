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
fn home_is_expanded() {
    let home = Some(Path::new("/home/u"));
    assert_eq!(expand_home("~/bin/x.sh", home), "/home/u/bin/x.sh");
    assert_eq!(expand_home("$HOME/x $HOME", home), "/home/u/x /home/u");
    assert_eq!(expand_home("\"$HOME\\x\"", home), "\"/home/u\\x\"");
    assert_eq!(expand_home("~other/x", home), "~other/x");
    assert_eq!(expand_home("echo ~/x", home), "echo ~/x");
    assert_eq!(expand_home("~/x", None), "~/x");
}

/// TUR-167: only a whole `$HOME` is the home folder.
#[test]
fn homebrew_prefix_is_not_home() {
    let home = Some(Path::new("/Users/me"));
    assert_eq!(
        expand_home("$HOMEBREW_PREFIX/bin/x", home),
        "$HOMEBREW_PREFIX/bin/x"
    );
    assert_eq!(
        expand_home("$HOMEDIR $HOME_X $HOME", home),
        "$HOMEDIR $HOME_X /Users/me"
    );
}

/// TUR-167: `sh` gets the command as written, so its own expansion and
/// quoting apply: `$HOMEBREW_PREFIX` is the variable, `'$HOME'` is literal.
#[test]
fn sh_expands_the_command_itself() {
    if windows() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let report = run_hook(
        "printf '%s|' '$HOME' \"$HOMEBREW_PREFIX_TUR167\" #",
        dir.path(),
        Duration::from_secs(20),
    );
    assert_eq!(report.outcome, Outcome::Ok, "{report:?}");
    assert_eq!(report.stdout, "$HOME||", "{report:?}");
}

/// TUR-167: `"timeout_secs": 99999999999` (meant as "never") must not panic
/// the hook thread; the hook runs and reports.
#[test]
fn a_huge_timeout_does_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let command = if windows() {
        "exit /b 4 & rem"
    } else {
        "exit 4 #"
    };
    let config = HooksConfig {
        on_meeting_end: Some(command.to_owned()),
        timeout_secs: 99_999_999_999,
        ..HooksConfig::default()
    };
    let report = run_moment(&config, Moment::MeetingEnd, dir.path());
    assert_eq!(
        report.map(|r| r.outcome),
        Some(Outcome::Failed { code: Some(4) })
    );
    let report = run_hook(command, dir.path(), Duration::MAX);
    assert_eq!(report.outcome, Outcome::Failed { code: Some(4) });
}

/// TUR-167: a hook that backgrounds a child holding its pipes returns soon
/// after it exits, and the child is killed, so neither it nor the threads
/// reading its pipes are left behind.
#[test]
fn a_backgrounded_grandchild_does_not_outlive_the_hook() {
    let dir = tempfile::tempdir().unwrap();
    let late = dir.path().join("late.txt");
    let late_s = late.to_string_lossy();
    let command = if windows() {
        format!(
            "start /b cmd /c \"%SystemRoot%\\System32\\ping.exe -n 6 127.0.0.1 >nul & echo late> {late_s}\" & echo started & rem"
        )
    } else {
        format!("(sleep 4; echo late > '{late_s}') & echo started #")
    };
    let started = Instant::now();
    let report = run_hook(&command, dir.path(), Duration::from_secs(60));
    assert_eq!(report.outcome, Outcome::Ok, "{report:?}");
    assert!(report.stdout.contains("started"), "{report:?}");
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
    std::thread::sleep(Duration::from_secs(6));
    assert!(!late.exists(), "the hook's child outlived the hook");
}

/// TUR-167: a running hook holds the folder gate, so a folder move is
/// refused with `folder-busy` until it ends.
#[test]
fn a_running_hook_holds_the_folder_gate() {
    use crate::folder_move::FolderGate;
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("m")).unwrap();
    let marker = root.path().join("m").join("ran");
    let command = if windows() {
        "%SystemRoot%\\System32\\ping.exe -n 3 127.0.0.1 >nul & echo x> ran & rem"
    } else {
        "sleep 1.5; echo x > ran #"
    };
    let config = HooksConfig {
        on_meeting_end: Some(command.to_owned()),
        ..HooksConfig::default()
    };
    let gate = FolderGate::default();
    let root_path = root.path().to_path_buf();
    std::thread::scope(|scope| {
        let hook = scope.spawn(|| {
            app::run_gated(
                Some(&gate),
                || Ok(root_path.clone()),
                &config,
                Moment::MeetingEnd,
                "m",
            )
        });
        std::thread::sleep(Duration::from_millis(500));
        let refused = gate.begin_move().err().map(|e| e.kind);
        assert_eq!(refused, Some("folder-busy"));
        let ran = hook.join().unwrap();
        assert!(matches!(ran, Ok(None)), "{ran:?}");
    });
    assert!(marker.exists());
    assert!(
        gate.begin_move().is_ok(),
        "the gate opens once the hook ends"
    );
}

#[test]
fn a_hook_due_during_a_folder_move_does_not_run() {
    use crate::folder_move::FolderGate;
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("m")).unwrap();
    let config = HooksConfig {
        on_meeting_end: Some("echo x > ran #".to_owned()),
        ..HooksConfig::default()
    };
    let gate = FolderGate::default();
    let _moving = gate.begin_move().unwrap();
    let ran = app::run_gated(
        Some(&gate),
        || Ok(root.path().to_path_buf()),
        &config,
        Moment::MeetingEnd,
        "m",
    );
    assert_eq!(ran.err().map(|e| e.kind), Some("folder-move-in-progress"));
    assert!(!root.path().join("m").join("ran").exists());
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
