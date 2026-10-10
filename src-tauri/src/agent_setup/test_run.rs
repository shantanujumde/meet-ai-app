//! The Test button: a 3-line sample meeting through the picked CLI.
//!
//! It is the real notes run, not a lighter check: the wrap-up prompt (the
//! user's own template if they saved one), the notes schema, no tools, a
//! fresh empty folder and the configured time limit. If the test passes, the
//! run after a real call has everything it needs too.
//!
//! The sample is three made-up lines. Neither the prompt nor the reply is
//! logged; a failure is logged by its kind only.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use agent::{CancelHandle, Job};
use prompts::notes::{Notes, notes_schema};
use prompts::wrap_up::{DEFAULT_WRAP_UP, Target, WrapUpInput, render_wrap_up, render_wrap_up_from};

use super::harness::{self, ErrorKind, Found, NotFound, agent_error};
use super::{AgentChoice, AgentTestResult, AgentTestTask};
use crate::cancels::{Cancels, Claim};
use crate::config::{self, AgentConfig};
use crate::error::UiError;
use crate::meetings;

/// The sample meeting, the same three lines the agent crate's own real-CLI
/// test uses: a decision, a task with an owner and a due day, and an open
/// question.
pub(super) const SAMPLE_TRANSCRIPT: &str = "\
[00:00:01] Ana: Let's ship the beta Friday.
[00:00:09] Ben: I'll write the release notes by Thursday.
[00:00:15] Ana: Do we need legal sign-off?
";

/// The sample meeting's title, as the prompt shows it.
const SAMPLE_TITLE: &str = "meet-ai test run";

/// The Test runs going, so quitting stops their CLIs (TUR-160). Managed
/// Tauri state. Each press gets its own key: two Tests side by side are
/// allowed, as before.
#[derive(Debug, Default)]
pub struct TestRuns {
    runs: Cancels<u64>,
    next: AtomicU64,
}

impl TestRuns {
    /// A place for one Test run, refused once the app is quitting.
    pub(super) fn claim(&self) -> Result<Claim<'_, u64>, UiError> {
        let key = self.next.fetch_add(1, Ordering::Relaxed);
        self.runs.claim(key).map_err(|_refused| {
            UiError::app(
                "app-quitting",
                "meet-ai is quitting, so the test did not run.",
            )
        })
    }

    /// Cancel every Test run going (Cancel on the Test screen): each one's
    /// CLI process tree is killed and its `test_agent` ends with
    /// `agent-cancelled`. A no-op when none is going. Every run, not one by
    /// key: the window does not know a run's key, and two Tests going at
    /// once (onboarding and Settings) is rare enough that Cancel stopping
    /// both is the plain answer.
    pub(super) fn cancel_all(&self) {
        for key in self.runs.keys() {
            self.runs.cancel(&key);
        }
    }

    /// The app is quitting: start no more Test runs, cancel the ones going,
    /// which kills their CLIs' process trees, and wait up to `wait` for them
    /// to end. On unix the CLI leads its own process group, so without this
    /// it outlived the app.
    pub fn shutdown(&self, wait: Duration) {
        let left = self.runs.shutdown(wait);
        if left > 0 {
            tracing::warn!(left, "agent test runs still going at quit");
        }
    }
}

/// Find the picked CLI and run the sample through it; `cancel` stops it.
pub(super) fn run(choice: &AgentChoice, cancel: &CancelHandle) -> Result<AgentTestResult, UiError> {
    let found = find(choice)?;
    let timeout = timeout();
    // No meetings folder yet (it is picked in the step before) only means
    // there is no saved template either; the built-in one is used.
    let root = meetings::root().ok();
    run_sample(
        found.harness.as_ref(),
        Some(&found.sign_in),
        choice.model().as_deref(),
        timeout,
        std::env::temp_dir(),
        root.as_deref(),
        cancel,
    )
}

/// The picked CLI, found where `choice` says (`choice.binary_path` first).
/// An error, without running anything, for `none` or a CLI that is not
/// found. No sign-in check: the Test itself is the check, and a CLI that is
/// signed out says so when it runs.
pub(super) fn find(choice: &AgentChoice) -> Result<Found, UiError> {
    let settings = AgentConfig {
        harness: choice.harness.into(),
        binary_path: choice.binary_path(),
        ..AgentConfig::default()
    };
    harness::harness_for(&settings, false).map_err(|missing| match missing {
        NotFound::NoAgent => UiError::app(
            "agent-none",
            "No agent is picked, so there is nothing to test.",
        ),
        NotFound::Agent { error, sign_in } => agent_error(error, Some(&sign_in)),
    })
}

