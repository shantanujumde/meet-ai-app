//! Runs the user's own agent CLI for meet-ai (SPEC A11, §4).
//!
//! This crate is the one place in the codebase allowed to hand a transcript to
//! an agent, and it does so only as a local child process: the CLI the user
//! installed and signed in to (`claude` or `codex`) gets the prompt on stdin,
//! does the model call under the user's own login, and prints JSON. There is
//! no network client and no SDK here, and there never will be (L9, L11).
//!
//! Every run:
//! - starts in a fresh, empty temp folder, so no project `CLAUDE.md`,
//!   `AGENTS.md` or settings leak in ([`process::fresh_work_dir`]);
//! - is killed when its time limit runs out or the user presses Cancel
//!   ([`process::run_cli`]);
//! - has its reply checked against the job's schema before anything is
//!   written ([`OutputCheck`]).
//!
//! A transcript is untrusted text. The app never runs anything a reply asks
//! for; it only reads the fields the schema allows.

#![forbid(unsafe_code)]

pub mod claude;
mod error;
mod job;
mod output;
pub mod process;

#[cfg(feature = "test-support")]
pub mod fake;

use std::path::PathBuf;

pub use claude::ClaudeHarness;
pub use error::AgentError;
pub use job::{CancelHandle, DEFAULT_TIMEOUT_SECS, Job, JobKind};
pub use output::{OutputCheck, parse_json};

/// An installed agent CLI, as found by [`Harness::detect`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Install {
    /// Absolute path to the binary.
    pub path: PathBuf,
    /// What `--version` printed, if it printed anything usable.
    pub version: Option<String>,
    /// Whether the CLI has an account it can run under.
    pub signed_in: bool,
}

/// One agent CLI meet-ai knows how to drive: Claude Code, Codex, or the fake
/// one tests use. Cursor, OpenCode and Gemini would come in here later.
///
/// Object-safe, so the app can hold the user's pick as `Box<dyn Harness>`.
/// `run` blocks until the child exits; call it off the UI thread.
pub trait Harness: Send + Sync {
    /// Stable id, as written to `agent.harness` and `analyzed_by`
    /// (`"claude-code"`, `"codex"`).
    fn id(&self) -> &'static str;

    /// Finds the CLI on this Mac. `None` when it is not installed.
    fn detect(&self) -> Option<Install>;

    /// Model names this CLI accepts, for the setup picker.
    fn models(&self) -> Vec<String>;

    /// Runs `job` and returns the reply, already checked against
    /// `job.schema`.
    fn run(&self, job: &Job) -> Result<serde_json::Value, AgentError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_is_object_safe() {
        fn takes(_: Option<&dyn Harness>) {}
        takes(None);
        let _boxed: Option<Box<dyn Harness>> = None;
    }
}
