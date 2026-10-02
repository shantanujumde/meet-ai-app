//! Running an agent CLI as a child process (SPEC A11): a fresh, empty working
//! folder, the prompt on stdin, a time limit, and the Cancel button.
//!
//! The prompt carries the transcript, so it only ever goes to the child's
//! stdin. It is never logged, never put in an error, and never passed as an
//! argument (arguments show up in `ps`).
//!
//! On unix the child gets its own process group. A real CLI is a node process
//! that may start MCP servers of its own, and stopping a run has to stop all of
//! them, not just the one we started.

use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use crate::{AgentError, Job};

/// How often the child is checked for exit, Cancel and the time limit.
const POLL_EVERY: Duration = Duration::from_millis(10);

/// How long to wait for the rest of the output after the child exits. A
/// grandchild that is still running can keep the pipes open past this.
const OUTPUT_GRACE: Duration = Duration::from_secs(2);

/// How long to wait for the output once those grandchildren are killed.
const AFTER_KILL_GRACE: Duration = Duration::from_millis(500);

/// Most of stderr kept for [`AgentError::CliFailed`]. The end is kept, since
/// that is where a CLI says what went wrong.
const MAX_STDERR_BYTES: usize = 4 * 1024;

/// Where a reader thread sends everything it read from one pipe.
type Drained = Receiver<Vec<u8>>;

/// What a CLI printed when it exited with status 0.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CliOutput {
    pub stdout: String,
    pub stderr: String,
}

/// Makes the run's new, empty working folder inside `job.work_root`.
///
/// The folder is deleted when the returned [`tempfile::TempDir`] is dropped.
pub fn fresh_work_dir(job: &Job) -> Result<tempfile::TempDir, AgentError> {
    tempfile::Builder::new()
        .prefix("meet-ai-agent-")
        .tempdir_in(&job.work_root)
        .map_err(|e| AgentError::CouldNotStart {
            reason: format!("could not make a working folder: {e}"),
        })
}

/// Runs `command` in `work_dir` with `job.prompt` on stdin, and blocks until it
/// is done.
///
/// The child is killed, together with anything it started, when `job.cancel`
/// is set or `job.timeout` runs out. Cancel wins if both happen at once.
/// `harness` is the name used in [`AgentError::NotInstalled`] and in logs.
pub fn run_cli(
    harness: &str,
    mut command: Command,
    job: &Job,
    work_dir: &Path,
) -> Result<CliOutput, AgentError> {
    if job.cancel.is_cancelled() {
        return Err(AgentError::Cancelled);
    }
    // A missing folder would make the spawn fail with "not found", which would
    // wrongly read as "the CLI is not installed".
    if !work_dir.is_dir() {
        return Err(AgentError::CouldNotStart {
            reason: "the working folder is missing".to_owned(),
        });
    }

    command
        .current_dir(work_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    own_process_group(&mut command);

    let started = Instant::now();
    let mut child = command.spawn().map_err(|e| spawn_error(harness, &e))?;
    let (stdout, stderr) = match start_pipes(&mut child, &job.prompt) {
        Ok(pipes) => pipes,
        Err(e) => {
            stop(&mut child);
            return Err(AgentError::CouldNotStart {
                reason: format!("could not start a helper thread: {e}"),
            });
        }
    };

    let status = wait_for_exit(&mut child, job, started).inspect_err(|e| {
        tracing::debug!(
            harness,
            elapsed_ms = started.elapsed().as_millis(),
            "agent CLI stopped: {e}"
        );
    })?;
    let (stdout, stderr) = collect_output(child.id(), &stdout, &stderr);
    tracing::debug!(harness, %status, elapsed_ms = started.elapsed().as_millis(), "agent CLI exited");

    let stdout = String::from_utf8_lossy(&stdout).into_owned();
    let stderr = tail(String::from_utf8_lossy(&stderr).trim(), MAX_STDERR_BYTES)
        .trim_start()
        .to_owned();
    if status.success() {
        Ok(CliOutput { stdout, stderr })
    } else {
        Err(AgentError::CliFailed {
            status: status.code(),
            stderr,
        })
    }
}

/// Puts the child in a new process group of its own, so one kill reaches
/// everything it starts.
#[cfg(unix)]
fn own_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn own_process_group(_command: &mut Command) {}

/// "Not found" means the CLI is not installed; anything else is reported as is.
fn spawn_error(harness: &str, error: &io::Error) -> AgentError {
    if error.kind() == io::ErrorKind::NotFound {
        AgentError::NotInstalled {
            harness: harness.to_owned(),
        }
    } else {
        AgentError::CouldNotStart {
            reason: format!("could not launch {harness}: {error}"),
        }
    }
}

/// Starts the threads that feed stdin and drain stdout and stderr, and returns
/// where the drained bytes will arrive.
///
/// All three run off the calling thread: a prompt can be megabytes, and a
/// child that fills its stdout pipe stops reading stdin until someone drains
/// it.
fn start_pipes(child: &mut Child, prompt: &str) -> io::Result<(Drained, Drained)> {
    let stdin = child.stdin.take();
    let prompt = prompt.to_owned();
    thread::Builder::new()
        .name("agent-stdin".to_owned())
        .spawn(move || write_prompt(stdin, &prompt))?;
    let stdout = read_in_background("agent-stdout", child.stdout.take())?;
    let stderr = read_in_background("agent-stderr", child.stderr.take())?;
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

/// Reads `pipe` to its end on a new thread and sends everything it read.
fn read_in_background<R: Read + Send + 'static>(
    name: &str,
    pipe: Option<R>,
) -> io::Result<Drained> {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                // On a read error, what was read so far is still in `bytes`.
                let _ = pipe.read_to_end(&mut bytes);
            }
            // The run may have stopped listening already; that is fine.
            let _ = tx.send(bytes);
        })?;
    Ok(rx)
}

