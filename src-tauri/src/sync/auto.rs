//! Tickets sent to the tracker on their own (TUR-113, SPEC A28).
//!
//! Every ticket on the Tickets page (`<root>/tickets/`) is sent to the
//! user's tracker in the background, one at a time, by the same agent run as
//! a Sync press ([`super::sync_in`]). Only these start a send:
//!
//! - a task approved from a meeting, and a ticket made by hand: that ticket;
//! - Retry: that ticket, even after a failure;
//! - a change in Settings, Tracker: every root ticket not in the tracker yet.
//!
//! Nothing else does: no timer, no retry loop, and nothing at launch. A
//! failed send is remembered with the settings it ran under ([`SettingsKey`]),
//! in memory only, and is not tried again on its own until Retry or until a
//! settings change gives it different settings. After a restart nothing is
//! remembered, so nothing is retried until one of the triggers above.
//!
//! With no tracker set up (Settings, Tracker never saved, or no agent
//! chosen) a ticket is just not sent: no run and no error. "Set up" is the
//! user's own choice in `config.jsonc`, never the defaults
//! ([`crate::config::tickets_chosen`]), so no ticket goes to a tracker the
//! user did not pick.
//!
//! Every change of a ticket's state goes out on
//! [`crate::events::TICKET_SYNC_EVENT`]; [`ticket_sync_states`] gives the
//! window the same states when it opens.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::Serialize;
use store::ticket::{self, Ticket};
use tauri::{AppHandle, Emitter as _, Manager as _};

use super::{ALREADY_SYNCED, RunSettings, SyncRuns, TICKET_MISSING, harness_for, sync_in};
use crate::config::{self, AgentConfig, Harness as HarnessChoice, TicketsConfig};
use crate::error::{UiError, on_blocking_pool};
use crate::events::TICKET_SYNC_EVENT;
use crate::folder_move::FolderGate;
use crate::meetings;
use crate::tickets::{self, TicketSummary};

/// The kind `sync_in` refuses a second run of the same ticket with.
const SYNC_BUSY: &str = "sync-busy";

/// How far one ticket is in being sent on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TicketSyncState {
    /// Not sent, and nothing is waiting to send it: no tracker is set up, or
    /// the ticket is gone. The window shows *Not sent* for a ticket with no
    /// issue key.
    NotSent,
    /// Waiting for the send before it to finish.
    Queued,
    /// The agent is sending it now.
    Sending,
    /// In the tracker; `ticket` has its key and link.
    Sent,
    /// The send did not create an issue; `error` says why and what to do.
    Failed,
}

/// One ticket's state, as [`crate::events::TICKET_SYNC_EVENT`] sends it.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TicketSyncStatus {
    pub ticket_id: String,
    pub state: TicketSyncState,
    /// Why it was not sent, with a kind the window can switch on
    /// (`sync::errors`). Only for [`TicketSyncState::Failed`].
    pub error: Option<UiError>,
    /// The ticket as it is now. Only for [`TicketSyncState::Sent`], and not
    /// then either when it was found already in the tracker.
    pub ticket: Option<TicketSummary>,
}

impl TicketSyncStatus {
    fn new(ticket_id: &str, state: TicketSyncState) -> Self {
        Self {
            ticket_id: ticket_id.to_owned(),
            state,
            error: None,
            ticket: None,
        }
    }
}

/// What the Tickets page needs to show sending: whether a tracker is set up,
/// which one, and every ticket queued, sending or failed.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TicketSyncOverview {
    /// A tracker is chosen in Settings, Tracker and an agent is chosen.
    pub tracker_set_up: bool,
    /// `linear`, `jira` or `github`, the default when none is set up.
    pub tracker: String,
    pub tickets: Vec<TicketSyncStatus>,
}

/// The settings a send ran under. A failure is not tried again on its own
/// while these stay the same.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SettingsKey {
    tracker: String,
    tracker_mcp: String,
    harness: &'static str,
    binary_path: Option<PathBuf>,
    model: Option<String>,
}

impl SettingsKey {
    pub fn of(agent: &AgentConfig, tickets: &TicketsConfig) -> Self {
        Self {
            tracker: tickets.tracker.clone(),
            tracker_mcp: tickets.tracker_mcp.clone(),
            harness: agent.harness.as_str(),
            binary_path: agent.binary_path.clone(),
            model: agent.model.clone(),
        }
    }
}

/// A tracker and agent that are set up, ready for a send.
#[derive(Debug, Clone)]
pub(crate) struct Setup {
    pub key: SettingsKey,
    pub agent: AgentConfig,
    pub run: RunSettings,
}

