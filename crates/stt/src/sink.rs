//! Where finalized text goes.
//!
//! SPEC §2.5 is explicit that every engine normalizes through **one**
//! `TranscriptSink`, which owns the §3.4 line contract, the whitespace
//! collapse, and the append. Engines emit structured [`Utterance`]s; they never
//! format and never write. That is the only reason "engine switch is a config
//! change only" (the Phase 1 exit gate) can be true — the file format simply
//! is not reachable from engine code.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;

use crate::{Error, Speaker, Utterance, format_transcript_line};

/// A destination for finalized utterances.
///
/// Volatile/partial results never come through here — SPEC §2.5 keeps them in
/// UI memory and off the filesystem entirely.
pub trait TranscriptSink {
    /// Record one finalized utterance.
    fn write(&mut self, utterance: &Utterance) -> Result<(), Error>;

    /// Record one finalized utterance that starts `wav_sec` seconds into its
    /// track's WAV (TUR-164).
    ///
    /// Engines call this rather than [`Self::write`]: they know where a line
    /// is in the WAV, not when it was said. The default stores it at that
    /// position, cut to whole seconds. The batch path's sink overrides it to
    /// place the line through `segments.json` first (SPEC §3.4,
    /// [`crate::segments`]).
    fn write_at(&mut self, wav_sec: f64, speaker: Speaker, text: String) -> Result<(), Error> {
        self.write(&Utterance {
            start_sec: wav_sec.max(0.0) as u64,
            speaker,
            text,
        })
    }

    /// Flush anything buffered. Called at least at end of transcription.
    fn flush(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

/// Appends `[HH:MM:SS] Speaker: text` lines to a `transcript.md`.
///
/// Append-only, per SPEC §3.4: a line, once written, is never rewritten or
/// reordered by this sink, and the file is opened in append mode, so what
/// reached the file is never truncated. It is not crash-safe on its own:
/// lines wait in an 8 KB buffer until [`TranscriptSink::flush`], so a crash
/// loses everything since the last flush. Live recording wraps it in
/// `src-tauri`'s `live_transcript::sink::Durable`, which flushes each line to
/// the OS as it is written; only that wrapper makes "a crash mid-meeting
/// leaves a valid prefix" true (TUR-175). Live, the
/// two tracks share one sink and lines land in the order they settle, not the
/// order they were said; the app sorts the file by time once the recording
/// stops (SPEC A19, `store::transcript_order`).
pub struct MarkdownSink {
    writer: BufWriter<File>,
    /// Last timestamp written, to notice a line that goes back in time.
    last_start_sec: Option<u64>,
    written: usize,
}

impl MarkdownSink {
    /// Open (or create) `transcript.md` at `path` for appending.
    pub fn create(path: &Path) -> Result<Self, Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            writer: BufWriter::new(file),
            last_start_sec: None,
            written: 0,
        })
    }

    /// How many lines this sink has written. Used by the silence test, which
    /// asserts this is exactly zero.
    pub fn lines_written(&self) -> usize {
        self.written
    }
}

impl TranscriptSink for MarkdownSink {
    fn write(&mut self, utterance: &Utterance) -> Result<(), Error> {
        // SPEC §3.4: empty text is never written. The engines drop empties too,
        // but this is the last line of defence and it is cheap.
        if utterance.text.trim().is_empty() {
            return Ok(());
        }

        // An append cannot move a line, so one that starts before the last
        // one is written as it arrived, timestamp untouched; refusing it would
        // lose speech. From one engine that is an engine bug. Live, with two
        // tracks on one sink, it is normal (each settles at its own pace), and
        // the file is sorted by time when the recording stops (SPEC A19).
        if let Some(last) = self.last_start_sec
            && utterance.start_sec < last
        {
            tracing::warn!(
                previous = last,
                received = utterance.start_sec,
                "utterance arrived out of order; written in arrival order, sorted when the recording stops"
            );
        }
        self.last_start_sec = Some(utterance.start_sec);

        writeln!(self.writer, "{}", format_transcript_line(utterance))?;
        self.written += 1;
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Error> {
        self.writer.flush()?;
        Ok(())
    }
}

/// Collects utterances in memory instead of writing them.
///
/// Two uses: tests, and the merge step in [`crate::transcribe`], which needs
/// both tracks in hand before it can interleave them by timestamp.
#[derive(Debug, Default)]
pub struct CollectingSink {
    pub utterances: Vec<Utterance>,
}

impl CollectingSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// The rendered lines, for snapshot assertions.
    pub fn lines(&self) -> Vec<String> {
        self.utterances.iter().map(format_transcript_line).collect()
    }
}

impl TranscriptSink for CollectingSink {
    fn write(&mut self, utterance: &Utterance) -> Result<(), Error> {
        if utterance.text.trim().is_empty() {
            return Ok(());
        }
        self.utterances.push(utterance.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utterance(start_sec: u64, text: &str) -> Utterance {
        Utterance {
            start_sec,
            speaker: Speaker::You,
            text: text.into(),
        }
    }

    #[test]
    fn empty_text_never_reaches_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("transcript.md");
        let _ = std::fs::remove_file(&path);

        let mut sink = MarkdownSink::create(&path).unwrap();
        sink.write(&utterance(0, "   ")).unwrap();
        sink.write(&utterance(1, "")).unwrap();
        sink.flush().unwrap();

        assert_eq!(sink.lines_written(), 0);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
    }

    #[test]
    fn lines_are_appended_in_the_spec_format() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("transcript.md");
        let _ = std::fs::remove_file(&path);

        let mut sink = MarkdownSink::create(&path).unwrap();
        sink.write(&utterance(4, "Sessions are the blocker."))
            .unwrap();
        sink.write(&Utterance {
            start_sec: 11,
            speaker: Speaker::Others,
            text: "Right, let's ticket it.".into(),
        })
        .unwrap();
        sink.flush().unwrap();

        let body = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            body,
            "[00:00:04] You: Sessions are the blocker.\n\
             [00:00:11] Others: Right, let's ticket it.\n"
        );
    }
}