/// Waits for the child to exit, killing it on Cancel or when the time runs
/// out.
///
/// Cancel and the time limit are checked before `try_wait`, so the child is
/// always killed before it is reaped and its id cannot belong to anyone else.
fn wait_for_exit(child: &mut Child, job: &Job, started: Instant) -> Result<ExitStatus, AgentError> {
    loop {
        if job.cancel.is_cancelled() {
            stop(child);
            return Err(AgentError::Cancelled);
        }
        if started.elapsed() >= job.timeout {
            stop(child);
            return Err(AgentError::TimedOut { after: job.timeout });
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => thread::sleep(POLL_EVERY),
            Err(e) => {
                stop(child);
                return Err(AgentError::CouldNotStart {
                    reason: format!("lost track of the agent CLI: {e}"),
                });
            }
        }
    }
}

/// Collects the child's output after it exited.
///
/// Something the child started may still hold the pipes open. After
/// [`OUTPUT_GRACE`] its process group is killed, and whatever arrives in the
/// next [`AFTER_KILL_GRACE`] is used. This never waits longer than that, and a
/// reader that is still stuck is left behind rather than joined.
fn collect_output(pid: u32, stdout: &Drained, stderr: &Drained) -> (Vec<u8>, Vec<u8>) {
    let deadline = Instant::now() + OUTPUT_GRACE;
    let mut out = receive_by(stdout, deadline);
    let mut err = receive_by(stderr, deadline);
    if out.is_none() || err.is_none() {
        // Safe even though the child is reaped: the group still has a live
        // member holding the pipe, so its id cannot have been reused.
        kill_group(pid);
        let deadline = Instant::now() + AFTER_KILL_GRACE;
        if out.is_none() {
            out = receive_by(stdout, deadline);
        }
        if err.is_none() {
            err = receive_by(stderr, deadline);
        }
    }
    (out.unwrap_or_default(), err.unwrap_or_default())
}

/// What `from` sends before `deadline`, if anything.
fn receive_by(from: &Drained, deadline: Instant) -> Option<Vec<u8>> {
    from.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .ok()
}

/// Kills the child and everything it started, then reaps it.
fn stop(child: &mut Child) {
    kill_group(child.id());
    // In case the group kill did not work (no `kill` on the PATH, say).
    let _ = child.kill();
    let _ = child.wait();
}

