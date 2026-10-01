//! Durable writes for settled lines.

use stt::{MarkdownSink, TranscriptSink, Utterance};

/// Makes every finalized line durable the moment it settles.
///
/// `MarkdownSink` buffers, and only flushes when a session finishes. Live,
/// that would leave `transcript.md` empty for the whole meeting and lose the
/// buffered tail on a crash — the review view reads the file, not this
/// module's memory. One `write(2)` per settled line is nothing.
pub(super) struct Durable(pub(super) MarkdownSink);

impl TranscriptSink for Durable {
    fn write(&mut self, utterance: &Utterance) -> Result<(), stt::Error> {
        self.0.write(utterance)?;
        self.0.flush()
    }

    fn flush(&mut self) -> Result<(), stt::Error> {
        self.0.flush()
    }
}
