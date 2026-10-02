//! Saving what a Sync run created into its ticket (TUR-20).
//!
//! A run takes minutes, and the window stays free to move the meetings
//! folder while it does. So the ticket's file is looked up again from the
//! root as it is when the save starts ([`crate::folder_move::writing_in`]),
//! not where it was when the run began.
//!
//! By then the issue exists in the tracker, and it is never dropped without
//! the user's say. A save that fails keeps the issue in [`Unsaved`], and
//! every later Sync of that task tries to save the kept issue instead of
//! running the agent again, until it is saved or the user dismisses it
//! ([`Unsaved::dismiss`]). The error always shows the issue's address:
//!
//! - [`SYNC_NOT_SAVED`]: the link could not be written (a move is running,
//!   the file is missing for now or read-only). Retry may work.
//! - [`SYNC_NOT_ATTACHED`]: the ticket is no longer the task the issue was
//!   made for. Its file changed after the sync started (the user's editor,
//!   or a notes re-run that gave the number to a different task; checked by
//!   [`Fingerprint`]), or it was synced some other way.
//!
//! [`Unsaved`] lives in memory only: the disk is the thing that just failed.

use std::collections::HashMap;
use std::fs;
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use prompts::push_ticket::Synced;
use store::ticket::Ticket;

use super::{find_ticket, record, refuse_if_synced, tracker_name};
use crate::error::UiError;
use crate::folder_move::{FolderGate, writing_in};
use crate::tickets::TicketSummary;

/// The issue exists but its link is not in the ticket yet; Retry saves it.
pub(crate) const SYNC_NOT_SAVED: &str = "sync-not-saved";

/// The issue exists but the ticket is no longer the task it was made for.
pub(crate) const SYNC_NOT_ATTACHED: &str = "sync-not-attached";

/// An issue a Sync run created, the tracker it is in, and the ticket it was
/// made for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Created {
    pub tracker: String,
    pub synced: Synced,
    pub ticket: Fingerprint,
}

/// A ticket file as it was when its sync started: its title and a hash of
/// its bytes. Only compared within one run of the app, so the std hasher's
/// fixed keys are enough.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fingerprint {
    title: Option<String>,
    hash: u64,
}

impl Fingerprint {
    pub fn of(path: &Path) -> Result<Self, UiError> {
        Ok(Self::of_bytes(&fs::read(path)?))
    }

    fn of_bytes(bytes: &[u8]) -> Self {
        let mut hasher = DefaultHasher::new();
        bytes.hash(&mut hasher);
        Self {
            title: Ticket::parse(&String::from_utf8_lossy(bytes)).title(),
            hash: hasher.finish(),
        }
    }
}

/// Where a kept issue is filed: the ticket's meeting and number. Ticket
/// numbers are only unique within one meeting plus the shared folder, so two
/// meetings can both have a `TICK-0001`.
type Key = (Option<String>, String);

fn key(ticket_id: &str, meeting_id: Option<&str>) -> Key {
    let meeting = meeting_id.filter(|id| !id.is_empty()).map(str::to_owned);
    (meeting, ticket_id.to_owned())
}

/// Why a save did not happen; both keep the issue.
enum Refusal {
    NotSaved(UiError),
    NotAttached(UiError),
}

impl From<UiError> for Refusal {
    fn from(error: UiError) -> Self {
        Self::NotSaved(error)
    }
}

/// Issues created but not yet written to their ticket.
#[derive(Debug, Default)]
pub(crate) struct Unsaved(Mutex<HashMap<Key, Created>>);

impl Unsaved {
    fn lock(&self) -> MutexGuard<'_, HashMap<Key, Created>> {
        // Plain values, each written whole, so a poisoned lock holds nothing
        // half-done.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The issue kept for this ticket, if any. While there is one, a Sync of
    /// the ticket saves it and never runs the agent.
    pub fn get(&self, ticket_id: &str, meeting_id: Option<&str>) -> Option<Created> {
        self.lock().get(&key(ticket_id, meeting_id)).cloned()
    }

    /// Forgets the issue kept for this ticket: the user has its link and
    /// asked for the next Sync to be a fresh one.
    pub fn dismiss(&self, ticket_id: &str, meeting_id: Option<&str>) {
        self.lock().remove(&key(ticket_id, meeting_id));
    }

    /// Writes `created` to the ticket's file under the root as it is now, if
    /// the ticket is still the task it was made for and not synced already.
    /// Otherwise the issue is kept, and the error shows where it is.
    pub fn save(
        &self,
        gate: Option<&FolderGate>,
        root: impl FnOnce() -> Result<PathBuf, UiError>,
        ticket_id: &str,
        meeting_id: Option<&str>,
        created: Created,
    ) -> Result<TicketSummary, UiError> {
        let saved = writing_in(gate, root, |root| {
            let path = find_ticket(root, ticket_id, meeting_id)?;
            let bytes = fs::read(&path).map_err(UiError::from)?;
            let found = Ticket::parse(&String::from_utf8_lossy(&bytes));
            refuse_if_synced(ticket_id, &found).map_err(Refusal::NotAttached)?;
            if Fingerprint::of_bytes(&bytes) != created.ticket {
                return Err(Refusal::NotAttached(UiError::app(
                    SYNC_NOT_ATTACHED,
                    format!("{ticket_id} changed after the sync started."),
                )));
            }
            Ok(record(&path, &created.tracker, &created.synced)?)
        });
        let key = key(ticket_id, meeting_id);
        let error = match saved {
            Ok(summary) => {
                self.lock().remove(&key);
                return Ok(summary);
            }
            Err(Refusal::NotSaved(error)) => not_saved(&created, &error),
            Err(Refusal::NotAttached(error)) => not_attached(&created, &error),
        };
        self.lock().insert(key, created);
        Err(error)
    }
}

/// `error`'s message as a sentence, ending in a full stop.
fn sentence(error: &UiError) -> String {
    let reason = error.message.trim_end();
    if reason.ends_with(['.', '!', '?']) {
        reason.to_owned()
    } else {
        format!("{reason}.")
    }
}

/// The error for an issue whose link could not be written yet.
fn not_saved(created: &Created, error: &UiError) -> UiError {
    let Synced {
        external_id,
        external_url,
    } = &created.synced;
    UiError::app(
        SYNC_NOT_SAVED,
        format!(
            "Created in {} as {external_id} but could not save the link: {} The issue is at {external_url}. Press Retry to save the link; it will not create another issue.",
            tracker_name(&created.tracker),
            sentence(error),
        ),
    )
}

/// The error for an issue the ticket can no longer take: `error` says why.
fn not_attached(created: &Created, error: &UiError) -> UiError {
    UiError::app(
        SYNC_NOT_ATTACHED,
        format!(
            "Created in {} but couldn't attach it to this task: {} {} Sync will not create another issue until you dismiss this.",
            tracker_name(&created.tracker),
            created.synced.external_url,
            sentence(error),
        ),
    )
}
