//! TUR-10: the notes run with the fake agent CLI, on real meeting folders in
//! the system temp folder.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use agent::fake::{FakeBehavior, FakeHarness};
use agent::{AgentError, CancelHandle};
use store::agent_notes::AGENT_NOTES_KEY;
use store::meeting::Meeting;
use store::watcher::SelfWrites;

use super::notes::{self, Agent, SaveWrite};
use super::runs::{AgentRuns, Sink, Work};
use super::{
    Failure, FailureKind, State, Status, after_stop, failure, meeting_of, read_meeting_notes,
    switch_notes,
};

const STANDUP: &str = "2026-09-01-1430-standup";

const TRANSCRIPT: &str = "\
[00:00:04] Others: Morning everyone, let's start with the API work.
[00:14:22] You: Sessions are still in memory, that's the blocker.
[00:21:05] Others: Then Redis it is: I'll take the load test.
";

/// A reply in the notes format, as `crates/store/tests/fixtures/notes/standup.json`.
fn standup_reply() -> serde_json::Value {
    serde_json::json!({
        "summary": "Sessions still live in memory. The team agreed to move them to Redis.",
        "decisions": ["Move sessions to Redis."],
        "open_questions": ["Who owns the Redis cluster after launch?"],
        "tasks": [
            {
                "title": "Redis session store",
                "details": "Replace the in-memory session map with Redis.",
                "owner": "Shantanu",
                "due": "Friday",
                "transcript_ref": "00:14:22"
            },
            {
                "title": "Load test the login path",
                "details": "Run the login load test against two API instances.",
                "owner": "Priya",
                "due": null,
                "transcript_ref": "00:21:05"
            }
        ]
    })
}

/// A throwaway meetings root.
struct Root {
    _guard: tempfile::TempDir,
    path: PathBuf,
}

impl Root {
    fn new() -> Self {
        let guard = tempfile::Builder::new()
            .prefix("meet-ai-agent-run-")
            .tempdir()
            .unwrap();
        Self {
            path: guard.path().to_path_buf(),
            _guard: guard,
        }
    }

    /// A meeting folder with `meeting.md` (title and date, and `extra`
    /// frontmatter) and, unless `None`, a transcript.
    fn meeting(&self, id: &str, transcript: Option<&str>, extra: &[(&str, &str)]) -> PathBuf {
        let dir = self.path.join(id);
        fs::create_dir_all(&dir).unwrap();
        let mut meeting = Meeting::new(id, "Standup");
        meeting
            .frontmatter
            .set_str("date", Some("2026-09-01T14:30:00+05:30"));
        for (key, value) in extra {
            meeting.frontmatter.set_str(key, Some(value));
        }
        meeting.write(&dir.join(store::MEETING_FILE)).unwrap();
        if let Some(text) = transcript {
            fs::write(dir.join(store::TRANSCRIPT_FILE), text).unwrap();
        }
        dir
    }

    fn standup(&self) -> PathBuf {
        self.meeting(STANDUP, Some(TRANSCRIPT), &[])
    }
}

/// Every file under `dir`, by relative path, with its bytes.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(next) = todo.pop() {
        for entry in fs::read_dir(&next).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                todo.push(path);
            } else {
                let bytes = fs::read(&path).unwrap();
                files.insert(path.strip_prefix(dir).unwrap().to_path_buf(), bytes);
            }
        }
    }
    files
}

/// The fake CLI doing `behavior`, found through `detect` as the real one is.
fn fake(behavior: FakeBehavior, timeout: Duration) -> impl FnOnce() -> Result<Agent, Failure> {
    move || {
        let harness = notes::detected(FakeHarness::new(behavior.clone()), |_| {
            FakeHarness::new(behavior)
        })?;
        Ok(Agent {
            harness: Box::new(harness),
            model: Some("fake-small".into()),
            timeout,
            work_root: std::env::temp_dir(),
        })
    }
}

/// One run straight through the pipeline, on this thread.
fn run(root: &Root, behavior: FakeBehavior, timeout: Duration) -> Result<u32, Failure> {
    notes::run_notes(
        &root.path,
        STANDUP,
        fake(behavior, timeout),
        &CancelHandle::new(),
        &SelfWrites::default(),
        in_root(&root.path),
    )
}

