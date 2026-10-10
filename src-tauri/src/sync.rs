//! Sync: push one task to the user's tracker through their own agent (SPEC
//! A11, "Sync"; L11; TUR-11).
//!
//! The app never talks to Linear, Jira or GitHub. Each Sync press is a
//! separate background run of the user's agent CLI (`crates/agent`) that:
//!
//! - gets **only the task**: title, details, owner, due and the meeting's
//!   title and date, rendered from `push-ticket.md`. Never the transcript,
//!   and no local path: the issue is shared, and a path shows the user's
//!   folder names;
//! - may use only the tools of the MCP server named in `tickets.tracker_mcp`
//!   (`mcp__<server>__*`);
//! - must reply `{external_id, external_url}`, which this module writes to
//!   the ticket's `synced_to`, `external_id` and `external_url`.
//!
//! A reply with no issue key or no web address means **not synced**. TUR-5
//! measured a Codex sync whose MCP call was refused: it exits 0 and answers
//! with no key. Such a reply is an error the window shows next to Retry, and
//! the ticket file is left as it was. Each cause has its own kind and words
//! ([`errors`], TUR-113).
//!
//! The commands here call `crates/agent` directly; the notes run and its
//! status events are `agent_run.rs`'s (TUR-10). Settings for the tracker are
//! in [`tracker`].

pub mod auto;
pub mod check;
mod errors;
mod kept;
mod runs;
mod save;
pub mod tracker;

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use agent::{AgentError, CancelHandle, ClaudeHarness, CodexHarness, Harness, Job};
use prompts::push_ticket::{PushTicketInput, Synced, parse_sync_reply, render_push_ticket_from};
use store::folder_name::{prettify_slug, split_folder_name};
use store::meeting::Meeting;
use store::ticket::{self, Ticket};
use tauri::{AppHandle, Manager as _, State};
use tauri_plugin_opener::OpenerExt as _;

use crate::config::{self, AgentConfig, Harness as HarnessChoice, TicketsConfig};
use crate::error::{UiError, on_blocking_pool};
use crate::folder_move::FolderGate;
use crate::meetings;
use crate::sync::save::{Created, Fingerprint};
use crate::tickets::{self, TicketSummary};

pub(crate) use errors::{agent_error, tracker_name};
pub use runs::SyncRuns;

/// The ticket's file is not where it should be.
pub(crate) const TICKET_MISSING: &str = "ticket-missing";

/// The ticket is already in the tracker.
pub(crate) const ALREADY_SYNCED: &str = "sync-already-synced";

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
            model: agent.model.clone(),
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
    on_blocking_pool(move || {
        let gate = app.try_state::<FolderGate>();
        sync_in(
            &app.state::<SyncRuns>(),
            gate.as_deref(),
            meetings::root,
            &ticket_id,
            meeting_id.as_deref(),
            || {
                let agent = config::agent()?;
                let settings = RunSettings::new(&agent, config::tickets()?);
                Ok((harness_for(&agent)?, settings))
            },
        )
    })
    .await?
}

/// Forget the issue a Sync created but could not attach to this task
/// (`sync-not-saved`, `sync-not-attached`), so the next Sync runs afresh. The
/// window shows the issue's link until the user dismisses it.
#[tauri::command]
#[specta::specta]
pub async fn dismiss_unsaved_sync(
    app: AppHandle,
    ticket_id: String,
    meeting_id: Option<String>,
) -> Result<(), UiError> {
    on_blocking_pool(move || {
        let gate = app.try_state::<FolderGate>();
        let unsaved = &app.state::<SyncRuns>().unsaved;
        unsaved.dismiss(
            gate.as_deref(),
            &meetings::root,
            &ticket_id,
            meeting_id.as_deref(),
        );
    })
    .await
}

/// Stop a running Sync; its `sync_task` then fails with `agent-cancelled`.
#[tauri::command]
#[specta::specta]
pub fn cancel_sync(runs: State<'_, SyncRuns>, ticket_id: String, meeting_id: Option<String>) {
    runs.cancel(&ticket_id, meeting_id.as_deref());
}

/// A meeting's tasks: the ones in its own `tickets/` folder, where the notes
/// run writes them, and shared ones that name it. Sorted by id.
#[tauri::command]
#[specta::specta]
pub async fn meeting_tasks(meeting_id: String) -> Result<Vec<TicketSummary>, UiError> {
    on_blocking_pool(move || meeting_tasks_in(&meetings::root()?, &meeting_id)).await?
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
    on_blocking_pool(move || {
        let url = synced_url(&meetings::root()?, &ticket_id, meeting_id.as_deref())?;
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|error| UiError::app("open-failed", error.to_string()))
    })
    .await?
}

// --- the run ----------------------------------------------------------------

