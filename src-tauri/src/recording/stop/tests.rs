//! TUR-161: every ending goes through [`close`] in one order, and an
//! interrupted recording gets the notes run a Stop would.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::sync::{Arc, Mutex};

use super::*;
use crate::config::{AgentConfig, ConfigError, Harness};

/// `agent` as `config.jsonc` would give it, with `auto_run` as asked.
fn agent(auto_run: bool) -> Result<AgentConfig, ConfigError> {
    Ok(AgentConfig {
        harness: Harness::ClaudeCode,
        model: None,
        binary_path: None,
        auto_run,
        timeout_sec: 30,
    })
}

/// A [`Close`] with no app and no Core Audio. It logs each step, and its
/// after-Stop handling asks the real notes-run decision
/// (`agent_run::starts_notes_after_stop`) about a real meeting folder.
struct Fake<'a> {
    root: &'a Path,
    meeting_id: &'a str,
    agent: Result<AgentConfig, ConfigError>,
    stop: fn() -> Result<(), String>,
    log: RefCell<Vec<String>>,
    notes_started: Cell<Option<bool>>,
}

impl<'a> Fake<'a> {
    fn new(root: &'a Path, meeting_id: &'a str, auto_run: bool) -> Self {
        Self {
            root,
            meeting_id,
            agent: agent(auto_run),
            stop: || Ok(()),
            log: RefCell::default(),
            notes_started: Cell::new(None),
        }
    }
}

impl Close for Fake<'_> {
    type Session = ();
    type Transcript = ();

    fn stop_session(&self, (): ()) -> Result<(), String> {
        self.log.borrow_mut().push("stop session".into());
        (self.stop)()
    }

    fn finish_transcript(&self, (): (), after_stop: bool) {
        self.log.borrow_mut().push(format!(
            "finish transcript, after-Stop handling {after_stop}"
        ));
        let started = after_stop
            && crate::agent_run::starts_notes_after_stop(self.root, self.meeting_id, &self.agent);
        self.notes_started.set(Some(started));
    }
}

/// A meetings root with one meeting whose transcript has a line in it.
fn meeting(id: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("transcript.md"), "[00:00:04] You: Morning.\n").unwrap();
    root
}

const MEETING: &str = "2026-10-10-1100-meeting";

/// Run `close` for `ending` the way the recorder does, with a session and a
/// transcript, and return what `end` was given.
fn end_with(fake: &Fake<'_>, ending: Ending) -> Option<Result<(), String>> {
    close(fake, Some(()), Some(()), ending, |stopped| {
        fake.log.borrow_mut().push("end".into());
        stopped
    })
}

#[test]
fn only_a_start_that_never_happened_skips_the_after_stop_handling() {
    assert!(Ending::Stopped.after_stop());
    assert!(Ending::Interrupted.after_stop());
    assert!(!Ending::NeverStarted.after_stop());
}

#[test]
fn every_ending_closes_the_audio_then_the_transcript_then_ends() {
    let root = meeting(MEETING);
    for ending in [Ending::Stopped, Ending::Interrupted, Ending::NeverStarted] {
        let fake = Fake::new(root.path(), MEETING, true);
        assert_eq!(end_with(&fake, ending), Some(Ok(())), "{ending:?}");
        assert_eq!(
            *fake.log.borrow(),
            [
                "stop session".to_string(),
                format!(
                    "finish transcript, after-Stop handling {}",
                    ending.after_stop()
                ),
                "end".to_string(),
            ],
            "{ending:?}"
        );
    }
}

/// TUR-161: a recording cut short by a disk error or a failed device reopen
/// gets the notes run a Stop would, when `agent.auto_run` is on.
#[test]
fn an_interrupted_recording_starts_the_notes_run_when_auto_run_is_on() {
    let root = meeting(MEETING);
    let fake = Fake::new(root.path(), MEETING, true);
    end_with(&fake, Ending::Interrupted);
    assert_eq!(fake.notes_started.get(), Some(true));
}

#[test]
fn an_interrupted_recording_starts_no_notes_run_when_auto_run_is_off() {
    let root = meeting(MEETING);
    let fake = Fake::new(root.path(), MEETING, false);
    end_with(&fake, Ending::Interrupted);
    assert_eq!(fake.notes_started.get(), Some(false));
}

/// The same as a Stop, either way.
#[test]
fn an_interrupted_recording_and_a_stopped_one_agree_about_the_notes_run() {
    let root = meeting(MEETING);
    for auto_run in [true, false] {
        let ended = [Ending::Stopped, Ending::Interrupted].map(|ending| {
            let fake = Fake::new(root.path(), MEETING, auto_run);
            end_with(&fake, ending);
            fake.notes_started.get()
        });
        assert_eq!(ended, [Some(auto_run); 2], "auto_run {auto_run}");
    }
}

