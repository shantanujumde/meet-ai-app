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
//! [`Unsaved`] is also written to a small file under the root (TUR-21,
//! [`super::kept`]), so a kept issue survives the app quitting before Retry.
//! Memory stays the first copy: the disk may be the thing that just failed,
//! so a write that fails or is refused by a folder move is tried again on the
//! next Sync, Dismiss or the end of the move. After a restart an issue read
//! from that file is reused only while its ticket still matches its
//! [`Fingerprint`]; a ticket that changed in between drops it, and that Sync
//! runs afresh.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use prompts::push_ticket::Synced;
use sha2::{Digest as _, Sha256};
use store::ticket::Ticket;

use super::{find_ticket, kept, record, refuse_if_synced, tracker_name};
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

/// A ticket file as it was when its sync started: the SHA-256 of its bytes,
/// in hex. Saved to disk with the kept issue ([`super::kept`]) and compared
/// after a restart, so it is a hash that never changes between builds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fingerprint(String);

impl Fingerprint {
    pub fn of(path: &Path) -> Result<Self, UiError> {
        Ok(Self::of_bytes(&fs::read(path)?))
    }

    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self(format!("{:x}", Sha256::digest(bytes)))
    }

    /// A fingerprint read back from disk. Not checked: one that is not a
    /// hash this app made just never matches a ticket.
    pub fn from_hex(hex: String) -> Self {
        Self(hex)
    }

    pub fn as_hex(&self) -> &str {
        &self.0
    }
}

/// Where a kept issue is filed: the ticket's meeting and number. Ticket
/// numbers are only unique within one meeting plus the shared folder, so two
/// meetings can both have a `TICK-0001`.
pub(super) type Key = (Option<String>, String);

