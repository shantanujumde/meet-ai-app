//! What a harness is asked to do: one job, one child process.

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// `agent.timeout_sec` when the config does not set it (SPEC §3.5).
pub const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// The two kinds of run SPEC A11 defines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    /// Transcript in, notes and tasks out. No tools at all.
    Notes,
    /// One task in, an issue key and URL out. Only the tracker's tools.
    Sync,
}

/// One agent run.
///
/// `Debug` leaves the prompt out on purpose: it carries the transcript, and a
/// job ends up in logs.
#[derive(Clone)]
pub struct Job {
    pub kind: JobKind,
    /// Passed to the CLI on stdin, never as an argument.
    pub prompt: String,
    /// JSON Schema the reply must pass before anything is written.
    pub schema: serde_json::Value,
    /// Model name the CLI accepts. `None` lets the CLI pick.
    pub model: Option<String>,
    /// Folder the run's working folder is made in. Each run gets a new, empty
    /// folder inside it, so no `CLAUDE.md`, `AGENTS.md` or project settings
    /// leak in. It is deleted when the run ends.
    pub work_root: PathBuf,
    /// The child is killed when this runs out (`agent.timeout_sec`).
    pub timeout: Duration,
    /// Tools the agent may use. Empty means none, which is what a notes run
    /// gets.
    pub allowed_tools: Vec<String>,
    /// Cancel from the UI. Clone it before handing the job over.
    pub cancel: CancelHandle,
}

impl Job {
    /// A notes run: no tools, default time limit, system temp folder.
    pub fn notes(prompt: impl Into<String>, schema: serde_json::Value) -> Self {
        Self {
            kind: JobKind::Notes,
            prompt: prompt.into(),
            schema,
            model: None,
            work_root: std::env::temp_dir(),
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            allowed_tools: Vec::new(),
            cancel: CancelHandle::new(),
        }
    }

    /// A sync run that may use only `allowed_tools`.
    pub fn sync(
        prompt: impl Into<String>,
        schema: serde_json::Value,
        allowed_tools: Vec<String>,
    ) -> Self {
        Self {
            kind: JobKind::Sync,
            allowed_tools,
            ..Self::notes(prompt, schema)
        }
    }
}

impl fmt::Debug for Job {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Job")
            .field("kind", &self.kind)
            .field("prompt_bytes", &self.prompt.len())
            .field("model", &self.model)
            .field("work_root", &self.work_root)
            .field("timeout", &self.timeout)
            .field("allowed_tools", &self.allowed_tools)
            .field("cancelled", &self.cancel.is_cancelled())
            .finish_non_exhaustive()
    }
}

/// Stops a running job from another thread (the UI's Cancel button).
///
/// Clones share one flag. Cancelling kills the child the same way the time
/// limit does, and the run returns [`crate::AgentError::Cancelled`].
#[derive(Debug, Clone, Default)]
pub struct CancelHandle(Arc<AtomicBool>);

impl CancelHandle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notes_job_gets_no_tools_and_the_default_time_limit() {
        let job = Job::notes("hi", serde_json::json!({}));
        assert_eq!(job.kind, JobKind::Notes);
        assert!(job.allowed_tools.is_empty());
        assert_eq!(job.timeout, Duration::from_secs(300));
    }

    #[test]
    fn a_sync_job_keeps_its_tools() {
        let job = Job::sync(
            "t",
            serde_json::json!({}),
            vec!["mcp__linear__create_issue".into()],
        );
        assert_eq!(job.kind, JobKind::Sync);
        assert_eq!(job.allowed_tools, ["mcp__linear__create_issue"]);
    }

    #[test]
    fn debug_does_not_print_the_prompt() {
        let job = Job::notes("SECRET TRANSCRIPT", serde_json::json!({}));
        let text = format!("{job:?}");
        assert!(!text.contains("SECRET"), "{text}");
        assert!(text.contains("prompt_bytes: 17"), "{text}");
    }

    #[test]
    fn cancel_is_shared_between_clones() {
        let job = Job::notes("x", serde_json::json!({}));
        let handle = job.cancel.clone();
        assert!(!job.cancel.is_cancelled());
        handle.cancel();
        assert!(job.cancel.is_cancelled());
    }
}
