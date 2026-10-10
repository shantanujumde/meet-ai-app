//! The Sync runs in flight, keyed by meeting and ticket (TUR-154).
//!
//! Ticket numbers are global now, but an older meetings folder, a copied
//! meeting or a pasted agent can still leave two meetings holding the same
//! `TICK-NNNN`. Keying by the pair, like [`super::save::Key`], keeps syncing
//! one from refusing or cancelling the other.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use agent::CancelHandle;

use super::save::{Key, Unsaved, key};
use crate::error::UiError;
use crate::meetings;

/// The Sync runs in flight, by meeting and ticket id, so the window can
/// cancel one and a second press on the same task is refused instead of
/// making two issues. Also the issues a run created but could not save, so
/// Retry saves them instead of making another (`save.rs`).
#[derive(Debug, Default)]
pub struct SyncRuns {
    running: Mutex<HashMap<Key, CancelHandle>>,
    pub(super) unsaved: Unsaved,
}

impl SyncRuns {
    fn lock(&self) -> MutexGuard<'_, HashMap<Key, CancelHandle>> {
        // The map holds only cancel flags, so a panic mid-insert leaves
        // nothing half-written worth refusing over.
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Marks `ticket_id` of `meeting_id` (`None` for a shared ticket) as
    /// syncing until the returned claim is dropped.
    pub(super) fn claim(
        &self,
        ticket_id: &str,
        meeting_id: Option<&str>,
    ) -> Result<Claim<'_>, UiError> {
        let mut runs = self.lock();
        let key = key(ticket_id, meeting_id);
        if runs.contains_key(&key) {
            return Err(UiError::app(
                "sync-busy",
                format!("{ticket_id} is already being synced."),
            ));
        }
        let cancel = CancelHandle::new();
        runs.insert(key.clone(), cancel.clone());
        Ok(Claim {
            runs: self,
            key,
            cancel,
        })
    }

    /// Writes the kept issues a folder move kept from being written; for the
    /// end of the move, which holds the gate itself (`save.rs`).
    pub fn after_folder_move(&self) {
        self.unsaved.flush(&meetings::root);
    }

    /// Stops the Sync run for `ticket_id` of `meeting_id`, if there is one.
    pub(super) fn cancel(&self, ticket_id: &str, meeting_id: Option<&str>) {
        if let Some(cancel) = self.lock().get(&key(ticket_id, meeting_id)) {
            cancel.cancel();
        }
    }

    /// The (meeting, ticket id) pairs with a Sync run going, for the
    /// retention job (TUR-45). The meeting is `None` for a shared ticket.
    pub fn running_tickets(&self) -> Vec<(Option<String>, String)> {
        self.lock().keys().cloned().collect()
    }
}

/// One ticket's place in [`SyncRuns`]; dropping it frees the ticket.
pub(super) struct Claim<'a> {
    runs: &'a SyncRuns,
    key: Key,
    pub(super) cancel: CancelHandle,
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        self.runs.lock().remove(&self.key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "2026-10-01-1000-a";
    const B: &str = "2026-10-02-1000-b";

    #[test]
    fn two_meetings_with_the_same_ticket_id_sync_side_by_side() {
        let runs = SyncRuns::default();
        let a = runs.claim("TICK-0001", Some(A)).unwrap();
        let b = runs.claim("TICK-0001", Some(B)).unwrap();
        let shared = runs.claim("TICK-0001", None).unwrap();
        assert_eq!(
            runs.claim("TICK-0001", Some(A)).err().unwrap().kind,
            "sync-busy"
        );

        runs.cancel("TICK-0001", Some(A));
        assert!(a.cancel.is_cancelled());
        assert!(!b.cancel.is_cancelled());
        assert!(!shared.cancel.is_cancelled());

        let mut running = runs.running_tickets();
        running.sort();
        assert_eq!(
            running,
            vec![
                (None, "TICK-0001".to_owned()),
                (Some(A.to_owned()), "TICK-0001".to_owned()),
                (Some(B.to_owned()), "TICK-0001".to_owned()),
            ]
        );
        drop(a);
        assert!(runs.claim("TICK-0001", Some(A)).is_ok());
        assert_eq!(
            runs.claim("TICK-0001", Some(B)).err().unwrap().kind,
            "sync-busy"
        );
    }

    #[test]
    fn an_empty_meeting_id_is_the_shared_folder() {
        let runs = SyncRuns::default();
        let _shared = runs.claim("TICK-0002", None).unwrap();
        assert_eq!(
            runs.claim("TICK-0002", Some("")).err().unwrap().kind,
            "sync-busy"
        );
    }
}
