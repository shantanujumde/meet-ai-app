//! Running an agent CLI as a child process (SPEC A11): a fresh, empty working
//! folder, the prompt on stdin, a time limit, and the Cancel button.
//!
//! The prompt carries the transcript, so it only ever goes to the child's
//! stdin. It is never logged, never put in an error, and never passed as an
//! argument (arguments show up in `ps`).
//!
//! The child starts as a [`ProcessTree`]: in a process group of its own on
//! unix, in a Job Object of its own on Windows. A real CLI is a node process
//! (on Windows often behind a `.cmd` shim run by `cmd.exe`) that may start MCP
//! servers of its own, and stopping a run has to stop all of them, not just
//! the one we started.
//!
//! The prompt staying on stdin matters on Windows too: since Rust 1.77.2
//! (CVE-2024-24576) a `.bat`/`.cmd` argument that cannot be escaped safely
//! makes the spawn fail, and a transcript is exactly that kind of text.

use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

pub use crate::process_tree::ProcessTree;

use crate::{AgentError, Job};

/// How often the child is checked for exit, Cancel and the time limit.
const POLL_EVERY: Duration = Duration::from_millis(10);

/// How long to wait for the rest of the output after the child exits. A
/// grandchild that is still running can keep the pipes open past this.
const OUTPUT_GRACE: Duration = Duration::from_secs(2);

/// How long to wait for the output once those grandchildren are killed.
const AFTER_KILL_GRACE: Duration = Duration::from_millis(500);

/// Most of stdout kept. A notes reply is a few KiB; past this the reply is
/// rejected rather than held in memory.
pub(crate) const MAX_STDOUT_BYTES: usize = 8 * 1024 * 1024;

/// Most of stderr kept for [`AgentError::CliFailed`]. The end is kept, since
/// that is where a CLI says what went wrong.
const MAX_STDERR_BYTES: usize = 4 * 1024;

/// Files that make a folder a project an agent CLI would read instructions or
/// settings from. Claude Code and Codex look for these in the working folder
/// and every folder above it.
const PROJECT_MARKERS: &[&str] = &[".git", "CLAUDE.md", "CLAUDE.local.md", "AGENTS.md"];

/// What a CLI printed when it exited with status 0.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CliOutput {
    pub stdout: String,
    pub stderr: String,
}

/// Makes the run's new, empty working folder inside `job.work_root`.
///
/// Refuses a `work_root` that is inside a project: the CLI would find that
/// project's `CLAUDE.md`, `AGENTS.md` or git repo by looking upwards, which is
/// what the fresh folder is there to prevent. The system temp folder (the
/// default) is never inside one.
///
/// The folder is deleted when the returned [`tempfile::TempDir`] is dropped.
pub fn fresh_work_dir(job: &Job) -> Result<tempfile::TempDir, AgentError> {
    let root = job
        .work_root
        .canonicalize()
        .map_err(|e| could_not_start(format!("could not use the working folder: {e}")))?;
    if let Some(found) = project_marker_above(&root) {
        return Err(could_not_start(format!(
            "the working folder is inside a project ({} found); use a folder outside any project",
            found.display()
        )));
    }
    tempfile::Builder::new()
        .prefix("meet-ai-agent-")
        .tempdir_in(&root)
        .map_err(|e| could_not_start(format!("could not make a working folder: {e}")))
}

/// The first project marker in `folder` or any folder above it.
fn project_marker_above(folder: &Path) -> Option<std::path::PathBuf> {
    folder
        .ancestors()
        .flat_map(|dir| PROJECT_MARKERS.iter().map(move |name| dir.join(name)))
        .find(|path| path.exists())
}

/// How a CLI run ended, whatever its exit status, as [`run_cli_exit`] returns
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliExit {
    /// The exit code. `None` when a signal stopped the child.
    pub code: Option<i32>,
    /// What it printed on stdout. Empty when that was over the size cap.
    ///
    /// Only for telling failures apart (a CLI that prints its error as JSON
    /// on stdout). It may carry model text, so it never goes into an error,
    /// the UI or a log.
    pub stdout: String,
    /// What it printed on stderr, trimmed to its last few KiB.
    pub stderr: String,
    /// Whether stdout was over the size cap and dropped.
    pub stdout_overflowed: bool,
}