impl Setup {
    /// `None` when no tracker is chosen or no agent is: then nothing is sent.
    pub fn of(chosen: bool, agent: AgentConfig, tickets: TicketsConfig) -> Option<Self> {
        if !chosen || agent.harness == HarnessChoice::None {
            return None;
        }
        Some(Self {
            key: SettingsKey::of(&agent, &tickets),
            run: RunSettings::new(&agent, tickets),
            agent,
        })
    }
}

/// Where the queue gets its settings, sends a ticket, and reports.
pub(crate) trait Sender {
    /// The settings now. Read again for every ticket, so a change while the
    /// queue runs is used from the next ticket on.
    fn setup(&self) -> Result<Option<Setup>, UiError>;
    /// One send of root ticket `ticket_id`: [`super::sync_in`].
    fn send(&self, ticket_id: &str, setup: &Setup) -> Result<TicketSummary, UiError>;
    fn emit(&self, status: &TicketSyncStatus);
}

/// The queue of tickets to send, and what happened to each.
#[derive(Debug, Default)]
pub struct AutoSync(Mutex<Queue>);

#[derive(Debug, Default)]
struct Queue {
    waiting: VecDeque<String>,
    /// Queued by Retry: sent even after a failure under the same settings.
    forced: HashSet<String>,
    /// Every ticket queued, sending or failed, for [`ticket_sync_states`].
    states: HashMap<String, TicketSyncStatus>,
    /// The settings each failed ticket last failed under.
    failed: HashMap<String, SettingsKey>,
    /// A [`AutoSync::drain`] is running; a second is never started.
    draining: bool,
}

/// What the queue does with the ticket it took next.
#[derive(Debug, PartialEq, Eq)]
enum Next {
    /// Send it.
    Send(String),
    /// Leave it as it is: it failed under these same settings.
    Skip,
    /// Not sent: no tracker is set up.
    NotSent(String),
    /// Nothing is waiting.
    Done,
}

impl Queue {
    /// Adds `ticket_id` once: a ticket already waiting is not added again,
    /// and one sending now is left to finish. Returns the status to send, if
    /// the window should see a change.
    fn add(&mut self, ticket_id: &str, forced: bool) -> Option<TicketSyncStatus> {
        let state = self.states.get(ticket_id).map(|status| status.state);
        if state == Some(TicketSyncState::Sending) {
            return None;
        }
        if forced {
            self.forced.insert(ticket_id.to_owned());
        }
        if !self.waiting.iter().any(|id| id == ticket_id) {
            self.waiting.push_back(ticket_id.to_owned());
        }
        // A failure stays on screen until its send really starts again: if the
        // settings did not change, it never does.
        let hidden = self.failed.contains_key(ticket_id) && !forced;
        if state == Some(TicketSyncState::Queued) || hidden {
            return None;
        }
        let status = TicketSyncStatus::new(ticket_id, TicketSyncState::Queued);
        self.states.insert(ticket_id.to_owned(), status.clone());
        Some(status)
    }

    /// Takes the next ticket and decides what to do with it under `key`, the
    /// settings now (`None`: no tracker set up). With nothing waiting, the
    /// drain stops.
    fn next(&mut self, key: Option<&SettingsKey>) -> Next {
        let Some(ticket_id) = self.waiting.pop_front() else {
            self.draining = false;
            return Next::Done;
        };
        let forced = self.forced.remove(&ticket_id);
        let Some(key) = key else {
            self.states.remove(&ticket_id);
            self.failed.remove(&ticket_id);
            return Next::NotSent(ticket_id);
        };
        if !forced && self.failed.get(&ticket_id) == Some(key) {
            return Next::Skip;
        }
        let status = TicketSyncStatus::new(&ticket_id, TicketSyncState::Sending);
        self.states.insert(ticket_id.clone(), status);
        Next::Send(ticket_id)
    }

    /// The next ticket fails without a run: the settings could not be read.
    /// Not remembered as a failure under any settings, so the next trigger
    /// tries it. `None`, and the drain stops, when nothing is waiting.
    fn unreadable(&mut self, error: UiError) -> Option<TicketSyncStatus> {
        let Some(ticket_id) = self.waiting.pop_front() else {
            self.draining = false;
            return None;
        };
        self.forced.remove(&ticket_id);
        self.failed.remove(&ticket_id);
        let mut status = TicketSyncStatus::new(&ticket_id, TicketSyncState::Failed);
        status.error = Some(error);
        self.states.insert(ticket_id, status.clone());
        Some(status)
    }