/// The user's chosen agent CLI as a [`Harness`]. Finding the binary does not
/// run a sign-in check: a signed-out CLI says so when the run starts.
pub(crate) fn harness_for(agent: &AgentConfig) -> Result<Box<dyn Harness>, UiError> {
    let path = locate(agent, errors::send_error)?;
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
    locate(agent, agent_error)
}

/// [`find_binary`], with `missing` wording the not-installed error.
fn locate(agent: &AgentConfig, missing: fn(AgentError) -> UiError) -> Result<PathBuf, UiError> {
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
        missing(AgentError::NotInstalled {
            harness: display.to_owned(),
        })
    })
}

/// [`sync_task`] without the `AppHandle`: `root` looks the meetings root up
/// and `agent` gives the harness and settings, asked for only when a run is
/// needed.
///
/// The root is looked up twice: once to start the run, and again for the
/// save, so a folder moved during the run gets the link in its new place.
/// Only the save goes through `gate`: holding it for the whole run would
/// refuse a folder move for minutes. While an issue an earlier run created
/// is kept unsaved, a Sync tries to save that issue and never runs the agent
/// (`save.rs`).
pub(crate) fn sync_in(
    runs: &SyncRuns,
    gate: Option<&FolderGate>,
    root: impl Fn() -> Result<PathBuf, UiError>,
    ticket_id: &str,
    meeting_id: Option<&str>,
    agent: impl FnOnce() -> Result<(Box<dyn Harness>, RunSettings), UiError>,
) -> Result<TicketSummary, UiError> {
    let claim = runs.claim(ticket_id, meeting_id)?;
    let created = match runs.unsaved.kept(gate, &root, ticket_id, meeting_id)? {
        Some(created) => created,
        None => {
            let (harness, settings) = agent()?;
            let start = root()?;
            let ticket = Fingerprint::of(&find_ticket(&start, ticket_id, meeting_id)?)?;
            let synced = run(
                &start,
                ticket_id,
                meeting_id,
                harness.as_ref(),
                &settings,
                &claim.cancel,
            )?;
            Created {
                tracker: settings.tickets.tracker,
                synced,
                ticket,
            }
        }
    };
    runs.unsaved
        .save(gate, &root, ticket_id, meeting_id, created)
}

/// Renders the push-ticket prompt for one ticket, runs `harness` with it and
/// reads the reply. Returns what was created; writes nothing.
pub(crate) fn run(
    root: &Path,
    ticket_id: &str,
    meeting_id: Option<&str>,
    harness: &dyn Harness,
    settings: &RunSettings,
    cancel: &CancelHandle,
) -> Result<Synced, UiError> {
    let path = find_ticket(root, ticket_id, meeting_id)?;
    let found = Ticket::read(&path)?;
    refuse_if_synced(ticket_id, &found)?;
    let tickets_config = &settings.tickets;
    let tools = agent::mcp::tracker_tools(harness.id(), &tickets_config.tracker_mcp);
    if tools.is_empty() {
        return Err(UiError::app(
            "sync-no-tracker",
            "Pick your tracker connection (also called an MCP server) in Settings before syncing.",
        ));
    }
    let input = push_ticket_input(root, ticket_id, meeting_id, &found, tickets_config)?;
    let prompt = render_push_ticket_from(root, &input)?;

    let mut job = Job::sync(prompt, prompts::push_ticket::sync_schema(), tools);
    job.model = settings.model.clone();
    job.timeout = settings.timeout;
    job.cancel = cancel.clone();
    let reply = harness.run(&job).map_err(errors::send_error)?;
    parse_sync_reply(&reply).ok_or_else(|| errors::not_synced(tickets_config, harness.id(), &reply))
}

/// Writes what a Sync run created into the ticket file. Never called for a
/// run with no issue, nor for a ticket that changed since its sync started
/// (`save.rs` refuses that save and keeps the issue).
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
    Err(UiError::app(ALREADY_SYNCED, message))
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
        tracker: tickets_config.tracker.clone(),
        tracker_mcp: tickets_config.tracker_mcp.clone(),
    })
}

/// The `Due: X.` line the notes writer puts in a ticket's body (§3.3 has no
/// due key).
pub(crate) fn due_in(body: &str) -> Option<String> {
    body.lines().find_map(|line| {
        let due = line.trim().strip_prefix("Due: ")?.strip_suffix('.')?.trim();
        (!due.is_empty()).then(|| due.to_owned())
    })
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
                TICKET_MISSING,
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
            Err(_) => tickets::unreadable(&stem),
        };
        // Written by the notes run inside the meeting's folder, so it is this
        // meeting's even if the file does not say so; and not approved yet.
        summary.meeting.get_or_insert_with(|| meeting_id.to_owned());
        summary.suggested = true;
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
    tickets::name_meetings(root, &mut rows);
    Ok(rows)
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
