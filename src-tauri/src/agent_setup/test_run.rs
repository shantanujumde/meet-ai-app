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

use agent::{AgentError, CancelHandle, ClaudeHarness, CodexHarness, Install, Job};
use prompts::notes::{Notes, notes_schema};
use prompts::wrap_up::{DEFAULT_WRAP_UP, Target, WrapUpInput, render_wrap_up, render_wrap_up_from};

use super::{AgentChoice, AgentCliId, AgentHarness, AgentTestResult, AgentTestTask};
use crate::cancels::{Cancels, Claim};
use crate::config;
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
    let harness = harness_for(choice)?;
    let timeout = timeout();
    // No meetings folder yet (it is picked in the step before) only means
    // there is no saved template either; the built-in one is used.
    let root = meetings::root().ok();
    run_sample(
        harness.as_ref(),
        choice.model().as_deref(),
        timeout,
        std::env::temp_dir(),
        root.as_deref(),
        cancel,
    )
}

/// The harness for the picked CLI, pointed at the copy detection finds
/// (`choice.binary_path` first). An error, without running anything, for
/// `none` or a CLI that is not found.
fn harness_for(choice: &AgentChoice) -> Result<Box<dyn agent::Harness>, UiError> {
    let path = choice.binary_path();
    let path = path.as_deref();
    match choice.harness {
        AgentHarness::None => Err(UiError::app(
            "agent-none",
            "No agent is picked, so there is nothing to test.",
        )),
        AgentHarness::ClaudeCode => {
            let install = found(AgentCliId::ClaudeCode, agent::detect::claude(path))?;
            Ok(Box::new(claude_harness(&install.path)))
        }
        AgentHarness::Codex => {
            let install = found(AgentCliId::Codex, agent::detect::codex(path))?;
            Ok(Box::new(CodexHarness::with_binary(install.path)))
        }
    }
}

/// `install`, or the agent-not-installed error naming `id`.
pub(super) fn found(id: AgentCliId, install: Option<Install>) -> Result<Install, UiError> {
    install.ok_or_else(|| {
        agent_error(AgentError::NotInstalled {
            harness: id.name().to_owned(),
        })
    })
}

/// Claude Code at `path`. Its own folder goes first on the child's `PATH`:
/// an npm install is a node script, and an app opened from Finder does not
/// have `node`'s folder on its `PATH`.
fn claude_harness(path: &Path) -> ClaudeHarness {
    let harness = ClaudeHarness::new().with_binary(path);
    match path.parent().and_then(agent::process::search_path_with) {
        Some(search_path) => harness.with_search_path(search_path),
        None => harness,
    }
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
/// `model` blank lets the CLI pick its own. `work_root` is where the run's
/// fresh folder is made. `meetings_root` is where a saved wrap-up template
/// is looked for; `None` uses the built-in one. `cancel` stops the CLI.
pub(super) fn run_sample(
    harness: &dyn agent::Harness,
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
        let error = with_model(agent_error(error), model);
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
        .map_err(|error| UiError::app("agent-schema-mismatch", error.to_string()))?;
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

/// An agent run's error as the screen gets it: one `app` kind per thing the
/// user can do about it, and the error's own sentence.
///
/// A private function, not `impl From<AgentError> for UiError`: the notes
/// run in the meeting view may want its own mapping, and two impls would
/// clash.
pub(super) fn agent_error(error: AgentError) -> UiError {
    let kind = match &error {
        AgentError::NotInstalled { .. } => "agent-not-installed",
        AgentError::NotSignedIn { .. } => "agent-not-signed-in",
        AgentError::TimedOut { .. } => "agent-timed-out",
        AgentError::Cancelled => "agent-cancelled",
        AgentError::CliFailed { .. } => "agent-failed",
        AgentError::InvalidJson { .. } => "agent-invalid-json",
        AgentError::SchemaMismatch { .. } => "agent-schema-mismatch",
        AgentError::CouldNotStart { .. } => "agent-could-not-start",
    };
    UiError::app(kind, error.to_string())
}
