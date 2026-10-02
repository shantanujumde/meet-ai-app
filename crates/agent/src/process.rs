//! Running an agent CLI as a child process: fresh folder, prompt on stdin,
//! time limit, cancel. STUB — being written; signatures are final.

use std::path::Path;
use std::process::Command;

use crate::{AgentError, Job};

/// What a CLI printed when it exited with status 0.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CliOutput {
    pub stdout: String,
    pub stderr: String,
}

/// Makes the run's new, empty working folder inside `job.work_root`.
pub fn fresh_work_dir(job: &Job) -> Result<tempfile::TempDir, AgentError> {
    tempfile::Builder::new()
        .prefix("meet-ai-agent-")
        .tempdir_in(&job.work_root)
        .map_err(|e| AgentError::CouldNotStart { reason: format!("could not make a working folder: {e}") })
}

/// Runs `command` in `work_dir` with `job.prompt` on stdin.
pub fn run_cli(harness: &str, command: Command, job: &Job, work_dir: &Path) -> Result<CliOutput, AgentError> {
    let _ = (harness, command, job, work_dir);
    Err(AgentError::CouldNotStart { reason: "not written yet".to_owned() })
}
