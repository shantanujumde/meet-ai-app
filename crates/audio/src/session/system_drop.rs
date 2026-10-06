//! Dropping the system track from a running recording (TUR-136).
//!
//! The system-audio permission check runs during the recording, on its own
//! thread, while the app's ticker thread owns the [`super::RecordingSession`].
//! So the check cannot call the session; it asks through a [`SystemDrop`],
//! and the next [`super::RecordingSession::tick`] (at most
//! [`super::TICK_INTERVAL`] later) carries it out with
//! [`super::RecordingSession::drop_system`].

use std::sync::{Arc, Mutex, PoisonError};

/// A handle for asking a running session, from another thread, to drop its
/// system track at its next tick. Cheap to clone; every clone asks the same
/// session.
#[derive(Debug, Clone, Default)]
pub struct SystemDrop(Arc<Mutex<Option<String>>>);

impl SystemDrop {
    /// Ask for the drop, naming the segment-boundary `reason`
    /// (`meeting_format::segments::reason`). Asking twice keeps the first
    /// reason: one drop is all a session can do.
    pub fn request(&self, reason: &str) {
        let mut pending = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if pending.is_none() {
            *pending = Some(reason.to_string());
        }
    }

    /// The pending request, if any, cleared as it is read.
    pub fn take(&self) -> Option<String> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_is_taken_once_and_keeps_its_first_reason() {
        let drop = SystemDrop::default();
        assert_eq!(drop.take(), None);
        let asker = drop.clone();
        asker.request("system_audio_denied");
        asker.request("something_else");
        assert_eq!(drop.take().as_deref(), Some("system_audio_denied"));
        assert_eq!(drop.take(), None, "taken means done");
    }
}
