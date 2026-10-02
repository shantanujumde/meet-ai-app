//! Sync: push one task to the user's tracker through their own agent (SPEC
//! A11, "Sync"; L11; TUR-11).
//!
//! The app never talks to Linear, Jira or GitHub. Each Sync press is a
//! separate background run of the user's agent CLI (`crates/agent`) that:
//!
//! - gets **only the task**: title, details, owner, due and a link back to
//!   the meeting, rendered from `push-ticket.md`. Never the transcript;
//! - may use only the tools of the MCP server named in `tickets.tracker_mcp`
//!   (`mcp__<server>__*`);
//! - must reply `{external_id, external_url}`, which this module writes to
//!   the ticket's `synced_to`, `external_id` and `external_url`.
//!
//! A reply with no issue key or no web address means **not synced**. TUR-5
//! measured a Codex sync whose MCP call was refused: it exits 0 and answers
//! with no key. Such a reply is an error the window shows next to Retry, and
//! the ticket file is left as it was.
//!
//! The commands here call `crates/agent` directly; the notes run and its
//! status events are `agent_run.rs`'s (TUR-10). Settings for the tracker are
//! in [`tracker`].

pub mod tracker;

use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use agent::{AgentError, CancelHandle, ClaudeHarness, CodexHarness, Harness, Job};
use prompts::push_ticket::{PushTicketInput, Synced, parse_sync_reply, render_push_ticket_from};
use store::folder_name::{prettify_slug, split_folder_name};
use store::meeting::Meeting;
use store::ticket::{self, Ticket};
use tauri::{AppHandle, Manager as _, State};
use tauri_plugin_opener::OpenerExt as _;

use crate::config::{self, AgentConfig, Harness as HarnessChoice, TicketsConfig};
use crate::error::UiError;
use crate::folder_move::FolderGate;
use crate::meetings;
use crate::tickets::{self, TicketSummary};

/// The Sync runs in flight, by ticket id, so the window can cancel one and a
/// second press on the same task is refused instead of making two issues.
#[derive(Debug, Default)]
pub struct SyncRuns(Mutex<HashMap<String, CancelHandle>>);

impl SyncRuns {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, CancelHandle>> {
        // The map holds only cancel flags, so a panic mid-insert leaves
        // nothing half-written worth refusing over.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Marks `ticket_id` as syncing until the returned claim is dropped.
    fn claim(&self, ticket_id: &str) -> Result<Claim<'_>, UiError> {
        let mut runs = self.lock();
        if runs.contains_key(ticket_id) {
            return Err(UiError::app(
                "sync-busy",
                format!("{ticket_id} is already being synced."),
            ));
        }
        let cancel = CancelHandle::new();
        runs.insert(ticket_id.to_owned(), cancel.clone());
        Ok(Claim {
            runs: self,
            ticket_id: ticket_id.to_owned(),
            cancel,
        })
    }

    /// Stops the Sync run for `ticket_id`, if there is one.
    fn cancel(&self, ticket_id: &str) {
        if let Some(cancel) = self.lock().get(ticket_id) {
            cancel.cancel();
        }
    }
}

/// One ticket's place in [`SyncRuns`]; dropping it frees the ticket.
struct Claim<'a> {
    runs: &'a SyncRuns,
    ticket_id: String,
    cancel: CancelHandle,
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        self.runs.lock().remove(&self.ticket_id);
    }
}

/// Everything a Sync run needs from the config.
#[derive(Debug, Clone)]
pub(crate) struct RunSettings {
    pub model: Option<String>,
    pub timeout: Duration,
    pub tickets: TicketsConfig,
}

impl RunSettings {
    fn new(agent: &AgentConfig, tickets: TicketsConfig) -> Self {
        Self {
            model: Some(agent.model.trim().to_owned()).filter(|m| !m.is_empty()),
            timeout: Duration::from_secs(agent.timeout_sec.max(1)),
            tickets,
        }
    }
}

// --- commands ---------------------------------------------------------------

/// Sync one task: one agent run that creates the issue, then the issue's key
/// and address written to the ticket. Takes as long as the agent does (up to
/// `agent.timeout_sec`), so it runs on the blocking pool.
#[tauri::command]
#[specta::specta]
pub async fn sync_task(
    app: AppHandle,
    ticket_id: String,
    meeting_id: Option<String>,
) -> Result<TicketSummary, UiError> {
    blocking(move || {
        let runs = app.state::<SyncRuns>();
        let claim = runs.claim(&ticket_id)?;
        let root = meetings::root()?;
        let agent = config::agent()?;
        let settings = RunSettings::new(&agent, config::tickets()?);
        let harness = harness_for(&agent)?;
        let (path, synced) = run(
            &root,
            &ticket_id,
            meeting_id.as_deref(),
            harness.as_ref(),
            &settings,
            &claim.cancel,
        )?;
        // Only the write goes through the gate: holding it for the whole run
        // would refuse a folder move for minutes.
        app.state::<FolderGate>()
            .writing(|| record(&path, &settings.tickets.tracker, &synced))
    })
    .await?
}

