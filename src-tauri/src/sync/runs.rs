//! The Sync runs in flight, keyed by meeting and ticket (TUR-154).
//!
//! Ticket numbers are global now, but an older meetings folder, a copied
//! meeting or a pasted agent can still leave two meetings holding the same
//! `TICK-NNNN`. Keying by the pair, like [`super::save::Key`], keeps syncing
//! one from refusing or cancelling the other.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::save::{Key, Unsaved, key};
use crate::cancels::{self, Cancels, Refused};
use crate::error::UiError;
use crate::meetings;

/// The Sync runs in flight, by meeting and ticket id, so the window can
/// cancel one and a second press on the same task is refused instead of
/// making two issues. Also the issues a run created but could not save, so
/// Retry saves them instead of making another (`save.rs`).
#[derive(Debug, Default)]
pub struct SyncRuns {
    running: Cancels<Key>,
    pub(super) unsaved: Unsaved,
    /// "Send a test ticket" checks going (`check.rs`), each press its own
    /// key, so quitting stops their CLIs too (TUR-168).
    checks: Cancels<u64>,
    next_check: AtomicU64,
}

/// One ticket's place in [`SyncRuns`]; dropping it frees the ticket.
pub(super) type Claim<'a> = cancels::Claim<'a, Key>;

impl SyncRuns {
    /// Marks `ticket_id` of `meeting_id` (`None` for a shared ticket) as
    /// syncing until the returned claim is dropped. Refused once the app is
    /// quitting ([`SyncRuns::shutdown`]).
    pub(super) fn claim(
        &self,
        ticket_id: &str,
        meeting_id: Option<&str>,
    ) -> Result<Claim<'_>, UiError> {
        self.running
            .claim(key(ticket_id, meeting_id))
            .map_err(|refused| match refused {
                Refused::Busy => {
                    UiError::app("sync-busy", format!("{ticket_id} is already being synced."))
                }
                Refused::Closed => UiError::app(
                    "app-quitting",
                    format!("meet-ai is quitting, so {ticket_id} was not synced."),
                ),
            })
    }

    /// Writes the kept issues a folder move kept from being written; for the
    /// end of the move, which holds the gate itself (`save.rs`).
    pub fn after_folder_move(&self) {
        self.unsaved.flush(&meetings::root);
    }

    /// Stops the Sync run for `ticket_id` of `meeting_id`, if there is one.
    pub(super) fn cancel(&self, ticket_id: &str, meeting_id: Option<&str>) {
        self.running.cancel(&key(ticket_id, meeting_id));
    }

    /// The (meeting, ticket id) pairs with a Sync run going, for the
    /// retention job (TUR-45). The meeting is `None` for a shared ticket.
    pub fn running_tickets(&self) -> Vec<(Option<String>, String)> {
        self.running.keys()
    }

    /// A place for one tracker check, refused once the app is quitting.
    pub(super) fn claim_check(&self) -> Result<cancels::Claim<'_, u64>, UiError> {
        let key = self.next_check.fetch_add(1, Ordering::Relaxed);
        self.checks.claim(key).map_err(|_refused| {
            UiError::app(
                "app-quitting",
                "meet-ai is quitting, so the test ticket was not sent.",
            )
        })
    }

    /// The app is quitting (TUR-160): start no more Sync runs, cancel every
    /// one going, which kills its CLI's process tree, and wait up to `wait`
    /// for them to end. Without this the CLI outlived the app and still made
    /// the issue, with nobody left to save the link, so the next Sync made a
    /// second one. Tracker checks are stopped the same way (TUR-168).
    pub fn shutdown(&self, wait: Duration) {
        let left = self.running.shutdown(wait);
        if left > 0 {
            tracing::warn!(left, "Sync runs still going at quit");
        }
        let left = self.checks.shutdown(wait);
        if left > 0 {
            tracing::warn!(left, "tracker checks still going at quit");
        }
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

    /// TUR-168: "Send a test ticket" is stopped at quit like a Sync run, and
    /// none starts after.
    #[test]
    fn quitting_stops_tracker_checks_too() {
        let runs = SyncRuns::default();
        let first = runs.claim_check().unwrap();
        let second = runs.claim_check().unwrap();
        std::thread::scope(|scope| {
            for claim in [first, second] {
                scope.spawn(move || {
                    while !claim.cancel.is_cancelled() {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                });
            }
            runs.shutdown(Duration::from_secs(5));
        });
        assert_eq!(runs.claim_check().err().unwrap().kind, "app-quitting");
    }
}
