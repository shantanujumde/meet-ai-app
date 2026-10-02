//! Saving what a Sync run created into its ticket (TUR-20).
//!
//! A run takes minutes, and the window stays free to move the meetings
//! folder while it does. So the ticket's file is looked up again from the
//! root as it is when the save starts ([`crate::folder_move::writing_in`]),
//! not where it was when the run began.
//!
//! By then the issue exists in the tracker. If the save fails for a reason
//! that can pass (a move is running, the file is read-only), the issue is
//! kept in [`Unsaved`] and the error shows its address. Retry then saves the
//! kept issue instead of running the agent again, which would make a second
//! one. If the save can never work (the ticket's file is gone, or it was
//! synced some other way meanwhile), the issue is dropped and the error
//! still shows where it is. [`Unsaved`] lives in memory only: the disk is the
//! thing that just failed.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

use prompts::push_ticket::Synced;
use store::ticket::Ticket;

use super::{find_ticket, record, refuse_if_synced, tracker_name};
use crate::error::UiError;
use crate::folder_move::{FolderGate, writing_in};
use crate::tickets::TicketSummary;

/// An issue a Sync run created, and the tracker it is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Created {
    pub tracker: String,
    pub synced: Synced,
}

/// Which ticket an issue belongs to: its meeting and its id. Ticket numbers
/// are only unique within one meeting plus the shared folder, so two
/// meetings can both have a `TICK-0001`.
type Key = (Option<String>, String);

fn key(ticket_id: &str, meeting_id: Option<&str>) -> Key {
    let meeting = meeting_id.filter(|id| !id.is_empty()).map(str::to_owned);
    (meeting, ticket_id.to_owned())
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

    /// The issue this ticket's last run created and could not save.
    pub fn get(&self, ticket_id: &str, meeting_id: Option<&str>) -> Option<Created> {
        self.lock().get(&key(ticket_id, meeting_id)).cloned()
    }

    /// Writes `created` to the ticket's file under the root as it is now,
    /// unless the ticket is already synced. A failure that can pass keeps the
    /// issue for the next try; every error says where the issue is.
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
            refuse_if_synced(ticket_id, &Ticket::read(&path)?)?;
            record(&path, &created.tracker, &created.synced)
        });
        let key = key(ticket_id, meeting_id);
        let error = match saved {
            Ok(summary) => {
                self.lock().remove(&key);
                return Ok(summary);
            }
            Err(error) => error,
        };
        match error.kind {
            "ticket-missing" | "sync-already-synced" => {
                self.lock().remove(&key);
                Err(dropped(&created, &error))
            }
            _ => {
                let refused = not_saved(&created, &error);
                self.lock().insert(key, created);
                Err(refused)
            }
        }
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

/// The error for an issue that exists in the tracker but not yet in its
/// ticket, and that Retry will save.
fn not_saved(created: &Created, error: &UiError) -> UiError {
    let Synced {
        external_id,
        external_url,
    } = &created.synced;
    UiError::app(
        "sync-not-saved",
        format!(
            "Created in {} as {external_id} but could not save the link: {} The issue is at {external_url}. Press Retry to save the link; it will not create another issue.",
            tracker_name(&created.tracker),
            sentence(error),
        ),
    )
}

/// The error for an issue whose ticket can never take its link: `error`
/// says why, with its own kind, and the message says where the issue is.
fn dropped(created: &Created, error: &UiError) -> UiError {
    let Synced {
        external_id,
        external_url,
    } = &created.synced;
    UiError::app(
        error.kind,
        format!(
            "{} This sync created {external_id} in {}, at {external_url}, and did not save it to the task. Delete it there if you do not need it.",
            sentence(error),
            tracker_name(&created.tracker),
        ),
    )
}
