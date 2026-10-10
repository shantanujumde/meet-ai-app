//! One `transcript.md` sink shared by a meeting's two live sessions.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::sink::TranscriptSink;
use crate::{Error, Utterance};

/// A [`TranscriptSink`] that two live sessions can write to at once.
///
/// One meeting is two tracks and therefore two sessions, but one
/// `transcript.md`. Wrap the real sink once and hand each session a clone.
#[derive(Clone)]
pub struct SharedSink(Arc<Mutex<dyn TranscriptSink + Send>>);

impl SharedSink {
    pub fn new<S: TranscriptSink + Send + 'static>(sink: S) -> Self {
        Self(Arc::new(Mutex::new(sink)))
    }

    /// Drops this handle and says whether it was the last one, so nothing
    /// can write through the sink any more. A session that finished cleanly
    /// has dropped its clone; one that panicked may have left a thread holding
    /// one. The count cannot go up between the check and the drop: a clone
    /// needs a handle, there are no `Weak` ones, and at a count of one the
    /// only handle is this one. (`Arc::into_inner` would say it in one call,
    /// but needs a sized type, and the sink is `dyn`.)
    pub fn release(self) -> bool {
        Arc::strong_count(&self.0) == 1
    }

    /// Take the sink. A panic on one track while it held the lock poisons it;
    /// the other track keeps writing rather than panicking too (TUR-175), as
    /// Parakeet's model lock does. Each `write` hands the sink one whole
    /// line, so a panic costs at most the line it was writing.
    fn lock(&self) -> MutexGuard<'_, dyn TranscriptSink + Send + 'static> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl TranscriptSink for SharedSink {
    fn write(&mut self, utterance: &Utterance) -> Result<(), Error> {
        self.lock().write(utterance)
    }

    fn flush(&mut self) -> Result<(), Error> {
        self.lock().flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Speaker, session::SharedCollector};

    fn line(start_sec: u64, text: &str) -> Utterance {
        Utterance {
            start_sec,
            speaker: Speaker::Others,
            text: text.into(),
        }
    }

    /// A sink that panics on the line that says so, while the lock is held.
    struct PanicsOn(SharedCollector);

    impl TranscriptSink for PanicsOn {
        fn write(&mut self, utterance: &Utterance) -> Result<(), Error> {
            assert!(utterance.text != "boom", "the sink panicked");
            self.0.write(utterance)
        }
    }

    #[test]
    fn a_panic_on_one_track_does_not_stop_the_other() {
        let collected = SharedCollector::new();
        let shared = SharedSink::new(PanicsOn(collected.clone()));

        let mut mic = shared.clone();
        let panicked = std::thread::spawn(move || mic.write(&line(1, "boom")))
            .join()
            .is_err();
        assert!(panicked, "the mic track's write panicked");
        assert!(shared.0.is_poisoned(), "and poisoned the lock");

        let mut system = shared.clone();
        system.write(&line(2, "still here")).unwrap();
        system.flush().unwrap();
        assert_eq!(collected.utterances(), [line(2, "still here")]);
    }
}
