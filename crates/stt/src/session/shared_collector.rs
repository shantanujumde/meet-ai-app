//! A sink the live tests can read while a session still holds it.

use std::sync::{Arc, PoisonError};

use crate::sink::TranscriptSink;
use crate::{Error, Utterance};

/// A [`crate::CollectingSink`] that can be read while a session still holds it.
///
/// The live tests need to see what has been finalized *so far*, which a
/// `Box<dyn TranscriptSink>` handed to a session does not allow.
#[derive(Debug, Clone, Default)]
pub struct SharedCollector(Arc<std::sync::Mutex<Vec<Utterance>>>);

impl SharedCollector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn utterances(&self) -> Vec<Utterance> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn len(&self) -> usize {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn lines(&self) -> Vec<String> {
        self.utterances()
            .iter()
            .map(crate::format_transcript_line)
            .collect()
    }
}

impl TranscriptSink for SharedCollector {
    fn write(&mut self, utterance: &Utterance) -> Result<(), Error> {
        if utterance.text.trim().is_empty() {
            return Ok(());
        }
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(utterance.clone());
        Ok(())
    }
}