pub(super) fn key(ticket_id: &str, meeting_id: Option<&str>) -> Key {
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

/// One kept issue.
#[derive(Debug, Clone)]
struct Kept {
    created: Created,
    /// Read back from the file an earlier run of the app wrote, and not yet
    /// checked against its ticket.
    from_disk: bool,
}

/// What [`Unsaved`] holds: the kept issues, and what the file under the
/// root has not caught up with yet.
#[derive(Debug, Default)]
struct State {
    kept: HashMap<Key, Kept>,
    /// Issues saved or dismissed since the file was last written, so reading
    /// the file again does not bring them back.
    forgotten: HashSet<Key>,
    /// `kept` has changes the file does not have.
    dirty: bool,
}

impl State {
    fn keep(&mut self, key: Key, created: Created) {
        self.forgotten.remove(&key);
        let from_disk = false;
        self.kept.insert(key, Kept { created, from_disk });
        self.dirty = true;
    }

    /// Also when memory does not hold it: the file may, if it has not been
    /// read since the app started.
    fn forget(&mut self, key: &Key) {
        self.kept.remove(key);
        self.forgotten.insert(key.clone());
        self.dirty = true;
    }

    /// Brings `self` and the file under the root as it is now in line: adds
    /// what the file kept that memory does not know yet, then writes memory
    /// back if the file is behind. Through the gate, so nothing is written
    /// into a folder that is moving. A refused or failed write leaves `dirty`
    /// set, and the next call tries again. Returns the root it used.
    fn sync_disk(
        &mut self,
        gate: Option<&FolderGate>,
        root: &impl Fn() -> Result<PathBuf, UiError>,
    ) -> Option<PathBuf> {
        let synced = writing_in(gate, root, |root| {
            for (key, created) in kept::load(root) {
                if !self.forgotten.contains(&key) {
                    let from_disk = true;
                    self.kept.entry(key).or_insert(Kept { created, from_disk });
                }
            }
            if self.dirty {
                kept::store(
                    root,
                    self.kept.iter().map(|(key, kept)| (key, &kept.created)),
                )?;
                self.forgotten.clear();
                self.dirty = false;
            }
            Ok::<_, UiError>(root.to_owned())
        });
        synced
            .inspect_err(|error| {
                tracing::warn!(
                    "kept Sync issues not written to {}: {}",
                    kept::FILE,
                    error.message
                );
            })
            .ok()
    }
}

/// Issues created but not yet written to their ticket.
#[derive(Debug, Default)]
pub(crate) struct Unsaved(Mutex<State>);

impl Unsaved {
    fn lock(&self) -> MutexGuard<'_, State> {
        // Plain values, each written whole, so a poisoned lock holds nothing
        // half-done.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The issue kept for this ticket in memory, if any.
    #[cfg(test)]
    pub fn get(&self, ticket_id: &str, meeting_id: Option<&str>) -> Option<Created> {
        let kept = self.lock().kept.get(&key(ticket_id, meeting_id)).cloned();
        kept.map(|kept| kept.created)
    }

    /// The issue kept for this ticket, if any. While there is one, a Sync of
    /// the ticket saves it and never runs the agent.
    ///
    /// Reads the file under the root first, for issues an earlier run of the
    /// app kept. Such an issue is reused only if its ticket's file is still
    /// the one its sync started from. If the file changed (a notes re-run
    /// gave the number to another task, an edit, or the link was saved but
    /// the app quit before it could forget the issue), the issue is dropped,
    /// with its address in the log, and this Sync runs afresh. A ticket that
    /// is missing for now keeps it: the save then says so.
    pub fn kept(
        &self,
        gate: Option<&FolderGate>,
        root: &impl Fn() -> Result<PathBuf, UiError>,
        ticket_id: &str,
        meeting_id: Option<&str>,
    ) -> Option<Created> {
        let key = key(ticket_id, meeting_id);
        let mut state = self.lock();
        let now = state.sync_disk(gate, root);
        let found = state.kept.get(&key)?.clone();
        if !found.from_disk {
            return Some(found.created);
        }
        let ticket = now
            .and_then(|root| find_ticket(&root, ticket_id, meeting_id).ok())
            .and_then(|path| fs::read(path).ok());
        match ticket {
            Some(bytes) if Fingerprint::of_bytes(&bytes) != found.created.ticket => {
                tracing::warn!(
                    "{ticket_id} changed since the app kept its issue {}; syncing it afresh",
                    found.created.synced.external_url
                );
                state.forget(&key);
                state.sync_disk(gate, root);
                None
            }
            Some(_) => {
                state
                    .kept
                    .entry(key)
                    .and_modify(|kept| kept.from_disk = false);
                Some(found.created)
            }
            None => Some(found.created),
        }
    }

    /// Forgets the issue kept for this ticket: the user has its link and
    /// asked for the next Sync to be a fresh one.
    pub fn dismiss(
        &self,
        gate: Option<&FolderGate>,
        root: &impl Fn() -> Result<PathBuf, UiError>,
        ticket_id: &str,
        meeting_id: Option<&str>,
    ) {
        let mut state = self.lock();
        state.forget(&key(ticket_id, meeting_id));
        state.sync_disk(gate, root);
    }

    /// Writes the kept issues to the file under the root, if it is behind.
    /// For the end of a folder move, which refuses every other write while it
    /// runs: it holds the gate itself, so this takes none.
    pub fn flush(&self, root: &impl Fn() -> Result<PathBuf, UiError>) {
        self.lock().sync_disk(None, root);
    }

    /// Writes `created` to the ticket's file under the root as it is now, if
    /// the ticket is still the task it was made for and not synced already.
    /// Otherwise the issue is kept, and the error shows where it is.
    pub fn save(
        &self,
        gate: Option<&FolderGate>,
        root: &impl Fn() -> Result<PathBuf, UiError>,
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
        let mut state = self.lock();
        let result = match saved {
            Ok(summary) => {
                state.forget(&key);
                Ok(summary)
            }
            Err(refusal) => {
                let error = match refusal {
                    Refusal::NotSaved(error) => not_saved(&created, &error),
                    Refusal::NotAttached(error) => not_attached(&created, &error),
                };
                state.keep(key, created);
                Err(error)
            }
        };
        state.sync_disk(gate, root);
        result
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