    /// Records how the send of `ticket_id` under `key` ended.
    fn finish(
        &mut self,
        ticket_id: &str,
        key: &SettingsKey,
        sent: Result<TicketSummary, UiError>,
    ) -> TicketSyncStatus {
        let mut status = TicketSyncStatus::new(ticket_id, TicketSyncState::Sent);
        match sent {
            Ok(summary) => status.ticket = Some(summary),
            // Already in the tracker: synced some other way meanwhile.
            Err(error) if error.kind == ALREADY_SYNCED => {}
            // Gone (discarded, deleted), or a Sync press is sending it now
            // and reports itself.
            Err(error) if error.kind == TICKET_MISSING || error.kind == SYNC_BUSY => {
                status.state = TicketSyncState::NotSent;
            }
            Err(error) => {
                status.state = TicketSyncState::Failed;
                status.error = Some(error);
            }
        }
        if status.state == TicketSyncState::Failed {
            self.failed.insert(ticket_id.to_owned(), key.clone());
            self.states.insert(ticket_id.to_owned(), status.clone());
        } else {
            self.failed.remove(ticket_id);
            self.states.remove(ticket_id);
        }
        status
    }
}

impl AutoSync {
    fn lock(&self) -> MutexGuard<'_, Queue> {
        // Plain values, each written whole: a panic mid-update leaves nothing
        // half-done worth refusing over.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Queues `ticket_ids`; `forced` for Retry. Returns `true` when no
    /// [`Self::drain`] is running and the caller must start one.
    pub(crate) fn queue<'a>(
        &self,
        ticket_ids: impl IntoIterator<Item = &'a str>,
        forced: bool,
        emit: impl Fn(&TicketSyncStatus),
    ) -> bool {
        let (changed, start) = {
            let mut queue = self.lock();
            let changed: Vec<TicketSyncStatus> = ticket_ids
                .into_iter()
                .filter_map(|id| queue.add(id, forced))
                .collect();
            let start = !queue.draining && !queue.waiting.is_empty();
            queue.draining |= start;
            (changed, start)
        };
        changed.iter().for_each(&emit);
        start
    }

    /// Sends every queued ticket, one at a time, until none is left. Only
    /// ever one at a time: [`Self::queue`] says when to start it.
    pub(crate) fn drain(&self, sender: &impl Sender) {
        loop {
            let setup = match sender.setup() {
                Ok(setup) => setup,
                Err(error) => {
                    // A broken config.jsonc: say so on the ticket, once.
                    let status = self.lock().unreadable(error);
                    match status {
                        Some(status) => sender.emit(&status),
                        None => return,
                    }
                    continue;
                }
            };
            let next = self.lock().next(setup.as_ref().map(|setup| &setup.key));
            let (ticket_id, setup) = match (next, setup) {
                (Next::Done, _) => return,
                (Next::Skip, _) => continue,
                (Next::NotSent(ticket_id), _) => {
                    sender.emit(&TicketSyncStatus::new(&ticket_id, TicketSyncState::NotSent));
                    continue;
                }
                (Next::Send(ticket_id), Some(setup)) => (ticket_id, setup),
                // `next` sends only under settings it was given.
                (Next::Send(_), None) => continue,
            };
            sender.emit(&TicketSyncStatus::new(&ticket_id, TicketSyncState::Sending));
            let sent = sender.send(&ticket_id, &setup);
            if let Err(error) = &sent {
                tracing::info!(
                    ticket_id,
                    kind = error.kind,
                    "ticket not sent to the tracker"
                );
            }
            let status = self.lock().finish(&ticket_id, &setup.key, sent);
            sender.emit(&status);
        }
    }

    /// Every ticket queued, sending or failed, by id.
    pub(crate) fn states(&self) -> Vec<TicketSyncStatus> {
        let mut states: Vec<TicketSyncStatus> = self.lock().states.values().cloned().collect();
        states.sort_by(|a, b| a.ticket_id.cmp(&b.ticket_id));
        states
    }
}

/// The root tickets a settings change queues: the ones not in the tracker
/// yet, leaving out dropped ones and suggestions.
pub(crate) fn unsent(rows: &[TicketSummary]) -> Vec<&str> {
    rows.iter()
        .filter(|row| !row.suggested && row.synced_to.is_none() && row.external_url.is_none())
        .filter(|row| row.status.as_deref() != Some(ticket::Status::Dropped.as_str()))
        .map(|row| row.id.as_str())
        .collect()
}