impl CliExit {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

/// Runs `command` in `work_dir` with `job.prompt` on stdin, and blocks until it
/// is done.
///
/// The child is killed, together with anything it started, when `job.cancel`
/// is set or `job.timeout` runs out. Cancel wins if both happen at once.
///
/// `display_name` is the CLI's name as the user knows it ("Claude Code"). It
/// goes into user-facing errors such as "Claude Code is not installed", and
/// into logs.
pub fn run_cli(
    display_name: &str,
    command: Command,
    job: &Job,
    work_dir: &Path,
) -> Result<CliOutput, AgentError> {
    let exit = run_cli_exit(display_name, command, job, work_dir)?;
    if !exit.success() {
        return Err(AgentError::CliFailed {
            status: exit.code,
            stderr: exit.stderr,
        });
    }
    if exit.stdout_overflowed {
        return Err(reply_too_big());
    }
    Ok(CliOutput {
        stdout: exit.stdout,
        stderr: exit.stderr,
    })
}

/// Runs a short health check (`--version`, a sign-in status) the way a real
/// run goes: a fresh, empty folder inside `work_root`, empty stdin, its own
/// process group, killed at `timeout`. The folder is deleted before this
/// returns.
pub fn run_probe(
    display_name: &str,
    command: Command,
    timeout: Duration,
    work_root: &Path,
) -> Result<CliOutput, AgentError> {
    // `run_cli` reads only the prompt, time limit, Cancel and work root of a
    // job. A probe's output never goes through a schema check, so the kind and
    // schema here are not used.
    let mut job = Job::notes(String::new(), serde_json::Value::Null);
    job.timeout = timeout;
    job.work_root = work_root.to_path_buf();
    let dir = fresh_work_dir(&job)?;
    run_cli(display_name, command, &job, dir.path())
}

/// A `Command` for the CLI at `path`, with `PATH` from [`search_path_with`].
pub fn cli_command(path: &Path) -> Command {
    let mut command = Command::new(path);
    if let Some(search_path) = path.parent().and_then(search_path_with) {
        command.env("PATH", search_path);
    }
    command
}

/// The app's `PATH` with `dir` put first. An npm install is a
/// `#!/usr/bin/env node` script with `node` next to it, but a Finder-launched
/// app's `PATH` does not have that folder. `None` when `dir` cannot go on a
/// `PATH` (it holds a `:`).
pub fn search_path_with(dir: &Path) -> Option<std::ffi::OsString> {
    let old = std::env::var_os("PATH").unwrap_or_default();
    let dirs = std::iter::once(dir.to_path_buf()).chain(std::env::split_paths(&old));
    std::env::join_paths(dirs).ok()
}

/// Like [`run_cli`], but a non-zero exit is not an error: the exit code and
/// what the CLI printed come back, so the caller can read a failure the CLI
/// reports on stdout. Not starting, the time limit and Cancel are still
/// errors.
pub fn run_cli_exit(
    display_name: &str,
    mut command: Command,
    job: &Job,
    work_dir: &Path,
) -> Result<CliExit, AgentError> {
    if job.cancel.is_cancelled() {
        return Err(AgentError::Cancelled);
    }
    // A missing folder would make the spawn fail with "not found", which would
    // wrongly read as "the CLI is not installed".
    if !work_dir.is_dir() {
        return Err(could_not_start("the working folder is missing"));
    }

    command
        .current_dir(work_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let started = Instant::now();
    let mut child = ProcessTree::spawn(command).map_err(|e| spawn_error(display_name, &e))?;
    let (stdout, stderr) = match start_pipes(&mut child, &job.prompt) {
        Ok(pipes) => pipes,
        Err(e) => {
            child.stop();
            return Err(could_not_start(format!(
                "could not start a helper thread: {e}"
            )));
        }
    };

    let log_stop = |e: &AgentError| {
        tracing::debug!(
            display_name,
            elapsed_ms = started.elapsed().as_millis(),
            "agent CLI stopped: {e}"
        );
    };
    let status = wait_for_exit(&mut child, job, started).inspect_err(log_stop)?;
    let (stdout, stderr) =
        collect_output(&mut child, job, started, &stdout, &stderr).inspect_err(log_stop)?;
    tracing::debug!(display_name, %status, elapsed_ms = started.elapsed().as_millis(), "agent CLI exited");

    let stderr = tail(
        String::from_utf8_lossy(&stderr.bytes).trim(),
        MAX_STDERR_BYTES,
    )
    .trim_start()
    .to_owned();
    let stdout_overflowed = stdout.overflowed;
    let stdout = if stdout_overflowed {
        String::new()
    } else {
        String::from_utf8_lossy(&stdout.bytes).into_owned()
    };
    Ok(CliExit {
        code: status.code(),
        stdout,
        stderr,
        stdout_overflowed,
    })
}

pub(crate) fn could_not_start(reason: impl Into<String>) -> AgentError {
    AgentError::CouldNotStart {
        reason: reason.into(),
    }
}

/// A reply over [`MAX_STDOUT_BYTES`], whether it came on stdout or in a file.
pub(crate) fn reply_too_big() -> AgentError {
    AgentError::InvalidJson {
        reason: format!(
            "the reply was larger than {} MiB",
            MAX_STDOUT_BYTES / (1024 * 1024)
        ),
    }
}

/// "Not found" means the CLI is not installed; anything else is reported as is.
fn spawn_error(display_name: &str, error: &io::Error) -> AgentError {
    if error.kind() == io::ErrorKind::NotFound {
        AgentError::NotInstalled {
            harness: display_name.to_owned(),
        }
    } else {
        could_not_start(format!("could not launch {display_name}: {error}"))
    }
}

/// Which part of a pipe's bytes a reader keeps.
#[derive(Debug, Clone, Copy)]
enum Keep {
    /// The first `n` bytes. Anything after is read and dropped, and marked.
    Head(usize),
    /// The last `n` bytes, at most twice that held at any time.
    Tail(usize),
}

/// What a reader kept from one pipe.
#[derive(Debug, Default)]
struct Drained {
    bytes: Vec<u8>,
    /// More arrived than [`Keep::Head`] allows.
    overflowed: bool,
}

impl Drained {
    fn push(&mut self, chunk: &[u8], keep: Keep) {
        match keep {
            Keep::Head(max) => {
                let room = max.saturating_sub(self.bytes.len());
                self.bytes
                    .extend_from_slice(&chunk[..chunk.len().min(room)]);
                self.overflowed |= chunk.len() > room;
            }
            Keep::Tail(max) => {
                self.bytes.extend_from_slice(chunk);
                if self.bytes.len() > 2 * max {
                    let excess = self.bytes.len() - max;
                    self.bytes.drain(..excess);
                }
            }
        }
    }
}

/// Starts the threads that feed stdin and drain stdout and stderr, and returns
/// where the drained bytes will arrive.
///
/// All three run off the calling thread: a prompt can be megabytes, and a
/// child that fills its stdout pipe stops reading stdin until someone drains
/// it.
fn start_pipes(
    child: &mut ProcessTree,
    prompt: &str,
) -> io::Result<(Receiver<Drained>, Receiver<Drained>)> {
    let stdin = child.take_stdin();
    let prompt = prompt.to_owned();
    thread::Builder::new()
        .name("agent-stdin".to_owned())
        .spawn(move || write_prompt(stdin, &prompt))?;
    let stdout = read_in_background(
        "agent-stdout",
        child.take_stdout(),
        Keep::Head(MAX_STDOUT_BYTES),
    )?;
    let stderr = read_in_background(
        "agent-stderr",
        child.take_stderr(),
        Keep::Tail(MAX_STDERR_BYTES),
    )?;
    Ok((stdout, stderr))
}

/// Writes the prompt, then closes stdin so the child sees the end of it.
fn write_prompt(stdin: Option<ChildStdin>, prompt: &str) {
    let Some(mut stdin) = stdin else { return };
    // A child may exit without reading all of its input; that is not an error.
    if let Err(e) = stdin.write_all(prompt.as_bytes())
        && e.kind() != io::ErrorKind::BrokenPipe
    {
        tracing::debug!("could not write the prompt to the agent CLI: {e}");
    }
    // `stdin` is dropped here, which closes the pipe.
}

/// Reads `pipe` to its end on a new thread, keeping what `keep` says, and
/// sends what it kept.
fn read_in_background<R: Read + Send + 'static>(
    name: &str,
    pipe: Option<R>,
    keep: Keep,
) -> io::Result<Receiver<Drained>> {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            let mut drained = Drained::default();
            if let Some(mut pipe) = pipe {
                let mut chunk = [0_u8; 64 * 1024];
                loop {
                    match pipe.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => drained.push(&chunk[..n], keep),
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                        // What was read so far is still in `drained`.
                        Err(_) => break,
                    }
                }
            }
            // The run may have stopped listening already; that is fine.
            let _ = tx.send(drained);
        })?;
    Ok(rx)
}