/// Kills every process in the group the child leads. Runs the system `kill`,
/// so no `unsafe` and no extra crate is needed.
#[cfg(unix)]
fn kill_group(pid: u32) {
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// There are no process groups to kill outside unix; [`stop`] kills the child
/// itself.
#[cfg(not(unix))]
fn kill_group(_pid: u32) {}

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

    #[cfg(unix)]
    mod unix {
        use super::super::*;
        use std::path::PathBuf;

        const HARNESS: &str = "test-cli";

        fn sh(script: &str) -> Command {
            let mut command = Command::new("sh");
            command.arg("-c").arg(script);
            command
        }

        fn job_in(root: &Path, prompt: &str) -> Job {
            let mut job = Job::notes(prompt, serde_json::json!({}));
            job.work_root = root.to_path_buf();
            job.timeout = Duration::from_secs(10);
            job
        }

        fn run(script: &str, job: &Job) -> Result<CliOutput, AgentError> {
            let dir = fresh_work_dir(job).unwrap();
            run_cli(HARNESS, sh(script), job, dir.path())
        }

        /// Waits up to two seconds for the process with `pid` to be gone.
        fn is_gone(pid: &str) -> bool {
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                let alive = Command::new("kill")
                    .args(["-0", pid])
                    .stderr(Stdio::null())
                    .status()
                    .unwrap()
                    .success();
                if !alive {
                    return true;
                }
                thread::sleep(Duration::from_millis(20));
            }
            false
        }

        #[test]
        fn success_returns_stdout_and_stderr() {
            let root = tempfile::tempdir().unwrap();
            let out = run("echo out; echo err >&2", &job_in(root.path(), "")).unwrap();
            assert_eq!(out.stdout, "out\n");
            assert_eq!(out.stderr, "err");
        }

        #[test]
        fn the_prompt_arrives_on_stdin() {
            let root = tempfile::tempdir().unwrap();
            let out = run("cat", &job_in(root.path(), "hello agent")).unwrap();
            assert_eq!(out.stdout, "hello agent");
        }

        #[test]
        fn a_two_mib_prompt_goes_through_without_a_deadlock() {
            let root = tempfile::tempdir().unwrap();
            let prompt = "transcript line\n".repeat(2 * 1024 * 1024 / 16);
            assert_eq!(prompt.len(), 2 * 1024 * 1024);
            let out = run("cat", &job_in(root.path(), &prompt)).unwrap();
            assert_eq!(out.stdout.len(), prompt.len());
            assert!(out.stdout == prompt);
        }

        #[test]
        fn the_child_runs_in_the_fresh_empty_folder() {
            let root = tempfile::tempdir().unwrap();
            let job = job_in(root.path(), "");
            let dir = fresh_work_dir(&job).unwrap();
            let out = run_cli(HARNESS, sh("pwd -P; ls -A | wc -l"), &job, dir.path()).unwrap();
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
            assert!(first.path().starts_with(root.path()));
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
            let err = run("echo 'bad flag' >&2; exit 3", &job_in(root.path(), "")).unwrap_err();
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
            let err = run_cli(
                "Claude Code",
                Command::new("/nonexistent/claude-xyz"),
                &job,
                dir.path(),
            )
            .unwrap_err();
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
            let err = run("sleep 30", &job).unwrap_err();
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
            job.timeout = Duration::from_millis(300);
            let script = format!("sleep 30 & echo $! > '{}'; wait", pid_file.display());
            let started = Instant::now();
            let err = run(&script, &job).unwrap_err();
            assert!(matches!(err, AgentError::TimedOut { .. }), "{err:?}");
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "{:?}",
                started.elapsed()
            );

            let pid = std::fs::read_to_string(&pid_file).unwrap();
            assert!(is_gone(pid.trim()), "grandchild {pid} is still running");
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
            let err = run("sleep 30", &job).unwrap_err();
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
            let err = run(&format!("touch '{}'", marker.display()), &job).unwrap_err();
            assert!(matches!(err, AgentError::Cancelled), "{err:?}");
            assert!(!marker.exists());
        }

        #[test]
        fn a_grandchild_holding_stdout_open_does_not_hang_the_run() {
            let root = tempfile::tempdir().unwrap();
            let started = Instant::now();
            let out = run("echo hi; sleep 30 &", &job_in(root.path(), "")).unwrap();
            assert_eq!(out.stdout, "hi\n");
            assert!(
                started.elapsed() < Duration::from_secs(4),
                "{:?}",
                started.elapsed()
            );
        }

        #[test]
        fn stderr_is_cut_to_its_last_4_kib() {
            let root = tempfile::tempdir().unwrap();
            let script = "head -c 10000 /dev/zero | tr '\\0' a >&2; echo ' the end' >&2; exit 1";
            let err = run(script, &job_in(root.path(), "")).unwrap_err();
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
    }
}
