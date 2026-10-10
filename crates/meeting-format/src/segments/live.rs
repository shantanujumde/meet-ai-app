//! `segments.json` while it is still being written (TUR-164).
//!
//! SPEC §3.4 places every transcript line through `segments.json`. The batch
//! path reads the file once the recording is over; the live transcript cannot
//! wait for that, so the recorder also publishes each version it writes here,
//! and the live session reads the newest one to place a line. Both sides then
//! run the same maths over the same data (`stt::segments`).

use std::sync::{Arc, Mutex, PoisonError};

use super::Segments;

/// A shared, in-memory copy of the newest `segments.json`.
///
/// Cheap to clone: every clone is the same copy. Empty until the recorder
/// publishes its first version.
#[derive(Debug, Clone, Default)]
pub struct LiveSegments(Arc<Mutex<Option<Segments>>>);

impl LiveSegments {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the copy with `segments`. Called by the recorder each time it
    /// writes the file, never from an audio callback.
    pub fn publish(&self, segments: Segments) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(segments);
    }

    /// Run `read` over the newest copy, or `None` before the first publish.
    pub fn with<R>(&self, read: impl FnOnce(&Segments) -> R) -> Option<R> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_until_published_then_every_clone_sees_the_newest_copy() {
        let live = LiveSegments::new();
        let reader = live.clone();
        assert_eq!(reader.with(|s| s.segments.len()), None);

        live.publish(Segments {
            version: 1,
            segments: Vec::new(),
        });
        assert_eq!(reader.with(|s| s.segments.len()), Some(0));
    }
}
