//! Every way an agent run can fail, worded for the person who has to fix it.

use std::time::Duration;

/// Why an agent run produced no notes (SPEC A11).
///
/// One variant per thing the user can act on: install the CLI, sign in,
/// raise the time limit, retry, or read what the CLI printed. The `Display`
/// text is shown in the meeting view next to the Retry button, so it never
/// contains the prompt or the transcript.
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    /// The CLI is not on this Mac, or not where the config says it is.
    #[error("{harness} is not installed, or meet-ai cannot find it")]
    NotInstalled { harness: String },

    /// The CLI is installed but has no signed-in account.
    #[error("{harness} is installed but not signed in; open it in Terminal and sign in")]
    NotSignedIn { harness: String },

    /// The run went past `agent.timeout_sec` and was stopped.
    #[error("the agent did not finish within {} seconds and was stopped", .after.as_secs())]
    TimedOut { after: Duration },

    /// The user pressed Cancel.
    #[error("the run was cancelled")]
    Cancelled,

    /// The CLI exited with an error. `stderr` is what it printed, trimmed to
    /// its last few KiB.
    #[error("the agent CLI failed ({}): {stderr}", exit_text(*.status))]
    CliFailed { status: Option<i32>, stderr: String },

    /// The CLI finished, but what it printed is not JSON.
    #[error("the agent's reply was not valid JSON: {reason}")]
    InvalidJson { reason: String },

    /// The reply is JSON, but not the shape the job asked for. Nothing is
    /// written to disk in this case.
    #[error("the agent's reply did not match the expected format: {}", .errors.join("; "))]
    SchemaMismatch { errors: Vec<String> },

    /// The run could not be started at all: the working folder could not be
    /// made, the CLI could not be launched, or the job's schema is broken.
    #[error("could not start the agent: {reason}")]
    CouldNotStart { reason: String },
}

fn exit_text(status: Option<i32>) -> String {
    match status {
        Some(code) => format!("exit code {code}"),
        None => "stopped by a signal".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_say_what_to_do() {
        let e = AgentError::NotSignedIn { harness: "Claude Code".into() };
        assert!(e.to_string().contains("sign in"));

        let e = AgentError::TimedOut { after: Duration::from_secs(300) };
        assert!(e.to_string().contains("300 seconds"));

        let e = AgentError::CliFailed { status: Some(2), stderr: "bad flag".into() };
        assert_eq!(e.to_string(), "the agent CLI failed (exit code 2): bad flag");

        let e = AgentError::CliFailed { status: None, stderr: String::new() };
        assert!(e.to_string().contains("signal"));
    }
}