/// Why a run stopped early, if it did: Cancel first, then the time limit.
fn stop_reason(job: &Job, started: Instant) -> Option<AgentError> {
    if job.cancel.is_cancelled() {
        Some(AgentError::Cancelled)
    } else if started.elapsed() >= job.timeout {
        Some(AgentError::TimedOut { after: job.timeout })
    } else {
        None
    }
}

/// Waits for the child to exit, killing it on Cancel or when the time runs
/// out.
///
/// Cancel and the time limit are checked before `try_wait`, so the child is
/// always killed before it is reaped and its id cannot belong to anyone else.
fn wait_for_exit(
    child: &mut ProcessTree,
    job: &Job,
    started: Instant,
) -> Result<ExitStatus, AgentError> {
    loop {
        if let Some(reason) = stop_reason(job, started) {
            child.stop();
            return Err(reason);
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => thread::sleep(POLL_EVERY),
            Err(e) => {
                child.stop();
                return Err(could_not_start(format!("lost track of the agent CLI: {e}")));
            }
        }
    }
}

/// Collects the child's output after it exited.
///
/// Something the child started may still hold the pipes open. After
/// [`OUTPUT_GRACE`] its process tree is killed, and whatever arrives in the
/// next [`AFTER_KILL_GRACE`] is used. Cancel and the time limit still apply
/// while waiting. A reader that is still stuck is left behind, not joined.
fn collect_output(
    child: &mut ProcessTree,
    job: &Job,
    started: Instant,
    stdout: &Receiver<Drained>,
    stderr: &Receiver<Drained>,
) -> Result<(Drained, Drained), AgentError> {
    let mut out = None;
    let mut err = None;
    let mut deadline = Instant::now() + OUTPUT_GRACE;
    let mut killed = false;
    loop {
        if let Some(reason) = stop_reason(job, started) {
            child.kill_tree();
            return Err(reason);
        }
        receive_into(&mut out, stdout);
        receive_into(&mut err, stderr);
        if out.is_some() && err.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            if killed {
                break;
            }
            // The child is reaped, but a live process is holding the pipe. On
            // unix, if it is still in the child's group, the group's id
            // cannot have been reused; if it left the group (`setsid`), this
            // kill misses it and its output is cut off. On Windows the job
            // handle is still ours, and a process cannot leave its job.
            child.kill_tree();
            killed = true;
            deadline = Instant::now() + AFTER_KILL_GRACE;
        }
    }
    Ok((out.unwrap_or_default(), err.unwrap_or_default()))
}

