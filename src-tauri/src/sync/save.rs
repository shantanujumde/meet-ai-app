//! Saving what a Sync run created into its ticket (TUR-20).
//!
//! A run takes minutes, and the window stays free to move the meetings
//! folder while it does. So the ticket's file is looked up again from the
//! root as it is when the save starts ([`crate::folder_move::writing_in`]),
//! not where it was when the run began.
//!
//! By then the issue exists in the tracker. If the save fails (a move is
//! running, the file is read-only or missing for now), the issue is kept in
//! [`Unsaved`] and the error shows its address. Retry then saves the kept
//! issue instead of running the agent again, which would make a second one.
//!
//! A kept issue is only reused for the same task. A ticket is named by its
//! meeting and number, and a notes re-run can give an untouched number to a
//! different task, so the kept issue also holds a [`Fingerprint`] of the
//! ticket as it was when the sync started. A ticket that no longer matches
//! drops the kept issue and gets a normal sync. So does a ticket that was
//! synced some other way meanwhile, or whose meeting was deleted; the error
//! still shows where the dropped issue is.
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
        let bytes = fs::read(path)?;
        let mut hasher = DefaultHasher::new();
        bytes.hash(&mut hasher);
        Ok(Self {
            title: Ticket::parse(&String::from_utf8_lossy(&bytes)).title(),
            hash: hasher.finish(),
        })
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

/// Issues created but not yet written to their ticket.
#[derive(Debug, Default)]
pub(crate) struct Unsaved(Mutex<HashMap<Key, Created>>);

impl Unsaved {
    fn lock(&self) -> MutexGuard<'_, HashMap<Key, Created>> {
        // Plain values, each written whole, so a poisoned lock holds nothing
        // half-done.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The issue kept for this ticket, if any, whatever it now holds.
    #[cfg(test)]
    pub fn get(&self, ticket_id: &str, meeting_id: Option<&str>) -> Option<Created> {
        self.lock().get(&key(ticket_id, meeting_id)).cloned()
    }

    /// The issue kept for this ticket under `root`, if the ticket is still
    /// the task it was made for. `None` means run a normal sync: nothing is
    /// kept, or the ticket changed and the kept issue was dropped.
    ///
    /// A missing ticket keeps the issue (the file may be back soon) and is
    /// an error that shows it; a missing meeting folder means the meeting
    /// was deleted, and drops it.
    pub fn matching(
        &self,
        root: &Path,
        ticket_id: &str,
        meeting_id: Option<&str>,
    ) -> Result<Option<Created>, UiError> {
        let key = key(ticket_id, meeting_id);
        let Some(kept) = self.lock().get(&key).cloned() else {
            return Ok(None);
        };
        let path = match find_ticket(root, ticket_id, meeting_id) {
            Ok(path) => path,
            Err(error) if error.kind == "ticket-missing" => {
                return Err(if meeting_deleted(root, key.0.as_deref()) {
                    self.lock().remove(&key);
                    dropped(&kept, &error)
                } else {
                    not_saved(&kept, &error)
                });
            }
            Err(error) => return Err(error),
        };
        // Before the fingerprint: syncing the ticket some other way changed
        // its file too, and that drop must still say where the issue is.
        if let Err(error) = refuse_if_synced(ticket_id, &Ticket::read(&path)?) {
            self.lock().remove(&key);
            return Err(dropped(&kept, &error));
        }
        if Fingerprint::of(&path)? == kept.ticket {
            Ok(Some(kept))
        } else {
            self.lock().remove(&key);
            Ok(None)
        }
    }

    /// Writes `created` to the ticket's file under the root as it is now,
    /// unless the ticket is already synced. Any other failure keeps the issue
    /// for the next try; every error says where the issue is.
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
        match saved {
            Ok(summary) => {
                self.lock().remove(&key);
                Ok(summary)
            }
            Err(error) if error.kind == "sync-already-synced" => {
                self.lock().remove(&key);
                Err(dropped(&created, &error))
            }
            Err(error) => {
                let refused = not_saved(&created, &error);
                self.lock().insert(key, created);
                Err(refused)
            }
        }
    }
}

/// True when `meeting_id` names a meeting whose folder is not under `root`.
fn meeting_deleted(root: &Path, meeting_id: Option<&str>) -> bool {
    meeting_id.is_some_and(|id| store::folder::meeting_dir(root, id).is_ok_and(|dir| !dir.is_dir()))
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