/// Stop a running Sync; its `sync_task` then fails with `agent-cancelled`.
#[tauri::command]
#[specta::specta]
pub fn cancel_sync(runs: State<'_, SyncRuns>, ticket_id: String) {
    runs.cancel(&ticket_id);
}

/// A meeting's tasks: the ones in its own `tickets/` folder, where the notes
/// run writes them, and shared ones that name it. Sorted by id.
#[tauri::command]
#[specta::specta]
pub async fn meeting_tasks(meeting_id: String) -> Result<Vec<TicketSummary>, UiError> {
    blocking(move || meeting_tasks_in(&meetings::root()?, &meeting_id)).await?
}

/// Open a synced task's issue in the browser. The address comes from the
/// ticket file and must be `https://`; the webview never opens a URL itself.
#[tauri::command]
#[specta::specta]
pub async fn open_synced_issue(
    app: AppHandle,
    ticket_id: String,
    meeting_id: Option<String>,
) -> Result<(), UiError> {
    blocking(move || {
        let url = synced_url(&meetings::root()?, &ticket_id, meeting_id.as_deref())?;
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|error| UiError::app("open-failed", error.to_string()))
    })
    .await?
}

/// Run `work` on Tauri's blocking pool (see `commands::on_blocking_pool`).
pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, UiError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| UiError::app("task-failed", error.to_string()))
}

// --- the run ----------------------------------------------------------------

/// The user's chosen agent CLI as a [`Harness`]. Finding the binary does not
/// run a sign-in check: a signed-out CLI says so when the run starts.
pub(crate) fn harness_for(agent: &AgentConfig) -> Result<Box<dyn Harness>, UiError> {
    let path = find_binary(agent)?;
    Ok(match agent.harness {
        HarnessChoice::Codex => Box::new(CodexHarness::with_binary(path)),
        _ => {
            let mut claude = ClaudeHarness::new().with_binary(&path);
            if let Some(search_path) = path.parent().and_then(agent::process::search_path_with) {
                claude = claude.with_search_path(search_path);
            }
            Box::new(claude)
        }
    })
}

/// Where the chosen agent CLI is: `agent.binary_path`, the login shell, then
/// the usual install folders (`agent::detect::find`).
pub(crate) fn find_binary(agent: &AgentConfig) -> Result<PathBuf, UiError> {
    let (cli, display) = match agent.harness {
        HarnessChoice::None => {
            return Err(UiError::app(
                "sync-no-agent",
                "Sync needs an agent. Pick Claude Code or Codex in Settings.",
            ));
        }
        HarnessChoice::ClaudeCode => (agent::detect::CLAUDE, agent::claude::DISPLAY_NAME),
        HarnessChoice::Codex => (agent::detect::CODEX, "Codex"),
    };
    let lookup = agent::detect::Lookup::system(agent.binary_path.as_deref());
    agent::detect::find(&cli, &lookup).ok_or_else(|| {
        agent_error(AgentError::NotInstalled {
            harness: display.to_owned(),
        })
    })
}

/// Renders the push-ticket prompt for one ticket, runs `harness` with it and
/// reads the reply. Returns the ticket's file and what was created; writes
/// nothing.
pub(crate) fn run(
    root: &Path,
    ticket_id: &str,
    meeting_id: Option<&str>,
    harness: &dyn Harness,
    settings: &RunSettings,
    cancel: &CancelHandle,
) -> Result<(PathBuf, Synced), UiError> {
    let path = find_ticket(root, ticket_id, meeting_id)?;
    let found = Ticket::read(&path)?;
    refuse_if_synced(ticket_id, &found)?;
    let tickets_config = &settings.tickets;
    let tools = agent::mcp::tracker_tools(harness.id(), &tickets_config.tracker_mcp);
    if tools.is_empty() {
        return Err(UiError::app(
            "sync-no-tracker",
            "Pick the tracker's MCP server in Settings before syncing.",
        ));
    }
    let input = push_ticket_input(root, ticket_id, meeting_id, &found, tickets_config)?;
    let prompt = render_push_ticket_from(root, &input)?;

    let mut job = Job::sync(prompt, prompts::push_ticket::sync_schema(), tools);
    job.model = settings.model.clone();
    job.timeout = settings.timeout;
    job.cancel = cancel.clone();
    let reply = harness.run(&job).map_err(agent_error)?;
    let synced = parse_sync_reply(&reply).ok_or_else(|| not_synced(tickets_config))?;
    Ok((path, synced))
}