/// One send of root ticket `ticket_id` under `setup`: [`sync_in`], filed
/// under the ticket's meeting as a Sync press in the window files it. So an
/// issue a press created but could not save is saved now, not made again.
pub(crate) fn send_root(
    runs: &SyncRuns,
    gate: Option<&FolderGate>,
    root: impl Fn() -> Result<PathBuf, UiError>,
    ticket_id: &str,
    setup: &Setup,
) -> Result<TicketSummary, UiError> {
    let meeting = root().ok().and_then(|root| meeting_of(&root, ticket_id));
    sync_in(runs, gate, root, ticket_id, meeting.as_deref(), || {
        Ok((harness_for(&setup.agent)?, setup.run.clone()))
    })
}

/// The meeting root ticket `ticket_id` came from, if it names one that is a
/// meeting folder name.
fn meeting_of(root: &Path, ticket_id: &str) -> Option<String> {
    ticket::parse_id(ticket_id)?;
    let path = root
        .join(store::TICKETS_DIR)
        .join(format!("{ticket_id}.md"));
    let meeting = Ticket::read(&path).ok()?.meeting()?;
    store::folder::meeting_dir(root, &meeting).ok()?;
    Some(meeting)
}

// --- the app ----------------------------------------------------------------

/// The running app as a [`Sender`].
struct AppSender<'a>(&'a AppHandle);

impl Sender for AppSender<'_> {
    fn setup(&self) -> Result<Option<Setup>, UiError> {
        current_setup()
    }

    fn send(&self, ticket_id: &str, setup: &Setup) -> Result<TicketSummary, UiError> {
        let app = self.0;
        let gate = app.try_state::<FolderGate>();
        send_root(
            &app.state::<SyncRuns>(),
            gate.as_deref(),
            meetings::root,
            ticket_id,
            setup,
        )
    }

    fn emit(&self, status: &TicketSyncStatus) {
        emit(self.0, status);
    }
}

fn emit(app: &AppHandle, status: &TicketSyncStatus) {
    if let Err(error) = app.emit(TICKET_SYNC_EVENT, status) {
        tracing::warn!(%error, "could not send a ticket's sync status to the window");
    }
}

/// The tracker and agent from `config.jsonc`, if both are set up.
fn current_setup() -> Result<Option<Setup>, UiError> {
    Ok(Setup::of(
        config::tickets_chosen()?,
        config::agent()?,
        config::tickets()?,
    ))
}

fn queue_in(app: &AppHandle, ticket_ids: &[&str], forced: bool) {
    let Some(auto) = app.try_state::<AutoSync>() else {
        return;
    };
    if !auto.queue(ticket_ids.iter().copied(), forced, |s| emit(app, s)) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AutoSync>().drain(&AppSender(&app));
    });
}

/// Queue new root tickets (approved, or made by hand) to be sent on their
/// own.
pub(crate) fn queue(app: &AppHandle, ticket_ids: &[&str]) {
    queue_in(app, ticket_ids, false);
}

/// Settings, Tracker changed: queue every root ticket not in the tracker
/// yet. A failure under the same settings as now stays as it is.
pub(crate) fn settings_changed(app: &AppHandle) {
    match meetings::root().and_then(|root| tickets::list_under(&root)) {
        Ok(rows) => queue(app, &unsent(&rows)),
        Err(error) => {
            tracing::warn!(message = %error.message, "could not list tickets to send after a tracker change");
        }
    }
}

// --- commands ---------------------------------------------------------------

/// Whether a tracker is set up, and every ticket queued, sending or failed.
/// After this, [`crate::events::TICKET_SYNC_EVENT`] keeps it current.
#[tauri::command]
#[specta::specta]
pub async fn ticket_sync_states(app: AppHandle) -> Result<TicketSyncOverview, UiError> {
    on_blocking_pool(move || {
        let tickets = app
            .try_state::<AutoSync>()
            .map(|auto| auto.states())
            .unwrap_or_default();
        Ok(TicketSyncOverview {
            tracker_set_up: current_setup()?.is_some(),
            tracker: config::tickets()?.tracker,
            tickets,
        })
    })
    .await?
}

/// Retry: send one root ticket again, even after a failure under the same
/// settings. Returns at once; the result comes on
/// [`crate::events::TICKET_SYNC_EVENT`].
#[tauri::command]
#[specta::specta]
pub fn retry_ticket_sync(app: AppHandle, ticket_id: String) -> Result<(), UiError> {
    // A ticket id, never a path: it comes from the window.
    if ticket::parse_id(&ticket_id).is_none() {
        return Err(UiError::app(
            "bad-ticket-id",
            format!("{ticket_id:?} is not a ticket id."),
        ));
    }
    queue_in(&app, &[ticket_id.as_str()], true);
    Ok(())
}

#[cfg(test)]
mod tests;
