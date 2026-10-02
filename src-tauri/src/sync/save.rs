//! Saving what a Sync run created into its ticket (TUR-20).
//!
//! A run takes minutes, and the window stays free to move the meetings
//! folder while it does. So the ticket's file is looked up again from the
//! root as it is when the save starts ([`crate::folder_move::writing_in`]),
//! not where it was when the run began.
//!
//! By then the issue exists in the tracker. If the save still fails (a move
//! is running, the file is gone or read-only), the issue is kept in
//! [`Unsaved`] and the error shows its address. Retry then saves the kept
//! issue instead of running the agent again, which would make a second one.
//! [`Unsaved`] lives in memory only: the disk is the thing that just failed.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

use prompts::push_ticket::Synced;

use super::{find_ticket, record, tracker_name};
use crate::error::UiError;
use crate::folder_move::{FolderGate, writing_in};
use crate::tickets::TicketSummary;

/// An issue a Sync run created, and the tracker it is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Created {
    pub tracker: String,
    pub synced: Synced,
}

/// Issues created but not yet written to their ticket, by ticket id.
#[derive(Debug, Default)]
pub(crate) struct Unsaved(Mutex<HashMap<String, Created>>);

impl Unsaved {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Created>> {
        // Plain values, each written whole, so a poisoned lock holds nothing
        // half-done.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The issue `ticket_id`'s last run created and could not save.
    pub fn get(&self, ticket_id: &str) -> Option<Created> {
        self.lock().get(ticket_id).cloned()
    }

    /// Writes `created` to `ticket_id`'s file under the root as it is now.
    /// On failure the issue is kept for the next try, and the error says
    /// where it is.
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
            record(&path, &created.tracker, &created.synced)
        });
        match saved {
            Ok(summary) => {
                self.lock().remove(ticket_id);
                Ok(summary)
            }
            Err(error) => {
                let refused = not_saved(&created, &error);
                self.lock().insert(ticket_id.to_owned(), created);
                Err(refused)
            }
        }
    }
}

/// The error for an issue that exists in the tracker but not in its ticket.
fn not_saved(created: &Created, error: &UiError) -> UiError {
    let Synced {
        external_id,
        external_url,
    } = &created.synced;
    let reason = error.message.trim_end();
    let stop = if reason.ends_with(['.', '!', '?']) {
        ""
    } else {
        "."
    };
    UiError::app(
        "sync-not-saved",
        format!(
            "Created in {} as {external_id} but could not save the link: {reason}{stop} The issue is at {external_url}. Press Retry to save the link; it will not create another issue.",
            tracker_name(&created.tracker),
        ),
    )
}