/// Takes what `from` sent if `slot` is still empty, waiting at most one poll.
fn receive_into(slot: &mut Option<Drained>, from: &Receiver<Drained>) {
    if slot.is_some() {
        return;
    }
    match from.recv_timeout(POLL_EVERY / 2) {
        Ok(drained) => *slot = Some(drained),
        // A reader that died without sending has nothing more to give.
        Err(RecvTimeoutError::Disconnected) => *slot = Some(Drained::default()),
        Err(RecvTimeoutError::Timeout) => {}
    }
}

/// The last `max` bytes of `text` at most, cut where a character starts.
fn tail(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_keeps_the_end_and_cuts_on_a_character() {
        assert_eq!(tail("short", 10), "short");
        assert_eq!(tail("abcdef", 3), "def");
        // "é" is two bytes, so three bytes from the end falls inside one.
        assert_eq!(tail("ééé", 3), "é");
    }

    /// Runs `fake-cli` (test-support), the same on every OS.
    mod cli {
        use super::super::*;
        use std::path::PathBuf;

        const NAME: &str = "Test CLI";

        /// `fake-cli` with these settings (see its header).
        fn fake(settings: &[(&str, &str)]) -> Command {
            let mut command = Command::new(test_support::fake_cli_path());
            for (name, value) in settings {
                command.env(format!("FAKE_{}", name.to_uppercase()), value);
            }
            command
        }

        fn job_in(root: &Path, prompt: &str) -> Job {
            let mut job = Job::notes(prompt, serde_json::json!({}));
            job.work_root = root.to_path_buf();
            job.timeout = Duration::from_secs(10);
            job
        }

        fn run(settings: &[(&str, &str)], job: &Job) -> Result<CliOutput, AgentError> {
            let dir = fresh_work_dir(job).unwrap();
            run_cli(NAME, fake(settings), job, dir.path())
        }

        /// Whether `pid` is gone within two seconds (`test_support`).
        fn is_gone(pid: u32) -> bool {
            test_support::process_is_gone(pid, Duration::from_secs(2))
        }

        #[test]
        fn success_returns_stdout_and_stderr() {
            let root = tempfile::tempdir().unwrap();
            let settings = [("stdout", "out\n"), ("stderr", "err\n")];
            let out = run(&settings, &job_in(root.path(), "")).unwrap();
            assert_eq!(out.stdout, "out\n");
            assert_eq!(out.stderr, "err");
        }

        #[test]
        fn the_prompt_arrives_on_stdin() {
            let root = tempfile::tempdir().unwrap();
            let out = run(&[("echo_stdin", "1")], &job_in(root.path(), "hello agent")).unwrap();
            assert_eq!(out.stdout, "hello agent");
        }

        #[test]
        fn a_two_mib_prompt_goes_through_without_a_deadlock() {
            let root = tempfile::tempdir().unwrap();
            let prompt = "transcript line\n".repeat(2 * 1024 * 1024 / 16);
            assert_eq!(prompt.len(), 2 * 1024 * 1024);
            let out = run(&[("echo_stdin", "1")], &job_in(root.path(), &prompt)).unwrap();
            assert_eq!(out.stdout.len(), prompt.len());
            assert!(out.stdout == prompt);
        }

        #[test]
        fn the_child_runs_in_the_fresh_empty_folder() {
            let root = tempfile::tempdir().unwrap();
            let job = job_in(root.path(), "");
            let dir = fresh_work_dir(&job).unwrap();
            let out = run_cli(NAME, fake(&[("print_cwd", "1")]), &job, dir.path()).unwrap();
            let mut lines = out.stdout.lines();
            let cwd = PathBuf::from(lines.next().unwrap()).canonicalize().unwrap();
            assert_eq!(cwd, dir.path().canonicalize().unwrap());
            assert_eq!(lines.next().unwrap().trim(), "0");
        }

        #[test]
        fn each_run_gets_its_own_folder_and_it_is_deleted_afterwards() {
            let root = tempfile::tempdir().unwrap();
            let job = job_in(root.path(), "");
            let first = fresh_work_dir(&job).unwrap();
            let second = fresh_work_dir(&job).unwrap();
            assert_ne!(first.path(), second.path());
            assert!(
                first
                    .path()
                    .starts_with(root.path().canonicalize().unwrap())
            );
            let name = first
                .path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            assert!(name.starts_with("meet-ai-agent-"), "{name}");

            let path = first.path().to_path_buf();
            assert!(path.is_dir());
            drop(first);
            assert!(!path.exists());
        }

        #[test]
        fn a_non_zero_exit_is_a_cli_failure_with_its_code_and_stderr() {
            let root = tempfile::tempdir().unwrap();
            let settings = [("stderr", "bad flag\n"), ("code", "3")];
            let err = run(&settings, &job_in(root.path(), "")).unwrap_err();
            match err {
                AgentError::CliFailed { status, stderr } => {
                    assert_eq!(status, Some(3));
                    assert_eq!(stderr, "bad flag");
                }
                other => panic!("expected CliFailed, got {other:?}"),
            }
        }

        #[test]
        fn a_missing_program_is_not_installed() {
            let root = tempfile::tempdir().unwrap();
            let job = job_in(root.path(), "");
            let dir = fresh_work_dir(&job).unwrap();
            let missing = root.path().join("nonexistent").join("claude-xyz");
            let err = run_cli("Claude Code", Command::new(missing), &job, dir.path()).unwrap_err();
            match err {
                AgentError::NotInstalled { harness } => assert_eq!(harness, "Claude Code"),
                other => panic!("expected NotInstalled, got {other:?}"),
            }
        }

        #[test]
        fn the_time_limit_stops_the_child() {
            let root = tempfile::tempdir().unwrap();
            let mut job = job_in(root.path(), "");
            job.timeout = Duration::from_millis(300);
            let started = Instant::now();
            let err = run(&[("sleep", "30")], &job).unwrap_err();
            assert!(
                matches!(err, AgentError::TimedOut { after } if after == Duration::from_millis(300)),
                "{err:?}"
            );
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "{:?}",
                started.elapsed()
            );
        }

        #[test]
        fn the_time_limit_also_kills_what_the_child_started() {
            let root = tempfile::tempdir().unwrap();
            let pid_file = root.path().join("grandchild.pid");
            let mut job = job_in(root.path(), "");
            job.timeout = Duration::from_millis(500);
            let pid_path = pid_file.display().to_string();
            let settings = [("grandchild_pid_file", pid_path.as_str()), ("sleep", "30")];
            let started = Instant::now();
            let err = run(&settings, &job).unwrap_err();
            assert!(matches!(err, AgentError::TimedOut { .. }), "{err:?}");
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "{:?}",
                started.elapsed()
            );

            let pid = test_support::wait_for_pid_file(&pid_file);
            assert!(is_gone(pid), "grandchild {pid} is still running");
        }

        /// TUR-54's "done when": a child that started a grandchild, then
        /// Cancel; nothing is left running.
        #[test]
        fn cancel_also_kills_what_the_child_started() {
            let root = tempfile::tempdir().unwrap();
            let pid_file = root.path().join("grandchild.pid");
            let job = job_in(root.path(), "");
            let handle = job.cancel.clone();
            let watched = pid_file.clone();
            let canceller = thread::spawn(move || {
                // Cancel only once the grandchild is really there.
                test_support::wait_for_pid_file(&watched);
                handle.cancel();
            });
            let pid_path = pid_file.display().to_string();
            let settings = [("grandchild_pid_file", pid_path.as_str()), ("sleep", "30")];
            let err = run(&settings, &job).unwrap_err();
            canceller.join().unwrap();
            assert!(matches!(err, AgentError::Cancelled), "{err:?}");

            let pid = test_support::wait_for_pid_file(&pid_file);
            assert!(is_gone(pid), "grandchild {pid} is still running");
        }

        #[test]
        fn cancel_from_another_thread_stops_the_child() {
            let root = tempfile::tempdir().unwrap();
            let job = job_in(root.path(), "");
            let handle = job.cancel.clone();
            let canceller = thread::spawn(move || {
                thread::sleep(Duration::from_millis(200));
                handle.cancel();
            });
            let started = Instant::now();
            let err = run(&[("sleep", "30")], &job).unwrap_err();
            canceller.join().unwrap();
            assert!(matches!(err, AgentError::Cancelled), "{err:?}");
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "{:?}",
                started.elapsed()
            );
        }

        #[test]
        fn an_already_cancelled_job_never_starts_the_child() {
            let root = tempfile::tempdir().unwrap();
            let marker = root.path().join("started");
            let job = job_in(root.path(), "");
            job.cancel.cancel();
            let marker_path = marker.display().to_string();
            let err = run(&[("touch", marker_path.as_str())], &job).unwrap_err();
            assert!(matches!(err, AgentError::Cancelled), "{err:?}");
            assert!(!marker.exists());
        }

        #[test]
        fn a_grandchild_holding_stdout_open_does_not_hang_the_run() {
            let root = tempfile::tempdir().unwrap();
            let pid_file = root.path().join("grandchild.pid");
            let pid_path = pid_file.display().to_string();
            let settings = [
                ("stdout", "hi\n"),
                ("grandchild_pid_file", pid_path.as_str()),
                ("grandchild_keeps_stdout", "1"),
            ];
            let started = Instant::now();
            let out = run(&settings, &job_in(root.path(), "")).unwrap();
            assert_eq!(out.stdout, "hi\n");
            assert!(
                started.elapsed() < OUTPUT_GRACE + AFTER_KILL_GRACE + Duration::from_secs(1),
                "{:?}",
                started.elapsed()
            );
            let pid = test_support::wait_for_pid_file(&pid_file);
            assert!(is_gone(pid), "grandchild {pid} is still running");
        }

        #[test]
        fn stderr_is_cut_to_its_last_4_kib() {
            let root = tempfile::tempdir().unwrap();
            let settings = [
                ("stderr_pad", "10000"),
                ("stderr", " the end"),
                ("code", "1"),
            ];
            let err = run(&settings, &job_in(root.path(), "")).unwrap_err();
            match err {
                AgentError::CliFailed { stderr, .. } => {
                    assert_eq!(stderr.len(), MAX_STDERR_BYTES);
                    assert!(
                        stderr.ends_with("aaa the end"),
                        "{}",
                        &stderr[stderr.len() - 20..]
                    );
                    assert!(stderr.starts_with('a'));
                }
                other => panic!("expected CliFailed, got {other:?}"),
            }
        }

        #[test]
        fn cancel_still_works_while_waiting_for_output() {
            let root = tempfile::tempdir().unwrap();
            let pid_file = root.path().join("grandchild.pid");
            let job = job_in(root.path(), "");
            let handle = job.cancel.clone();
            let canceller = thread::spawn(move || {
                thread::sleep(Duration::from_millis(300));
                handle.cancel();
            });
            // The CLI exits at once; its grandchild keeps stdout open for 30 s.
            let pid_path = pid_file.display().to_string();
            let settings = [
                ("stdout", "hi\n"),
                ("grandchild_pid_file", pid_path.as_str()),
                ("grandchild_keeps_stdout", "1"),
            ];
            let started = Instant::now();
            let err = run(&settings, &job).unwrap_err();
            canceller.join().unwrap();
            assert!(matches!(err, AgentError::Cancelled), "{err:?}");
            assert!(started.elapsed() < OUTPUT_GRACE, "{:?}", started.elapsed());
            let pid = test_support::wait_for_pid_file(&pid_file);
            assert!(is_gone(pid), "grandchild {pid} is still running");
        }

        #[test]
        fn a_reply_over_the_stdout_limit_is_rejected() {
            let root = tempfile::tempdir().unwrap();
            let pad = (MAX_STDOUT_BYTES + 1).to_string();
            let err = run(&[("stdout_pad", pad.as_str())], &job_in(root.path(), "")).unwrap_err();
            match err {
                AgentError::InvalidJson { reason } => assert!(reason.contains("8 MiB"), "{reason}"),
                other => panic!("expected InvalidJson, got {other:?}"),
            }
        }

        #[test]
        fn a_tree_dropped_before_its_child_exits_is_killed() {
            let root = tempfile::tempdir().unwrap();
            let pid_file = root.path().join("grandchild.pid");
            let mut command = fake(&[
                ("grandchild_pid_file", &pid_file.display().to_string()),
                ("sleep", "30"),
            ]);
            command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let tree = ProcessTree::spawn(command).unwrap();
            let child = tree.id();
            let pid = test_support::wait_for_pid_file(&pid_file);
            drop(tree);
            assert!(is_gone(child), "child {child} is still running");
            assert!(is_gone(pid), "grandchild {pid} is still running");
        }

        #[test]
        fn a_work_root_inside_a_project_is_refused() {
            for marker in PROJECT_MARKERS {
                let project = tempfile::tempdir().unwrap();
                std::fs::write(project.path().join(marker), "").unwrap();
                let nested = project.path().join("a").join("b");
                std::fs::create_dir_all(&nested).unwrap();

                let err = fresh_work_dir(&job_in(&nested, "")).unwrap_err();
                match err {
                    AgentError::CouldNotStart { reason } => {
                        assert!(reason.contains(marker), "{marker}: {reason}")
                    }
                    other => panic!("{marker}: expected CouldNotStart, got {other:?}"),
                }
                assert_eq!(std::fs::read_dir(&nested).unwrap().count(), 0, "{marker}");
            }
        }

        #[test]
        fn the_default_work_root_is_not_inside_a_project() {
            let job = Job::notes("", serde_json::json!({}));
            let dir = fresh_work_dir(&job).unwrap();
            assert!(dir.path().is_dir());
        }
    }

    #[test]
    fn stdout_keeps_its_head_and_marks_the_rest() {
        let mut drained = Drained::default();
        drained.push(b"abc", Keep::Head(5));
        assert!(!drained.overflowed);
        drained.push(b"defg", Keep::Head(5));
        assert_eq!(drained.bytes, b"abcde");
        assert!(drained.overflowed);
    }

    #[test]
    fn stderr_keeps_its_tail_and_never_holds_much_more() {
        let mut drained = Drained::default();
        for i in 0..1000_u32 {
            drained.push(format!("{i:04}").as_bytes(), Keep::Tail(8));
            assert!(drained.bytes.len() <= 16, "{}", drained.bytes.len());
        }
        assert!(drained.bytes.ends_with(b"09980999"));
        assert!(!drained.overflowed);
    }
}
