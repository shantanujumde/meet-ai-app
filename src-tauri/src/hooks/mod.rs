//! User hooks (TUR-63, SPEC §5 Phase 6): the user's own command, run at three
//! moments of a meeting with the meeting folder.
//!
//! - [`run_hook`]: one hook, start to finish, with no Tauri in it. The shell
//!   is the platform's ([`platform::invocation`]); the folder goes in
//!   `MEETAI_MEETING_DIR` and as the last argument; output is capped and
//!   logged; past the timeout, and once it exits, the whole process tree is
//!   killed by [`agent::process::run_capped`], the agent runs' own runner
//!   (TUR-54, TUR-167).
//! - [`app`]: when each moment fires, and the "hook failed" note. A running
//!   hook holds the folder gate, so the meetings folder never moves under it.
//!
//! A hook never blocks or breaks a meeting: it runs on a thread of its own,
//! and its failure is only logged and shown.

use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::Duration;

pub mod app;
mod platform;

/// The env var holding the meeting folder.
pub const MEETING_DIR_ENV: &str = "MEETAI_MEETING_DIR";

/// Most bytes of stdout and of stderr kept for the log; the rest is read and
/// dropped, so a chatty hook never stalls on a full pipe.
pub const OUTPUT_CAP: usize = 64 * 1024;

/// The three moments a hook can run at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moment {
    TranscriptReady,
    AnalysisComplete,
    MeetingEnd,
}

impl Moment {
    /// The config key, which is also the name the meeting view shows.
    pub fn name(self) -> &'static str {
        match self {
            Self::TranscriptReady => "on_transcript_ready",
            Self::AnalysisComplete => "on_analysis_complete",
            Self::MeetingEnd => "on_meeting_end",
        }
    }

    /// This moment's command in `config`, if one is set.
    pub fn command(self, config: &crate::config::HooksConfig) -> Option<&str> {
        match self {
            Self::TranscriptReady => config.on_transcript_ready.as_deref(),
            Self::AnalysisComplete => config.on_analysis_complete.as_deref(),
            Self::MeetingEnd => config.on_meeting_end.as_deref(),
        }
    }
}

/// How one hook ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Exited with status 0.
    Ok,
    /// Exited with another status (`None`: killed by a signal).
    Failed { code: Option<i32> },
    /// Ran past the timeout; its whole tree was killed.
    TimedOut,
    /// Could not be started at all.
    NotStarted { error: String },
}

impl Outcome {
    pub fn is_ok(&self) -> bool {
        *self == Self::Ok
    }

    /// One short sentence for the log and the meeting view.
    pub fn describe(&self) -> String {
        match self {
            Self::Ok => "finished".to_owned(),
            Self::Failed { code: Some(code) } => format!("exited with status {code}"),
            Self::Failed { code: None } => "was stopped by a signal".to_owned(),
            Self::TimedOut => "ran too long and was stopped".to_owned(),
            Self::NotStarted { error } => format!("could not start: {error}"),
        }
    }
}

/// What a hook did, with its output (each capped at [`OUTPUT_CAP`]).
#[derive(Debug, Clone)]
pub struct Report {
    pub outcome: Outcome,
    pub stdout: String,
    pub stderr: String,
}

// Adapted from github.com/fastrepl/anarlog/crates/hooks/src/runner.rs @ 93deb8642e75a0a2f8ece1bed186da4362213edd (MIT)
/// `command` with a leading `~` and every whole `$HOME` made the home folder,
/// so `~/bin/x.sh` works under `cmd` and PowerShell, which expand neither.
/// Only for Windows: `sh` expands both itself, and knows quoting
/// ([`run_hook`]). A whole `$HOME` is one followed by the end, a slash, a
/// quote or a space, so `$HOMEBREW_PREFIX` stays. Unchanged when there is no
/// home.
pub fn expand_home_in_command(command: &str, home: Option<&Path>) -> String {
    let Some(home) = home.and_then(Path::to_str) else {
        return command.to_owned();
    };
    let command = replace_home_tokens(command, home);
    match command.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\', ' ']) => {
            format!("{home}{rest}")
        }
        _ => command,
    }
}

/// Every `$HOME` in `command` that is a whole token, made `home`.
fn replace_home_tokens(command: &str, home: &str) -> String {
    const TOKEN: &str = "$HOME";
    let mut out = String::with_capacity(command.len());
    let mut rest = command;
    while let Some(at) = rest.find(TOKEN) {
        let after = &rest[at + TOKEN.len()..];
        let whole = after.is_empty() || after.starts_with(['/', '\\', '"', '\'', ' ']);
        out.push_str(&rest[..at]);
        out.push_str(if whole { home } else { TOKEN });
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Run `command` for the meeting in `meeting_dir`, waiting at most `timeout`.
/// Never panics; every failure is in the [`Report`].
pub fn run_hook(command: &str, meeting_dir: &Path, timeout: Duration) -> Report {
    let windows = platform::is_windows();
    let command = if windows {
        expand_home_in_command(command, dirs::home_dir().as_deref())
    } else {
        command.to_owned()
    };
    let invocation = platform::invocation(windows, &command, meeting_dir);
    let mut cmd = platform::command(&invocation);
    cmd.env(MEETING_DIR_ENV, meeting_dir)
        .current_dir(meeting_dir);
    match agent::process::run_capped(cmd, timeout, OUTPUT_CAP) {
        Ok(run) => Report {
            outcome: run.status.map_or(Outcome::TimedOut, exited),
            stdout: run.stdout,
            stderr: run.stderr,
        },
        Err(error) => Report {
            outcome: Outcome::NotStarted {
                error: error.to_string(),
            },
            stdout: String::new(),
            stderr: String::new(),
        },
    }
}

fn exited(status: ExitStatus) -> Outcome {
    if status.success() {
        Outcome::Ok
    } else {
        Outcome::Failed {
            code: status.code(),
        }
    }
}

/// Run the hook for `moment`, if one is set, and log what it did. Returns the
/// report only when it did not succeed, for the "hook failed" note. Runs on
/// the caller's thread; [`app`] gives it one of its own.
pub fn run_moment(
    config: &crate::config::HooksConfig,
    moment: Moment,
    meeting_dir: &Path,
) -> Option<Report> {
    let command = moment.command(config)?;
    let report = run_hook(
        command,
        meeting_dir,
        Duration::from_secs(config.timeout_secs),
    );
    let hook = moment.name();
    let outcome = report.outcome.describe();
    if report.outcome.is_ok() {
        tracing::info!(hook, stdout = %report.stdout, stderr = %report.stderr, "hook {outcome}");
        None
    } else {
        tracing::warn!(hook, stdout = %report.stdout, stderr = %report.stderr, "hook {outcome}");
        Some(report)
    }
}

/// The meeting folder for `meeting_id` under `root`, if the id is a plain name.
pub fn meeting_dir(root: &Path, meeting_id: &str) -> Option<PathBuf> {
    store::folder::meeting_dir(root, meeting_id).ok()
}

#[cfg(test)]
mod tests;