/// Saves into `root`, as a run does when the folder has not moved.
fn in_root(root: &Path) -> impl FnOnce(SaveWrite<'_>) -> Result<u32, Failure> + '_ {
    move |write| write(root)
}

/// The same run as [`Work`] for [`AgentRuns::start`].
fn work(root: &Root, behavior: FakeBehavior, timeout: Duration) -> Work {
    let path = root.path.clone();
    Box::new(move |cancel| {
        notes::run_notes(
            &path,
            STANDUP,
            fake(behavior, timeout),
            cancel,
            &SelfWrites::default(),
            in_root(&path),
        )
    })
}

/// Records what the window and the notification would have been told.
#[derive(Default)]
struct Recorded {
    statuses: Mutex<Vec<Status>>,
    ready: Mutex<Vec<(String, u32)>>,
}

impl Sink for Recorded {
    fn status(&self, status: &Status) {
        self.statuses.lock().unwrap().push(status.clone());
    }
    fn notes_ready(&self, meeting_id: &str, tasks: u32) {
        self.ready
            .lock()
            .unwrap()
            .push((meeting_id.to_owned(), tasks));
    }
}

impl Recorded {
    fn states(&self) -> Vec<State> {
        let statuses = self.statuses.lock().unwrap();
        statuses.iter().map(|s| s.state.clone()).collect()
    }

    /// Waits for the run to end and returns how it ended.
    fn wait_for_end(&self) -> State {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(last) = self.states().last()
                && *last != State::Running
            {
                return last.clone();
            }
            assert!(Instant::now() < deadline, "the run never ended");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn failed(kind: FailureKind) -> impl Fn(&State) -> bool {
    move |state| matches!(state, State::Failed { failure } if failure.kind == kind)
}

const LONG: Duration = Duration::from_secs(30);

#[test]
fn a_run_writes_the_notes_and_tasks_and_says_so() {
    crate::platform::skip_without_fake_cli!();
    let root = Root::new();
    let dir = root.standup();
    let runs = AgentRuns::default();
    let sink = Arc::new(Recorded::default());

    let started = runs.start(
        STANDUP,
        work(&root, FakeBehavior::Reply(standup_reply()), LONG),
        sink.clone(),
    );
    assert_eq!(started.state, State::Running);
    assert_eq!(sink.wait_for_end(), State::Done { tasks: 2 });

    assert_eq!(sink.states(), [State::Running, State::Done { tasks: 2 }]);
    assert_eq!(*sink.ready.lock().unwrap(), [(STANDUP.to_owned(), 2)]);
    assert_eq!(runs.status(STANDUP).state, State::Done { tasks: 2 });

    let meeting = Meeting::read(&dir.join(store::MEETING_FILE))
        .unwrap()
        .unwrap();
    assert!(meeting.section("Summary").unwrap().contains("Redis"));
    assert_eq!(
        meeting.frontmatter.get_str("analyzed_by").as_deref(),
        Some("claude-code")
    );
    assert_eq!(
        meeting.frontmatter.get_str("analyzed_model").as_deref(),
        Some("fake-small")
    );
    let mut tickets: Vec<String> = fs::read_dir(dir.join(store::TICKETS_DIR))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("TICK-"))
        .collect();
    tickets.sort();
    assert_eq!(tickets, ["TICK-0001.md", "TICK-0002.md"]);

    let notes = read_meeting_notes(&root.path, STANDUP).unwrap();
    assert!(!notes.notes_off);
    assert_eq!(notes.analyzed_by.as_deref(), Some("claude-code"));
    let headings: Vec<&str> = notes.sections.iter().map(|s| s.heading.as_str()).collect();
    assert_eq!(
        headings,
        ["Summary", "Decisions", "Action Items", "Open Questions"]
    );
}

#[test]
fn each_agent_failure_writes_nothing_and_says_why() {
    crate::platform::skip_without_fake_cli!();
    let cases: Vec<(FakeBehavior, Duration, FailureKind, &str)> = vec![
        (
            FakeBehavior::NotInstalled,
            LONG,
            FailureKind::NotInstalled,
            "not installed",
        ),
        (
            FakeBehavior::NotSignedIn,
            LONG,
            FailureKind::NotSignedIn,
            "not signed in",
        ),
        (
            FakeBehavior::Fail {
                code: 2,
                stderr: "  rate limit reached\n".into(),
            },
            LONG,
            FailureKind::CliFailed,
            "rate limit reached",
        ),
        (
            FakeBehavior::Stdout("Sure! Here are your notes.".into()),
            LONG,
            FailureKind::BadReply,
            "did not match the notes format",
        ),
        (
            FakeBehavior::Reply(serde_json::json!({ "summary": "only this" })),
            LONG,
            FailureKind::BadReply,
            "nothing was written",
        ),
        (
            FakeBehavior::Sleep(Duration::from_secs(20)),
            Duration::from_millis(300),
            FailureKind::TimedOut,
            "did not finish within",
        ),
    ];
    for (behavior, timeout, kind, says) in cases {
        let root = Root::new();
        let dir = root.standup();
        let before = snapshot(&dir);
        let label = format!("{behavior:?}");

        let failure = run(&root, behavior, timeout).unwrap_err();

        assert_eq!(failure.kind, kind, "{label}");
        assert!(
            failure.message.contains(says),
            "{label}: {}",
            failure.message
        );
        assert_eq!(snapshot(&dir), before, "{label} changed the folder");
    }
}

#[test]
fn not_signed_in_gives_the_command_to_sign_in() {
    let root = Root::new();
    root.standup();
    let failure = run(&root, FakeBehavior::NotSignedIn, LONG).unwrap_err();
    assert_eq!(failure.command.as_deref(), Some("claude"));
    assert!(failure.message.contains("/login"), "{}", failure.message);

    let claude = failure::from_agent(
        &AgentError::NotSignedIn {
            harness: "Claude Code".into(),
        },
        agent::claude::ID,
    );
    assert!(claude.message.starts_with("Claude Code is not signed in"));
    let codex = failure::from_agent(
        &AgentError::NotSignedIn {
            harness: "Codex".into(),
        },
        "codex",
    );
    assert_eq!(codex.command.as_deref(), Some("codex login"));
    assert!(codex.message.contains("codex login"), "{}", codex.message);
}

#[test]
fn the_remaining_agent_errors_map_to_plain_words() {
    let timed_out = failure::from_agent(
        &AgentError::TimedOut {
            after: Duration::from_secs(300),
        },
        "claude-code",
    );
    assert!(timed_out.message.contains("300 seconds"));
    assert!(timed_out.message.contains("agent.timeout_sec"));

    let cancelled = failure::from_agent(&AgentError::Cancelled, "claude-code");
    assert_eq!(cancelled.kind, FailureKind::Cancelled);

    let silent = failure::from_agent(
        &AgentError::CliFailed {
            status: Some(1),
            stderr: " \n".into(),
        },
        "claude-code",
    );
    assert!(silent.message.contains("exit code 1"), "{}", silent.message);

    let long = failure::from_agent(
        &AgentError::CliFailed {
            status: Some(1),
            stderr: format!("{}the real error", "x".repeat(2000)),
        },
        "claude-code",
    );
    assert!(long.message.ends_with("the real error"));
    assert!(long.message.chars().count() < 500, "{}", long.message);

    let schema = failure::from_agent(
        &AgentError::SchemaMismatch {
            errors: vec!["/tasks: missing".into()],
        },
        "claude-code",
    );
    assert_eq!(schema.kind, FailureKind::BadReply);

    let start = failure::from_agent(
        &AgentError::CouldNotStart {
            reason: "no temp folder".into(),
        },
        "claude-code",
    );
    assert_eq!(start.kind, FailureKind::CouldNotStart);
    assert!(start.message.contains("no temp folder"));
}

#[test]
fn a_meeting_with_no_transcript_is_not_sent() {
    let root = Root::new();
    let dir = root.meeting(STANDUP, None, &[]);
    let before = snapshot(&dir);
    assert_eq!(
        run(&root, FakeBehavior::Reply(standup_reply()), LONG)
            .unwrap_err()
            .kind,
        FailureKind::NoTranscript
    );
    fs::write(dir.join(store::TRANSCRIPT_FILE), "  \n").unwrap();
    assert_eq!(
        run(&root, FakeBehavior::Reply(standup_reply()), LONG)
            .unwrap_err()
            .kind,
        FailureKind::NoTranscript
    );
    fs::remove_file(dir.join(store::TRANSCRIPT_FILE)).unwrap();
    assert_eq!(snapshot(&dir), before);

    // No folder at all.
    let missing = notes::run_notes(
        &root.path,
        "2026-09-02-1000-nothing",
        fake(FakeBehavior::Reply(standup_reply()), LONG),
        &CancelHandle::new(),
        &SelfWrites::default(),
        in_root(&root.path),
    );
    assert_eq!(missing.unwrap_err().kind, FailureKind::NoTranscript);
}

#[test]
fn a_meeting_with_notes_off_is_never_sent() {
    let root = Root::new();
    let dir = root.meeting(STANDUP, Some(TRANSCRIPT), &[(AGENT_NOTES_KEY, "off")]);
    let before = snapshot(&dir);
    let asked = Arc::new(AtomicBool::new(false));
    let asked_in = Arc::clone(&asked);

    let result = notes::run_notes(
        &root.path,
        STANDUP,
        move || {
            asked_in.store(true, Ordering::SeqCst);
            fake(FakeBehavior::Reply(standup_reply()), LONG)()
        },
        &CancelHandle::new(),
        &SelfWrites::default(),
        in_root(&root.path),
    );

    assert_eq!(result.unwrap_err().kind, FailureKind::NotesOff);
    assert!(
        !asked.load(Ordering::SeqCst),
        "the agent was looked for at all"
    );
    assert_eq!(snapshot(&dir), before);
    assert!(read_meeting_notes(&root.path, STANDUP).unwrap().notes_off);
}

#[test]
fn cancel_stops_the_run_and_leaves_the_meeting_as_it_was() {
    crate::platform::skip_without_fake_cli!();
    let root = Root::new();
    let dir = root.standup();
    let before = snapshot(&dir);
    let runs = AgentRuns::default();
    let sink = Arc::new(Recorded::default());

    runs.start(
        STANDUP,
        work(&root, FakeBehavior::Sleep(Duration::from_secs(20)), LONG),
        sink.clone(),
    );
    std::thread::sleep(Duration::from_millis(200));
    let started = Instant::now();
    assert_eq!(runs.cancel(STANDUP).state, State::Running);

    let ended = sink.wait_for_end();
    assert!(failed(FailureKind::Cancelled)(&ended), "{ended:?}");
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(sink.ready.lock().unwrap().is_empty());
    assert_eq!(snapshot(&dir), before);
    // Nothing left to cancel.
    assert!(failed(FailureKind::Cancelled)(&runs.cancel(STANDUP).state));
}

#[test]
fn a_second_start_while_running_is_ignored() {
    crate::platform::skip_without_fake_cli!();
    let root = Root::new();
    root.standup();
    let runs = AgentRuns::default();
    let sink = Arc::new(Recorded::default());
    let second_ran = Arc::new(AtomicU32::new(0));

    runs.start(
        STANDUP,
        work(&root, FakeBehavior::Sleep(Duration::from_secs(20)), LONG),
        sink.clone(),
    );
    let counter = Arc::clone(&second_ran);
    let again = runs.start(
        STANDUP,
        Box::new(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(0)
        }),
        sink.clone(),
    );

    assert_eq!(again.state, State::Running);
    assert_eq!(sink.states(), [State::Running], "a second Running went out");
    runs.cancel(STANDUP);
    sink.wait_for_end();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(second_ran.load(Ordering::SeqCst), 0);

    // Once it has ended, Retry starts a new one.
    let retry = runs.start(
        STANDUP,
        work(&root, FakeBehavior::Reply(standup_reply()), LONG),
        sink.clone(),
    );
    assert_eq!(retry.state, State::Running);
    let deadline = Instant::now() + Duration::from_secs(10);
    while runs.status(STANDUP).state == State::Running {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(runs.status(STANDUP).state, State::Done { tasks: 2 });
}

#[test]
fn quitting_mid_run_leaves_the_meeting_and_retry_works_next_launch() {
    crate::platform::skip_without_fake_cli!();
    let root = Root::new();
    let dir = root.standup();
    let before = snapshot(&dir);
    let runs = AgentRuns::default();
    let sink = Arc::new(Recorded::default());

    runs.start(
        STANDUP,
        work(&root, FakeBehavior::Sleep(Duration::from_secs(20)), LONG),
        sink.clone(),
    );
    std::thread::sleep(Duration::from_millis(200));
    let quit = Instant::now();
    runs.shutdown(Duration::from_secs(5));

    assert!(
        quit.elapsed() < Duration::from_secs(5),
        "shutdown waited it out"
    );
    assert!(runs.is_closed());
    assert_eq!(sink.states(), [State::Running], "the quit was reported");
    assert!(sink.ready.lock().unwrap().is_empty());
    assert_eq!(runs.status(STANDUP).state, State::Idle);
    assert_eq!(snapshot(&dir), before);

    // Nothing starts while quitting.
    let ran = Arc::new(AtomicBool::new(false));
    let ran_in = Arc::clone(&ran);
    let ignored = runs.start(
        STANDUP,
        Box::new(move |_| {
            ran_in.store(true, Ordering::SeqCst);
            Ok(0)
        }),
        sink.clone(),
    );
    assert_eq!(ignored.state, State::Idle);
    std::thread::sleep(Duration::from_millis(50));
    assert!(!ran.load(Ordering::SeqCst));

    // Next launch: a fresh state, and Retry writes the notes.
    let next_launch = AgentRuns::default();
    let sink = Arc::new(Recorded::default());
    assert_eq!(next_launch.status(STANDUP).state, State::Idle);
    next_launch.start(
        STANDUP,
        work(&root, FakeBehavior::Reply(standup_reply()), LONG),
        sink.clone(),
    );
    assert_eq!(sink.wait_for_end(), State::Done { tasks: 2 });
    assert!(dir.join(store::TICKETS_DIR).join("TICK-0001.md").is_file());
}

#[test]
fn a_run_that_panics_ends_as_a_failure() {
    let runs = AgentRuns::default();
    let sink = Arc::new(Recorded::default());
    runs.start(STANDUP, Box::new(|_| panic!("boom")), sink.clone());
    let ended = sink.wait_for_end();
    assert!(failed(FailureKind::CouldNotStart)(&ended), "{ended:?}");
}

#[test]
fn the_transcript_path_names_the_meeting() {
    let path = Path::new("/m/2026-09-01-1430-standup").join(store::TRANSCRIPT_FILE);
    assert_eq!(
        meeting_of(&path),
        Some((PathBuf::from("/m"), "2026-09-01-1430-standup".to_owned()))
    );
}

#[test]
fn a_meeting_with_no_notes_yet_has_no_sections() {
    let root = Root::new();
    root.meeting(STANDUP, Some(TRANSCRIPT), &[]);
    let notes = read_meeting_notes(&root.path, STANDUP).unwrap();
    assert!(notes.sections.is_empty());
    assert_eq!(notes.analyzed_by, None);

    let none = read_meeting_notes(&root.path, "2026-09-02-1000-nothing").unwrap();
    assert!(none.sections.is_empty());
    assert!(read_meeting_notes(&root.path, "../escape").is_err());
}

// ---------------------------------------------------------------------------
// "Make notes for this meeting" (SPEC A11, TUR-12)
// ---------------------------------------------------------------------------

/// Flip the switch the way `set_meeting_notes` does.
fn switch(root: &Root, agent_runs: Option<&AgentRuns>, on: bool) -> super::MeetingNotes {
    switch_notes(&root.path, agent_runs, STANDUP, on, &SelfWrites::default()).unwrap()
}

#[test]
fn switched_off_before_stop_the_meeting_is_never_sent() {
    let root = Root::new();
    // As a call leaves it: a transcript, no meeting.md yet.
    let dir = root.path.join(STANDUP);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(store::TRANSCRIPT_FILE), TRANSCRIPT).unwrap();
    let mut expected = snapshot(&dir);

    assert!(switch(&root, None, false).notes_off);
    let switched = snapshot(&dir);
    let meeting_md = PathBuf::from(store::MEETING_FILE);
    expected.insert(meeting_md.clone(), switched[&meeting_md].clone());
    assert_eq!(switched, expected, "the switch wrote only meeting.md");

    let asked = Arc::new(AtomicBool::new(false));
    let asked_in = Arc::clone(&asked);
    let result = notes::run_notes(
        &root.path,
        STANDUP,
        move || {
            asked_in.store(true, Ordering::SeqCst);
            fake(FakeBehavior::Reply(standup_reply()), LONG)()
        },
        &CancelHandle::new(),
        &SelfWrites::default(),
        in_root(&root.path),
    );

    assert_eq!(result.unwrap_err().kind, FailureKind::NotesOff);
    assert!(
        !asked.load(Ordering::SeqCst),
        "no agent was looked for, so none could start"
    );
    assert_eq!(snapshot(&dir), switched);
}

#[test]
fn switched_back_on_make_notes_now_writes_the_notes() {
    crate::platform::skip_without_fake_cli!();
    let root = Root::new();
    let dir = root.standup();
    assert!(switch(&root, None, false).notes_off);
    let back_on = switch(&root, None, true);
    assert!(!back_on.notes_off);
    assert!(back_on.sections.is_empty(), "no notes yet");

    assert_eq!(
        run(&root, FakeBehavior::Reply(standup_reply()), LONG).unwrap(),
        2
    );

    let notes = read_meeting_notes(&root.path, STANDUP).unwrap();
    assert!(!notes.notes_off);
    assert_eq!(notes.analyzed_by.as_deref(), Some("claude-code"));
    assert!(dir.join(store::TICKETS_DIR).join("TICK-0001.md").is_file());
}

#[test]
fn switched_off_while_the_agent_works_nothing_is_written() {
    crate::platform::skip_without_fake_cli!();
    let root = Root::new();
    let dir = root.standup();
    let root_path = root.path.clone();

    let result = notes::run_notes(
        &root.path,
        STANDUP,
        move || {
            // The meeting was read and found on; the user flips the switch
            // before the agent answers.
            notes_switch_off(&root_path);
            fake(FakeBehavior::Reply(standup_reply()), LONG)()
        },
        &CancelHandle::new(),
        &SelfWrites::default(),
        in_root(&root.path),
    );

    assert_eq!(result.unwrap_err().kind, FailureKind::NotesOff);
    assert!(!dir.join(store::TICKETS_DIR).exists());
    let notes = read_meeting_notes(&root.path, STANDUP).unwrap();
    assert!(notes.notes_off);
    assert_eq!(notes.analyzed_by, None);
    assert!(notes.sections.is_empty());
}

fn notes_switch_off(root: &Path) {
    store::notes_switch::set(root, STANDUP, false, &SelfWrites::default()).unwrap();
}

#[test]
fn switching_off_stops_the_running_run_and_says_notes_off() {
    crate::platform::skip_without_fake_cli!();
    let root = Root::new();
    let dir = root.standup();
    let runs = AgentRuns::default();
    let sink = Arc::new(Recorded::default());

    runs.start(
        STANDUP,
        work(&root, FakeBehavior::Sleep(Duration::from_secs(20)), LONG),
        sink.clone(),
    );
    std::thread::sleep(Duration::from_millis(200));
    let started = Instant::now();
    assert!(switch(&root, Some(&runs), false).notes_off);

    let ended = sink.wait_for_end();
    assert!(failed(FailureKind::NotesOff)(&ended), "{ended:?}");
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(sink.ready.lock().unwrap().is_empty());
    assert!(!dir.join(store::TICKETS_DIR).exists());
    assert_eq!(
        read_meeting_notes(&root.path, STANDUP).unwrap().analyzed_by,
        None
    );
}

#[test]
fn switching_on_leaves_a_running_run_alone() {
    crate::platform::skip_without_fake_cli!();
    let root = Root::new();
    root.standup();
    let runs = AgentRuns::default();
    let sink = Arc::new(Recorded::default());

    runs.start(
        STANDUP,
        work(&root, FakeBehavior::Reply(standup_reply()), LONG),
        sink.clone(),
    );
    switch(&root, Some(&runs), true);

    assert_eq!(sink.wait_for_end(), State::Done { tasks: 2 });
}

#[test]
fn the_switch_refuses_a_bad_meeting_id() {
    let root = Root::new();
    let result = switch_notes(&root.path, None, "../escape", false, &SelfWrites::default());
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// After Stop: a final transcript, no run when notes are off, a folder move
// (TUR-17)
// ---------------------------------------------------------------------------

/// `agent` as `config.jsonc` would give it, with `auto_run` and `harness` as
/// asked.
fn settings(auto_run: bool, harness: crate::config::Harness) -> crate::config::AgentConfig {
    crate::config::AgentConfig {
        harness,
        model: Some("fake-small".into()),
        binary_path: None,
        auto_run,
        timeout_sec: 30,
    }
}

fn auto_run_on() -> Result<crate::config::AgentConfig, crate::config::ConfigError> {
    Ok(settings(true, crate::config::Harness::ClaudeCode))
}

/// A transcript that is final at once.
fn already_final(_: Duration) -> bool {
    true
}

#[test]
fn a_slow_transcript_save_makes_the_run_wait_for_the_last_line() {
    crate::platform::skip_without_fake_cli!();
    let root = Root::new();
    let dir = root.meeting(STANDUP, Some("[00:00:04] Others: Morning everyone.\n"), &[]);
    let runs = AgentRuns::default();
    let sink = Arc::new(Recorded::default());
    let last_line = "[00:21:05] Others: Then Redis it is: I'll take the load test.";
    let transcript = dir.join(store::TRANSCRIPT_FILE);

    let started_with = Mutex::new(None);
    after_stop(
        &runs,
        &root.path,
        STANDUP,
        &auto_run_on(),
        |_| {
            // The engine is still saving: its last line lands, then it is done.
            std::thread::sleep(Duration::from_millis(200));
            let mut text = fs::read_to_string(&transcript).unwrap();
            text.push_str(last_line);
            text.push('\n');
            fs::write(&transcript, text).unwrap();
            true
        },
        sink.as_ref(),
        || {
            *started_with.lock().unwrap() = Some(fs::read_to_string(&transcript).unwrap());
            runs.start(
                STANDUP,
                work(&root, FakeBehavior::Reply(standup_reply()), LONG),
                sink.clone(),
            );
        },
    );

    let seen = started_with
        .lock()
        .unwrap()
        .clone()
        .expect("the run started");
    assert!(
        seen.contains(last_line),
        "the run started before the last line: {seen}"
    );
    assert_eq!(sink.wait_for_end(), State::Done { tasks: 2 });
}

#[test]
fn a_transcript_that_never_becomes_final_starts_no_run_and_offers_retry() {
    crate::platform::skip_without_fake_cli!();
    let root = Root::new();
    let dir = root.standup();
    let before = snapshot(&dir);
    let runs = AgentRuns::default();
    let sink = Arc::new(Recorded::default());
    let started = AtomicBool::new(false);

    after_stop(
        &runs,
        &root.path,
        STANDUP,
        &auto_run_on(),
        |wait| {
            assert_eq!(wait, super::FINAL_WAIT);
            false
        },
        sink.as_ref(),
        || started.store(true, Ordering::SeqCst),
    );

    assert!(
        !started.load(Ordering::SeqCst),
        "no run on half a transcript"
    );
    let states = sink.states();
    assert_eq!(states.len(), 1, "one status, no Writing notes…: {states:?}");
    assert!(failed(FailureKind::CouldNotStart)(&states[0]), "{states:?}");
    let State::Failed { failure } = &states[0] else {
        unreachable!()
    };
    assert!(failure.message.contains("Retry"), "{}", failure.message);
    assert_eq!(runs.status(STANDUP).state, states[0]);
    assert_eq!(snapshot(&dir), before, "nothing written");

    // Retry, once the transcript is there, runs as usual.
    runs.start(
        STANDUP,
        work(&root, FakeBehavior::Reply(standup_reply()), LONG),
        sink.clone(),
    );
    assert_eq!(sink.wait_for_end(), State::Done { tasks: 2 });
}

#[test]
fn notes_off_at_stop_creates_no_run_at_all() {
    let root = Root::new();
    let dir = root.meeting(STANDUP, Some(TRANSCRIPT), &[(AGENT_NOTES_KEY, "off")]);
    let before = snapshot(&dir);
    let runs = AgentRuns::default();
    let sink = Arc::new(Recorded::default());
    let started = AtomicBool::new(false);

    after_stop(
        &runs,
        &root.path,
        STANDUP,
        &auto_run_on(),
        |_| panic!("nothing waits for a meeting whose notes are off"),
        sink.as_ref(),
        || started.store(true, Ordering::SeqCst),
    );

    assert!(!started.load(Ordering::SeqCst));
    assert!(sink.states().is_empty(), "no status at all");
    assert_eq!(runs.status(STANDUP).state, State::Idle);
    assert_eq!(snapshot(&dir), before);
}

#[test]
fn notes_switched_off_while_the_transcript_saves_creates_no_run() {
    let root = Root::new();
    root.standup();
    let runs = AgentRuns::default();
    let sink = Arc::new(Recorded::default());
    let started = AtomicBool::new(false);

    after_stop(
        &runs,
        &root.path,
        STANDUP,
        &auto_run_on(),
        |_| {
            notes_switch_off(&root.path);
            false
        },
        sink.as_ref(),
        || started.store(true, Ordering::SeqCst),
    );

    assert!(!started.load(Ordering::SeqCst));
    assert!(sink.states().is_empty(), "not even the not-final failure");
}

#[test]
fn auto_run_off_or_no_agent_starts_nothing_and_on_writes_the_notes() {
    crate::platform::skip_without_fake_cli!();
    use crate::config::{ConfigError, Harness};

    let root = Root::new();
    let dir = root.standup();
    let before = snapshot(&dir);
    let runs = AgentRuns::default();
    let sink = Arc::new(Recorded::default());
    let start = || {
        runs.start(
            STANDUP,
            work(&root, FakeBehavior::Reply(standup_reply()), LONG),
            sink.clone(),
        );
    };

    for off in [
        settings(false, Harness::ClaudeCode),
        settings(true, Harness::None),
    ] {
        after_stop(
            &runs,
            &root.path,
            STANDUP,
            &Ok(off.clone()),
            already_final,
            sink.as_ref(),
            start,
        );
        assert!(sink.states().is_empty(), "{off:?} started a run");
    }
    assert_eq!(snapshot(&dir), before);

    // A config that cannot be read still starts, so the view says why.
    let unreadable: Result<_, ConfigError> = Err(ConfigError::Invalid("broken".into()));
    assert!(super::auto_runs(&unreadable));

    after_stop(
        &runs,
        &root.path,
        STANDUP,
        &auto_run_on(),
        already_final,
        sink.as_ref(),
        start,
    );
    assert_eq!(sink.wait_for_end(), State::Done { tasks: 2 });
    assert_eq!(
        read_meeting_notes(&root.path, STANDUP)
            .unwrap()
            .analyzed_by
            .as_deref(),
        Some("claude-code")
    );
}

#[test]
fn a_folder_moved_mid_run_gets_the_notes_in_its_new_place() {
    crate::platform::skip_without_fake_cli!();
    let parent = Root::new();
    let old_root = parent.path.join("Meetings");
    let new_root = parent.path.join("Moved");
    let root = Root {
        _guard: tempfile::tempdir().unwrap(),
        path: old_root.clone(),
    };
    root.standup();

    let gate = crate::folder_move::FolderGate::default();
    let looked_up = AtomicBool::new(false);
    let result = notes::run_notes(
        &old_root,
        STANDUP,
        || {
            // The user moves the meetings folder while the agent works.
            let moving = gate.begin_move().unwrap();
            fs::rename(&old_root, &new_root).unwrap();
            drop(moving);
            fake(FakeBehavior::Reply(standup_reply()), LONG)()
        },
        &CancelHandle::new(),
        &SelfWrites::default(),
        |write| {
            crate::folder_move::writing_in(
                Some(&gate),
                || {
                    looked_up.store(true, Ordering::SeqCst);
                    Ok(new_root.clone())
                },
                write,
            )
        },
    );

    assert_eq!(result, Ok(2));
    assert!(
        looked_up.load(Ordering::SeqCst),
        "the root was looked up at save time"
    );
    assert!(
        !old_root.exists(),
        "nothing written back under the old root"
    );
    let notes = read_meeting_notes(&new_root, STANDUP).unwrap();
    assert!(!notes.sections.is_empty(), "the notes are in the new place");
    assert!(new_root.join(STANDUP).join(store::TICKETS_DIR).is_dir());
}

#[test]
fn a_save_during_a_folder_move_is_refused_and_writes_nothing() {
    crate::platform::skip_without_fake_cli!();
    let root = Root::new();
    let dir = root.standup();
    let before = snapshot(&dir);
    let gate = crate::folder_move::FolderGate::default();
    let _moving = gate.begin_move().unwrap();

    let result = notes::run_notes(
        &root.path,
        STANDUP,
        fake(FakeBehavior::Reply(standup_reply()), LONG),
        &CancelHandle::new(),
        &SelfWrites::default(),
        |write| crate::folder_move::writing_in(Some(&gate), || Ok(root.path.clone()), write),
    );

    assert_eq!(result.unwrap_err().kind, FailureKind::WriteFailed);
    assert_eq!(snapshot(&dir), before);
}

// ---------------------------------------------------------------------------
// The failures the TUR-10 review found untested
// ---------------------------------------------------------------------------

#[test]
fn a_run_that_cannot_start_writes_nothing_and_says_why() {
    crate::platform::skip_without_fake_cli!();
    let root = Root::new();
    let dir = root.standup();
    let before = snapshot(&dir);
    let missing = root.path.join("no-such-folder");

    let result = notes::run_notes(
        &root.path,
        STANDUP,
        move || {
            let mut agent = fake(FakeBehavior::Reply(standup_reply()), LONG)()?;
            // The working folder the CLI would run in is not there.
            agent.work_root = missing;
            Ok(agent)
        },
        &CancelHandle::new(),
        &SelfWrites::default(),
        in_root(&root.path),
    );

    let failure = result.unwrap_err();
    assert_eq!(failure.kind, FailureKind::CouldNotStart);
    assert!(
        failure.message.contains("working folder"),
        "{}",
        failure.message
    );
    assert_eq!(snapshot(&dir), before);
}

#[test]
fn notes_that_cannot_be_saved_say_so() {
    crate::platform::skip_without_fake_cli!();
    let root = Root::new();
    let dir = root.standup();
    // The tickets folder is a file, so no task can be written into it.
    fs::write(dir.join(store::TICKETS_DIR), "not a folder").unwrap();

    let failure = run(&root, FakeBehavior::Reply(standup_reply()), LONG).unwrap_err();

    assert_eq!(failure.kind, FailureKind::WriteFailed);
    assert!(
        failure
            .message
            .starts_with("The notes came back but could not be saved"),
        "{}",
        failure.message
    );
}

#[test]
fn no_agent_set_up_is_said_before_anything_is_sent() {
    let root = Root::new();
    let dir = root.standup();
    let before = snapshot(&dir);

    let result = notes::run_notes(
        &root.path,
        STANDUP,
        || notes::from_settings(&settings(true, crate::config::Harness::None)),
        &CancelHandle::new(),
        &SelfWrites::default(),
        in_root(&root.path),
    );

    let failure = result.unwrap_err();
    assert_eq!(failure.kind, FailureKind::NoAgent);
    assert!(
        failure.message.contains("Copy prompt"),
        "{}",
        failure.message
    );
    assert_eq!(snapshot(&dir), before);
}
