//! Placing a line on the recording's clock (SPEC §3.4, TUR-164).
//!
//! Engines know where a line starts in its track's WAV. The transcript wants
//! when it was said, which is that position mapped through `segments.json`
//! ([`crate::segments::place`]): across the unpadded gaps at device switches
//! and pauses, and through the anchors that correct each device clock's
//! drift. Both paths do it with the same function over the same data:
//!
//! * **Batch** ([`crate::transcribe_meeting`]) reads the finished file and
//!   hands the engine a [`PlacingSink`], so every
//!   [`TranscriptSink::write_at`] is placed before it is stored.
//! * **Live** ([`SessionOptions::with_timeline`]) reads the copy the recorder
//!   publishes each time it writes the file, and the session's emitter places
//!   each line ([`place_live`]) before the pane or the file sees it.
//!
//! With no `segments.json` (a recording made before it existed, or a test
//! with no recorder), a line stays at its WAV position.

use meeting_format::segments::LiveSegments;

use crate::segments::{Segments, SegmentsTimeline, place};
use crate::sink::TranscriptSink;
use crate::{Channel, Error, Speaker, Utterance};

/// The track a speaker is recorded on (L5).
fn channel_of(speaker: Speaker) -> Channel {
    match speaker {
        Speaker::You => Channel::Mic,
        Speaker::Others => Channel::System,
    }
}

/// Seconds since the recording started for a line `wav_sec` into `speaker`'s
/// WAV, by the newest `segments.json` the recorder published, or the WAV
/// position before it has published one.
///
/// Live, a line past the open segment's last counted frame is normal (the
/// count is brought up to date at each checkpoint), so it is placed from that
/// segment without the warning the batch path gives.
pub(crate) fn place_live(timeline: Option<&LiveSegments>, speaker: Speaker, wav_sec: f64) -> f64 {
    let wav_sec = wav_sec.max(0.0);
    let frame = wav_sec * f64::from(meeting_format::SAMPLE_RATE);
    timeline
        .and_then(|timeline| timeline.with(|segments| place(segments, channel_of(speaker), frame)))
        .flatten()
        .map_or(wav_sec, |placed| placed.sec)
}

impl crate::session::SessionOptions {
    /// Place this session's lines by the recording's `segments.json` as the
    /// recorder writes it (SPEC §3.4, TUR-164), so a live line and a batch
    /// re-run of the same meeting agree on when it was said. `timeline` is
    /// the recorder's shared copy, `audio::tee::TeeFeed::timeline`.
    pub fn with_timeline(mut self, timeline: LiveSegments) -> Self {
        self.timeline = Some(timeline);
        self
    }
}

/// A sink that places each line by a finished recording's `segments.json`
/// before passing it on: what the batch path hands an engine.
pub(crate) struct PlacingSink<'a> {
    pub inner: &'a mut dyn TranscriptSink,
    /// `None` when the meeting has no readable `segments.json`.
    pub segments: Option<&'a Segments>,
    pub channel: Channel,
}

impl TranscriptSink for PlacingSink<'_> {
    /// Already placed: passed on as it is.
    fn write(&mut self, utterance: &Utterance) -> Result<(), Error> {
        self.inner.write(utterance)
    }

    fn write_at(&mut self, wav_sec: f64, speaker: Speaker, text: String) -> Result<(), Error> {
        let wav_sec = wav_sec.max(0.0);
        let sec = self
            .segments
            .and_then(|segments| segments.wav_sec_to_sec(self.channel, wav_sec))
            .unwrap_or(wav_sec);
        self.inner.write(&Utterance {
            // Truncated, like a live line: rounding would put an utterance a
            // fraction before its own audio.
            start_sec: sec.max(0.0) as u64,
            speaker,
            text,
        })
    }

    fn flush(&mut self) -> Result<(), Error> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests;
