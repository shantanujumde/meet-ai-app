//! Durable writes for settled lines, and the one sort at the end.

use std::path::Path;

use store::transcript_order::{self, Sorted};
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

/// Put the finished `transcript.md` in time order (SPEC A19).
///
/// Lines land in the order they settle, and the two tracks settle at their
/// own pace, so the file can go back in time where one track overtook the
/// other. Call once every session has finished and the sink is dropped:
/// nothing may append between the read and the rename. A failure is logged
/// and leaves the file as written; every line is still in it.
pub(super) fn sort_by_time(transcript: &Path) {
    match transcript_order::sort_by_time(transcript) {
        Ok(Sorted::Rewritten) => tracing::info!("transcript.md put in time order"),
        Ok(Sorted::InOrder | Sorted::Missing) => {}
        Ok(Sorted::LeftAlone { unparsed }) => tracing::warn!(
            unparsed,
            "transcript.md has lines that are not transcript lines, so it was left in the order written"
        ),
        Err(error) => tracing::warn!(
            %error,
            "could not put transcript.md in time order; it stays in the order written"
        ),
    }
}
