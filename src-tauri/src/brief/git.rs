//! `git log` for the brief's "since last time" list.
//!
//! The repo comes from `meeting.md`'s `repo:` or `repos.default`, so it can
//! sit on a network share that stalls. git runs as a process tree with a
//! time limit and a cap on what is kept of its output
//! (`agent::process::run_capped`, TUR-168): a stuck git is killed and the
//! brief shows without the part, rather than spinning and tying up a thread.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::Commit;

/// How long `git log` may take before the brief gives up on it.
const GIT_TIMEOUT: Duration = Duration::from_secs(5);

/// The most of `git log`'s output kept, per pipe. A line is a short hash and
/// a subject, and at most `BRIEF_MAX_COMMITS` are asked for.
const GIT_OUTPUT_CAP: usize = 64 * 1024;

/// Commits in `repo` since `since`, newest first, merges skipped, at most
/// `max`. `None` when `repo` is not a folder, is not a git repo, `git`
/// cannot be run, or it ran past [`GIT_TIMEOUT`]: the brief then leaves the
/// part out.
pub(super) fn recent_commits(repo: &Path, since: &str, max: usize) -> Option<Vec<Commit>> {
    recent_commits_with(OsStr::new("git"), repo, since, max, GIT_TIMEOUT)
}

/// [`recent_commits`], running `git` (a program name or path) with
/// `timeout`.
pub(super) fn recent_commits_with(
    git: &OsStr,
    repo: &Path,
    since: &str,
    max: usize,
    timeout: Duration,
) -> Option<Vec<Commit>> {
    if !repo.is_dir() {
        return None;
    }
    let mut command = Command::new(git);
    command
        .arg("-C")
        .arg(repo)
        .args(["-c", "log.showSignature=false", "log", "--no-merges"])
        .arg(format!("--since={since}"))
        .arg("--format=%h %s")
        .arg("-n")
        .arg(max.to_string())
        .env("GIT_TERMINAL_PROMPT", "0");
    let run = match agent::process::run_capped(command, timeout, GIT_OUTPUT_CAP) {
        Ok(run) => run,
        Err(error) => {
            tracing::debug!(%error, "brief: git could not be run");
            return None;
        }
    };
    let Some(status) = run.status else {
        tracing::warn!(
            timeout_secs = timeout.as_secs_f32(),
            "brief: git log took too long and was stopped; leaving the commits out"
        );
        return None;
    };
    if !status.success() {
        return None;
    }
    Some(
        run.stdout
            .lines()
            .filter_map(parse_line)
            .take(max)
            .collect(),
    )
}

/// One `%h %s` line.
fn parse_line(line: &str) -> Option<Commit> {
    let line = line.trim_end();
    if line.is_empty() {
        return None;
    }
    let (hash, subject) = line.split_once(' ').unwrap_or((line, ""));
    Some(Commit {
        hash: hash.to_owned(),
        subject: subject.to_owned(),
    })
}
