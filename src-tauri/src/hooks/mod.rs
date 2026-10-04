//! User hooks (TUR-63, SPEC §5 Phase 6): the user's own command, run at three
//! moments of a meeting with the meeting folder.
//!
//! - [`run_hook`]: one hook, start to finish, with no Tauri in it. The shell
//!   is the platform's ([`platform::invocation`]); the folder goes in
//!   `MEETAI_MEETING_DIR` and as the last argument; output is capped and
//!   logged; past the timeout the whole process tree is killed, with the
//!   same [`agent::ProcessTree`] the agent runs use (TUR-54).
//! - [`app`]: when each moment fires, and the "hook failed" note.
//!
//! A hook never blocks or breaks a meeting: it runs on a thread of its own,
//! and its failure is only logged and shown.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub mod app;
mod platform;

/// The env var holding the meeting folder.
pub const MEETING_DIR_ENV: &str = "MEETAI_MEETING_DIR";

/// Most bytes of stdout and of stderr kept for the log; the rest is read and
/// dropped, so a chatty hook never stalls on a full pipe.
pub const OUTPUT_CAP: usize = 64 * 1024;

/// How often a running hook is checked on.
const POLL: Duration = Duration::from_millis(25);

/// How long, after the hook exits, its output may take to arrive. A child it
/// left running can hold the pipes open; the log then has what came so far.
const DRAIN_WAIT: Duration = Duration::from_secs(1);

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
/// `command` with a leading `~` and every `$HOME` made the home folder, so
/// `~/bin/x.sh` works under `cmd` too. Unchanged when there is no home.
pub fn expand_home(command: &str, home: Option<&Path>) -> String {
    let Some(home) = home.and_then(Path::to_str) else {
        return command.to_owned();
    };
    let command = command.replace("$HOME", home);
    match command.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\', ' ']) => {
            format!("{home}{rest}")
        }
        _ => command,
    }
}

/// Run `command` for the meeting in `meeting_dir`, waiting at most `timeout`.
/// Never panics; every failure is in the [`Report`].
pub fn run_hook(command: &str, meeting_dir: &Path, timeout: Duration) -> Report {
    let command = expand_home(command, dirs::home_dir().as_deref());
    let invocation = platform::invocation(platform::is_windows(), &command, meeting_dir);
    let mut cmd = platform::command(&invocation);
    cmd.env(MEETING_DIR_ENV, meeting_dir)
        .current_dir(meeting_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut tree = match agent::ProcessTree::spawn(cmd) {
        Ok(tree) => tree,
        Err(error) => {
            return Report {
                outcome: Outcome::NotStarted {
                    error: error.to_string(),
                },
                stdout: String::new(),
                stderr: String::new(),
            };
        }
    };
    let stdout = tree.take_stdout().map(read_capped);
    let stderr = tree.take_stderr().map(read_capped);
    let outcome = wait(&mut tree, timeout);
    Report {
        outcome,
        stdout: collect(stdout),
        stderr: collect(stderr),
    }
}

/// Wait for the tree's child, killing the whole tree past `timeout`.
fn wait(tree: &mut agent::ProcessTree, timeout: Duration) -> Outcome {
    let deadline = Instant::now() + timeout;
    loop {
        match tree.try_wait() {
            Ok(Some(status)) => return exited(status),
            Ok(None) if Instant::now() >= deadline => {
                tree.stop();
                return Outcome::TimedOut;
            }
            Ok(None) => std::thread::sleep(POLL),
            Err(error) => {
                tree.stop();
                return Outcome::NotStarted {
                    error: error.to_string(),
                };
            }
        }
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

/// Read `pipe` on a thread of its own, keeping the first [`OUTPUT_CAP`] bytes.
fn read_capped(pipe: impl Read + Send + 'static) -> mpsc::Receiver<String> {
    let (send, receive) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("meet-ai-hook-output".to_owned())
        .spawn(move || {
            let _ = send.send(capped(pipe, OUTPUT_CAP));
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not read a hook's output");
    }
    receive
}

/// The first `cap` bytes of `pipe` as text, reading the rest to the end.
fn capped(mut pipe: impl Read, cap: usize) -> String {
    let mut kept = Vec::new();
    let mut buf = [0_u8; 8192];
    loop {
        match pipe.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let room = cap.saturating_sub(kept.len());
                kept.extend_from_slice(&buf[..n.min(room)]);
            }
        }
    }
    String::from_utf8_lossy(&kept).into_owned()
}

fn collect(output: Option<mpsc::Receiver<String>>) -> String {
    output
        .and_then(|receive| receive.recv_timeout(DRAIN_WAIT).ok())
        .unwrap_or_default()
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