/// Writes what a Sync run created into the ticket file, re-read first so an
/// edit made during the run is kept. Never called for a run with no issue.
pub(crate) fn record(
    path: &Path,
    tracker: &str,
    synced: &Synced,
) -> Result<TicketSummary, UiError> {
    let mut found = Ticket::read(path)?;
    found.frontmatter.set_str("synced_to", Some(tracker));
    found
        .frontmatter
        .set_str("external_id", Some(&synced.external_id));
    found
        .frontmatter
        .set_str("external_url", Some(&synced.external_url));
    found.write(path)?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    Ok(tickets::summary_of(stem, &found))
}

/// A ticket already in the tracker is not synced again: a second run would
/// make a second issue.
fn refuse_if_synced(ticket_id: &str, found: &Ticket) -> Result<(), UiError> {
    let tracker = found.synced_to();
    if tracker.is_none() && found.frontmatter.get_str("external_url").is_none() {
        return Ok(());
    }
    let place = tracker.as_deref().map_or("the tracker", tracker_name);
    let message = match found.frontmatter.get_str("external_id") {
        Some(key) => format!("{ticket_id} is already in {place} as {key}."),
        None => format!("{ticket_id} is already in {place}."),
    };
    Err(UiError::app("sync-already-synced", message))
}

/// The prompt's input: the task and its meeting, nothing from the transcript.
fn push_ticket_input(
    root: &Path,
    ticket_id: &str,
    meeting_id: Option<&str>,
    found: &Ticket,
    tickets_config: &TicketsConfig,
) -> Result<PushTicketInput, UiError> {
    let meeting_id = meeting_id
        .map(str::to_owned)
        .or_else(|| found.meeting())
        .filter(|id| !id.is_empty());
    let mut meeting_title = None;
    let mut meeting_date = None;
    let mut meeting_file = None;
    if let Some(id) = &meeting_id {
        let dir = store::folder::meeting_dir(root, id)?;
        let file = dir.join(store::MEETING_FILE);
        if let Some(meeting) = Meeting::read(&file)? {
            meeting_title = meeting.title();
            meeting_date = meeting.date();
        }
        let (day, time, slug) = split_folder_name(id);
        meeting_title =
            meeting_title.or_else(|| Some(slug.map(prettify_slug).unwrap_or_else(|| id.clone())));
        meeting_date = meeting_date
            .or_else(|| day.map(|day| format!("{day} {}", time.unwrap_or_default())))
            .map(|date| date.trim().to_owned());
        meeting_file = Some(file.display().to_string());
    }
    Ok(PushTicketInput {
        ticket_id: found.id().unwrap_or_else(|| ticket_id.to_owned()),
        title: found.title().unwrap_or_else(|| ticket_id.to_owned()),
        details: found.body.trim().to_owned(),
        owner: found.assignee(),
        due: due_in(&found.body),
        meeting_id,
        meeting_title,
        meeting_date,
        meeting_file,
        tracker: tickets_config.tracker.clone(),
        tracker_mcp: tickets_config.tracker_mcp.clone(),
    })
}

/// The `Due: X.` line the notes writer puts in a ticket's body (§3.3 has no
/// due key).
fn due_in(body: &str) -> Option<String> {
    body.lines().find_map(|line| {
        let due = line.trim().strip_prefix("Due: ")?.strip_suffix('.')?.trim();
        (!due.is_empty()).then(|| due.to_owned())
    })
}

/// The error for a run that finished without creating an issue.
fn not_synced(tickets_config: &TicketsConfig) -> UiError {
    UiError::app(
        "sync-not-done",
        format!(
            "The agent finished but did not create an issue in {}. Check that \"{}\" is connected and signed in (Settings, Tracker), then press Retry.",
            tracker_name(&tickets_config.tracker),
            tickets_config.tracker_mcp
        ),
    )
}

/// `linear` → `Linear`, for messages.
pub(crate) fn tracker_name(tracker: &str) -> &str {
    match tracker {
        "linear" => "Linear",
        "jira" => "Jira",
        "github" => "GitHub",
        other => other,
    }
}

