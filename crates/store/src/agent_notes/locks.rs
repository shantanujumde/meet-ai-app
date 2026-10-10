//! The `meeting.md` lock, apart from the ticket-number lock (TUR-166).
//!
//! Both used to be [`super::lock_meeting_writers`], so a rename or the notes
//! switch waited for any ticket being numbered anywhere. Lock order: a writer
//! that needs both takes [`super::lock_meeting_writers`] first.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// Held by every writer [`lock_meeting_md`] lists. Process-wide, like the
/// ticket-number lock.
static MEETING_MD: Mutex<()> = Mutex::new(());

/// The lock every app writer of a `meeting.md` holds for its whole read and
/// write, so none lands in the middle of another: a notes run
/// ([`super::write()`]), a discarded suggestion
/// ([`crate::suggested::discard`]), the calendar's title
/// ([`crate::meeting_event::apply`]), a rename
/// ([`crate::meeting_title::set_by_user`]) and the notes switch
/// ([`crate::notes_switch::set`]).
///
/// The guard protects no data, so a panic elsewhere leaves nothing torn and a
/// poisoned lock is taken as is.
pub fn lock_meeting_md() -> MutexGuard<'static, ()> {
    MEETING_MD.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    /// TUR-166: a rename does not wait for a ticket being numbered.
    #[test]
    fn a_rename_does_not_wait_for_the_ticket_number_lock() {
        let root = tempfile::tempdir().expect("tempdir");
        let id = "2026-10-01-1000-sync";
        std::fs::create_dir_all(root.path().join(id)).expect("meeting dir");
        let (done, finished) = std::sync::mpsc::channel();
        std::thread::scope(|s| {
            let numbers = super::super::lock_meeting_writers();
            s.spawn(|| {
                done.send(crate::meeting_title::set_by_user(
                    root.path(),
                    id,
                    "Renamed",
                ))
                .ok();
            });
            let renamed = finished.recv_timeout(Duration::from_secs(10));
            drop(numbers);
            assert_eq!(
                renamed
                    .expect("renamed while numbers were held")
                    .expect("rename"),
                "Renamed"
            );
        });
    }
}
