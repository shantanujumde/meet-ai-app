use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// A monotonic counter shared by every session in one meeting.
///
/// Meeting-global rather than per-session on purpose: the mic and the system
/// track run as two sessions but render into one pane, so their keys have to
/// come out of one sequence or they collide.
#[derive(Debug, Clone, Default)]
pub struct SeqCounter(Arc<AtomicU64>);

impl SeqCounter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Take the next number. Starts at 0 and never repeats within a meeting.
    pub fn next(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed)
    }

    /// How many numbers have been handed out. Test affordance.
    pub fn issued(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}