#[test]
fn a_start_that_never_happened_starts_no_notes_run_even_with_auto_run_on() {
    let root = meeting(MEETING);
    let fake = Fake::new(root.path(), MEETING, true);
    end_with(&fake, Ending::NeverStarted);
    assert_eq!(fake.notes_started.get(), Some(false));
}

/// A stop that panics is an error for `end`, and the transcript is still
/// finished: the recorder must never be stranded in `Stopping`.
#[test]
fn a_panic_while_stopping_still_finishes_the_transcript_and_ends() {
    let root = meeting(MEETING);
    let mut fake = Fake::new(root.path(), MEETING, true);
    fake.stop = || panic!("the header patch has a bug");
    let stopped = end_with(&fake, Ending::Interrupted);
    let message = stopped.expect("a session was stopped").unwrap_err();
    assert!(message.contains("stopping panicked"), "{message}");
    assert!(message.contains("the header patch has a bug"), "{message}");
    assert_eq!(fake.log.borrow().last().map(String::as_str), Some("end"));
    assert_eq!(fake.notes_started.get(), Some(true));
}

/// A session lost with its ticker: nothing to stop, the transcript is still
/// finished, and `end` hears there was no session.
#[test]
fn a_lost_session_still_finishes_the_transcript() {
    let root = meeting(MEETING);
    let fake = Fake::new(root.path(), MEETING, true);
    let stopped = close(&fake, None, Some(()), Ending::Stopped, |stopped| stopped);
    assert_eq!(stopped, None);
    assert_eq!(
        *fake.log.borrow(),
        ["finish transcript, after-Stop handling true"]
    );
}

/// TUR-97 end to end, minus Core Audio: a tick that fails on the ticker
/// thread leaves the recorder `Idle` with the reason on its status — the
/// status `recording://state` carries, and the only place it is said —
/// and the next start clears it.
#[test]
fn a_failing_tick_leaves_an_idle_status_that_says_why_until_the_next_start() {
    let recorder = Arc::new(Recorder::default());
    {
        let mut inner = recorder.lock();
        inner.enter_starting().expect("a fresh recorder is idle");
        inner.status.phase = Phase::Recording;
        inner.status.meeting_id = Some("2026-09-30-1300-meeting".into());
    }

    // `fail_mid_recording`'s own two halves, with the parts that need an
    // `AppHandle` and a real session — stopping it, the emits and the
    // notification — left out. The phase seen between them is recorded
    // so the test also sees the `Stopping` the window is shown.
    let between = Arc::new(Mutex::new(None));
    let on_fail = {
        let (recorder, between) = (Arc::clone(&recorder), Arc::clone(&between));
        move |state: u32, message: String| {
            let Some((stopping, transcription)) = recorder.claim_interrupted(&message) else {
                return Some(state);
            };
            assert!(transcription.is_none(), "this test starts no transcript");
            *between.lock().unwrap() = Some(stopping.phase);
            let (idle, error) = recorder.end_interrupted(&message, None);
            assert_eq!(
                idle.error.as_ref().map(|e| &e.message),
                Some(&error.message)
            );
            None
        }
    };
    let ticker = Ticker::spawn(
        "recording-test-fail",
        std::time::Duration::from_millis(1),
        0u32,
        |_state: &mut u32| Err("mic fsync: disk full".to_string()),
        on_fail,
    )
    .expect("spawns");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while recorder.status().phase != Phase::Idle {
        assert!(
            std::time::Instant::now() < deadline,
            "the failed tick never ended the recording"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(ticker.stop().expect("joins cleanly"), None);
    assert_eq!(*between.lock().unwrap(), Some(Phase::Stopping));

    let status = recorder.status();
    assert!(status.meeting_id.is_none());
    assert!(status.started_at_ms.is_none());
    let error = status.error.expect("the idle status says why");
    assert_eq!(error.kind, "recording-interrupted");
    assert!(
        error.message.contains("mic fsync: disk full"),
        "{}",
        error.message
    );

    let starting = recorder
        .lock()
        .enter_starting()
        .expect("idle again, so a new start is allowed");
    assert_eq!(starting.phase, Phase::Starting);
    assert!(
        starting.error.is_none(),
        "a new start clears the old reason"
    );
}

/// A tick that fails while a user's stop is already under way leaves the
/// ending to that stop: nothing is claimed, no error is set, and the
/// session goes back through the join.
#[test]
fn a_failed_tick_during_a_user_stop_leaves_the_ending_to_that_stop() {
    let recorder = Recorder::default();
    recorder.lock().status.phase = Phase::Stopping;
    assert!(recorder.claim_interrupted("mic fsync: disk full").is_none());
    let status = recorder.status();
    assert_eq!(status.phase, Phase::Stopping);
    assert!(status.error.is_none());
}