/// `agent.timeout_sec`. A config the app cannot read (the screen is often
/// open to fix one) falls back to the default limit rather than refusing.
fn timeout() -> Duration {
    let seconds = match config::agent() {
        Ok(agent) => agent.timeout_sec,
        Err(error) => {
            tracing::warn!(%error, "agent test: config.jsonc unreadable; using the default time limit");
            agent::DEFAULT_TIMEOUT_SECS
        }
    };
    Duration::from_secs(seconds)
}

/// The notes run on the sample meeting, through `harness`.
///
/// `sign_in` is what to type to sign the CLI in, for the error when it is
/// signed out. `model` blank lets the CLI pick its own. `work_root` is where
/// the run's fresh folder is made. `meetings_root` is where a saved wrap-up
/// template is looked for; `None` uses the built-in one. `cancel` stops the
/// CLI.
pub(super) fn run_sample(
    harness: &dyn agent::Harness,
    sign_in: Option<&str>,
    model: Option<&str>,
    timeout: Duration,
    work_root: PathBuf,
    meetings_root: Option<&Path>,
    cancel: &CancelHandle,
) -> Result<AgentTestResult, UiError> {
    let mut job = Job::notes(sample_prompt(meetings_root)?, notes_schema().clone());
    let model = model.map(str::trim).filter(|model| !model.is_empty());
    job.model = model.map(str::to_owned);
    job.timeout = timeout;
    job.work_root = work_root;
    job.cancel = cancel.clone();

    let started = Instant::now();
    let reply = harness.run(&job).map_err(|error| {
        let error = with_model(agent_error(error, sign_in), model);
        tracing::warn!(
            harness = harness.id(),
            kind = error.kind,
            "agent test run failed"
        );
        error
    })?;
    let seconds = whole_seconds(started.elapsed());
    // The harness already checked the reply against this schema; this turns
    // it into the typed notes.
    let notes = Notes::from_value(reply)
        .map_err(|error| UiError::app(ErrorKind::SchemaMismatch.key(), error.to_string()))?;
    tracing::info!(
        harness = harness.id(),
        seconds,
        tasks = notes.tasks.len(),
        "agent test run finished"
    );
    Ok(result(notes, seconds))
}

/// The wrap-up prompt for the sample meeting, as the agent run gets it.
pub(super) fn sample_prompt(meetings_root: Option<&Path>) -> Result<String, UiError> {
    let input = WrapUpInput {
        title: SAMPLE_TITLE.to_owned(),
        date: chrono::Local::now()
            .format("%Y-%m-%dT%H:%M:%S%:z")
            .to_string(),
        transcript: SAMPLE_TRANSCRIPT.to_owned(),
        notes: String::new(),
    };
    Ok(match meetings_root {
        Some(root) => render_wrap_up_from(root, &input, &Target::Agent)?,
        None => render_wrap_up(DEFAULT_WRAP_UP, &input, &Target::Agent)?,
    })
}

/// `elapsed` rounded to the nearest second.
fn whole_seconds(elapsed: Duration) -> u32 {
    u32::try_from(elapsed.as_millis().saturating_add(500) / 1000).unwrap_or(u32::MAX)
}

fn result(notes: Notes, seconds: u32) -> AgentTestResult {
    AgentTestResult {
        summary: notes.summary,
        decisions: notes.decisions,
        open_questions: notes.open_questions,
        tasks: notes
            .tasks
            .into_iter()
            .map(|task| AgentTestTask {
                title: task.title,
                owner: task.owner,
                due: task.due,
            })
            .collect(),
        seconds,
    }
}

/// `error` with the model the test ran, for a failure a wrong model name can
/// cause, so "Test" says which name failed. Nothing is added with no model:
/// the CLI picked its own.
fn with_model(mut error: UiError, model: Option<&str>) -> UiError {
    if let Some(model) = model
        && matches!(error.kind, "agent-failed" | "agent-could-not-start")
    {
        error.message = format!("{} (model: {model})", error.message);
    }
    error
}
