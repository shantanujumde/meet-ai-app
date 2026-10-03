//! `git log` for the brief's "since last time" list.

use std::path::Path;
use std::process::{Command, Stdio};

use super::Commit;

/// Commits in `repo` since `since`, newest first, merges skipped, at most
/// `max`. `None` when `repo` is not a folder, is not a git repo, or `git`
/// cannot be run: the brief then leaves the part out.
pub(super) fn recent_commits(repo: &Path, since: &str, max: usize) -> Option<Vec<Commit>> {
    if !repo.is_dir() {
        return None;
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "log.showSignature=false", "log", "--no-merges"])
        .arg(format!("--since={since}"))
        .arg("--format=%h %s")
        .arg("-n")
        .arg(max.to_string())
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Some(text.lines().filter_map(parse_line).take(max).collect())
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