/// An agent run's failure, worded by `crates/agent`, with a stable kind the
/// window can switch on.
pub(crate) fn agent_error(error: AgentError) -> UiError {
    let kind = match &error {
        AgentError::NotInstalled { .. } => "agent-not-installed",
        AgentError::NotSignedIn { .. } => "agent-not-signed-in",
        AgentError::TimedOut { .. } => "agent-timed-out",
        AgentError::Cancelled => "agent-cancelled",
        AgentError::CliFailed { .. } => "agent-failed",
        AgentError::InvalidJson { .. } | AgentError::SchemaMismatch { .. } => "agent-bad-reply",
        AgentError::CouldNotStart { .. } => "agent-could-not-start",
    };
    UiError::app(kind, error.to_string())
}

// --- finding tickets --------------------------------------------------------

/// The ticket's file: in its meeting's `tickets/` folder first, then in the
/// shared one at the root.
fn find_ticket(root: &Path, ticket_id: &str, meeting_id: Option<&str>) -> Result<PathBuf, UiError> {
    // A ticket id, never a path: it comes from the window.
    if ticket::parse_id(ticket_id).is_none() {
        return Err(UiError::app(
            "bad-ticket-id",
            format!("{ticket_id:?} is not a ticket id."),
        ));
    }
    let file = format!("{ticket_id}.md");
    let mut dirs = Vec::new();
    if let Some(id) = meeting_id.filter(|id| !id.is_empty()) {
        dirs.push(store::folder::meeting_dir(root, id)?.join(store::TICKETS_DIR));
    }
    dirs.push(root.join(store::TICKETS_DIR));
    dirs.into_iter()
        .map(|dir| dir.join(&file))
        .find(|path| path.is_file())
        .ok_or_else(|| {
            UiError::app(
                "ticket-missing",
                format!("Could not find the file for {ticket_id}."),
            )
        })
}

/// [`meeting_tasks`] under `root`.
pub(crate) fn meeting_tasks_in(
    root: &Path,
    meeting_id: &str,
) -> Result<Vec<TicketSummary>, UiError> {
    let dir = store::folder::meeting_dir(root, meeting_id)?.join(store::TICKETS_DIR);
    let mut rows: Vec<TicketSummary> = Vec::new();
    for (stem, path) in ticket_files(&dir)? {
        let mut summary = match Ticket::read(&path) {
            Ok(found) => tickets::summary_of(&stem, &found),
            Err(_) => unreadable(&stem),
        };
        // Written by the notes run inside the meeting's folder, so it is this
        // meeting's even if the file does not say so.
        summary.meeting.get_or_insert_with(|| meeting_id.to_owned());
        rows.push(summary);
    }
    for shared in tickets::list_under(root)? {
        let theirs = shared.meeting.as_deref() == Some(meeting_id);
        if theirs && !rows.iter().any(|row| row.id == shared.id) {
            rows.push(shared);
        }
    }
    rows.sort_by(|a, b| {
        ticket::parse_id(&a.id)
            .cmp(&ticket::parse_id(&b.id))
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(rows)
}

/// A ticket file that could not be read, still listed so it is not lost.
fn unreadable(stem: &str) -> TicketSummary {
    TicketSummary {
        id: stem.to_owned(),
        title: stem.to_owned(),
        status: None,
        meeting: None,
        body: String::new(),
        has_problems: true,
        synced_to: None,
        external_id: None,
        external_url: None,
    }
}

/// The `.md` files in `dir` as `(file stem, path)`, by name. A missing folder
/// is an empty list.
fn ticket_files(dir: &Path) -> Result<Vec<(String, PathBuf)>, UiError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut files = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") || !path.is_file() {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            files.push((stem.to_owned(), path.clone()));
        }
    }
    files.sort();
    Ok(files)
}

/// The synced issue's address, checked the same way a Sync reply is.
pub(crate) fn synced_url(
    root: &Path,
    ticket_id: &str,
    meeting_id: Option<&str>,
) -> Result<String, UiError> {
    let found = Ticket::read(&find_ticket(root, ticket_id, meeting_id)?)?;
    let reply = serde_json::json!({
        "external_id": found.frontmatter.get_str("external_id").unwrap_or_else(|| ticket_id.to_owned()),
        "external_url": found.frontmatter.get_str("external_url"),
    });
    parse_sync_reply(&reply)
        .map(|synced| synced.external_url)
        .ok_or_else(|| {
            UiError::app(
                "sync-no-link",
                format!("{ticket_id} has no issue link to open."),
            )
        })
}

#[cfg(test)]
mod tests;
