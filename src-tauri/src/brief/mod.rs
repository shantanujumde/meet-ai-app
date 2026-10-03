//! The pre-meeting brief (TUR-32, `docs/problem.md` item 44, U5).
//!
//! Before a call: what was said last time, and what changed in the repo since.
//! A plain read-only view, no agent run.
//!
//! * **Last time** is the most recent meeting before today with the same title
//!   (trimmed, any case). It is found through the search index
//!   ([`store::index::Index::meetings_titled`]), not by walking every folder;
//!   only that one folder is then read for its summary, decisions and tickets.
//!   Matching on a calendar series id is left for later: `Event` has none yet.
//! * **Recent commits** come from that meeting's `repo`, else `repos.default`
//!   (read by the existing [`config::default_repo`], not a second reader):
//!   `git log` since that meeting's date, merges skipped, at most
//!   [`BRIEF_MAX_COMMITS`]. No earlier meeting, no repo, a missing folder or
//!   one that is not a git repo all leave the commits part out quietly.

mod git;

use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use serde::Serialize;
use store::index::IndexedMeeting;
use store::ticket::Status;
use tauri::AppHandle;

use crate::config;
use crate::error::UiError;
use crate::{meetings, search};

/// The most commits the brief lists.
pub const BRIEF_MAX_COMMITS: usize = 20;

/// Everything the brief view shows for one meeting title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MeetingBrief {
    /// The title asked for, as given.
    pub title: String,
    /// The last meeting with this title before today; `None` when there is
    /// none.
    pub previous: Option<PreviousMeeting>,
    /// Commits since `previous`; `None` when there is no previous meeting, no
    /// repo, or the repo cannot be read.
    pub commits: Option<RepoCommits>,
}

/// What was said last time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviousMeeting {
    /// The folder name, e.g. `2026-09-01-1430-standup`.
    pub id: String,
    pub title: String,
    /// The `date` key as written, `None` when the file has none.
    pub date: Option<String>,
    /// The `## Summary` text, `None` when blank or missing.
    pub summary: Option<String>,
    /// The `## Decisions` text, `None` when blank or missing.
    pub decisions: Option<String>,
    /// Its tickets that are not done or dropped.
    pub open_tickets: Vec<BriefTicket>,
}

/// One ticket still open from last time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BriefTicket {
    pub id: String,
    pub title: String,
    /// `open` or `in_progress`; `None` when the file has no valid status.
    pub status: Option<String>,
}

/// The repo the meeting is about and what landed in it since last time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RepoCommits {
    /// The repo path as configured, e.g. `~/apps/api`.
    pub repo: String,
    /// What `git log --since` was given.
    pub since: String,
    /// Newest first, at most [`BRIEF_MAX_COMMITS`].
    pub commits: Vec<Commit>,
}

/// One `git log` line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    /// The short hash.
    pub hash: String,
    pub subject: String,
}

/// The brief for the meeting called `title`.
///
/// Runs on the blocking pool: it reads the index, a meeting folder and runs
/// `git`.
#[tauri::command]
#[specta::specta]
pub async fn meeting_brief(app: AppHandle, title: String) -> Result<MeetingBrief, UiError> {
    tauri::async_runtime::spawn_blocking(move || {
        let matches = search::state(&app).meetings_titled(&title)?;
        let today = chrono::Local::now().date_naive();
        brief_from(
            &meetings::root()?,
            &title,
            &matches,
            today,
            config::default_repo()?,
        )
    })
    .await
    .map_err(|error| UiError::app("task-failed", error.to_string()))?
}

/// The brief for `title`, given the index's `matches` for it (newest first),
/// the local date, and `repos.default`.
fn brief_from(
    root: &Path,
    title: &str,
    matches: &[IndexedMeeting],
    today: NaiveDate,
    default_repo: Option<String>,
) -> Result<MeetingBrief, UiError> {
    let Some(found) = matches
        .iter()
        .find(|meeting| day_of(meeting).is_some_and(|day| day < today))
    else {
        return Ok(MeetingBrief {
            title: title.to_owned(),
            previous: None,
            commits: None,
        });
    };

    let folder = store::folder::load(&store::folder::meeting_dir(root, &found.id)?)?;
    let meeting = folder.meeting.as_ref();
    let section = |heading: &str| {
        meeting
            .and_then(|m| m.section(heading))
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    let open_tickets = folder
        .tickets
        .iter()
        .filter(|ticket| !matches!(ticket.status(), Some(Status::Done | Status::Dropped)))
        .map(|ticket| BriefTicket {
            id: ticket.id().unwrap_or_default(),
            title: ticket.title().unwrap_or_default(),
            status: ticket.status().map(|status| status.as_str().to_owned()),
        })
        .collect();
    let previous = PreviousMeeting {
        id: found.id.clone(),
        title: found.title.clone(),
        date: found.date.clone(),
        summary: section("Summary"),
        decisions: section("Decisions"),
        open_tickets,
    };

    let since = found
        .date
        .clone()
        .filter(|date| !date.trim().is_empty())
        .or_else(|| day_of(found).map(|day| day.to_string()));
    let repo = meeting
        .and_then(|m| m.frontmatter.get_str("repo"))
        .or(default_repo)
        .filter(|repo| !repo.trim().is_empty());
    let commits = match (repo, since) {
        (Some(repo), Some(since)) => {
            git::recent_commits(&expand_home(&repo), &since, BRIEF_MAX_COMMITS).map(|commits| {
                RepoCommits {
                    repo,
                    since,
                    commits,
                }
            })
        }
        _ => None,
    };

    Ok(MeetingBrief {
        title: title.to_owned(),
        previous: Some(previous),
        commits,
    })
}

/// The meeting's day: from its `date` key, else its folder name
/// (`YYYY-MM-DD-HHMM-…`).
fn day_of(meeting: &IndexedMeeting) -> Option<NaiveDate> {
    let leading_day = |text: &str| {
        text.get(..10)
            .and_then(|day| NaiveDate::parse_from_str(day, "%Y-%m-%d").ok())
    };
    meeting
        .date
        .as_deref()
        .and_then(leading_day)
        .or_else(|| leading_day(&meeting.id))
}

/// `~` or `~/…` under the home folder; anything else as written.
fn expand_home(repo: &str) -> PathBuf {
    let repo = repo.trim();
    let rest = match repo.strip_prefix('~') {
        Some("") => Some(""),
        Some(rest) => rest.strip_prefix('/'),
        None => None,
    };
    match (rest, dirs::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(repo),
    }
}

#[cfg(test)]
mod tests;
