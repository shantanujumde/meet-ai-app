//! The live transcription seam: an object that exists *while* a meeting runs.
//!
//! [`crate::SttEngine::transcribe`] takes a finished WAV. That is the right
//! shape for `transcript.md`, which SPEC §3.4 makes append-only and therefore
//! has to be correct the first time, but it is the wrong shape for the Phase 2
//! live pane — there is nothing to hold on to while audio is still arriving.
//! [`SttSession`] is that missing object:
//!
//! ```no_run
//! # use stt::{Speaker, SttEngine, session::{SessionOptions, NoListener}};
//! # fn demo(engine: &mut dyn SttEngine, samples: &[i16]) -> Result<(), stt::Error> {
//! let mut session = engine.start_session(
//!     SessionOptions::new(Speaker::You),
//!     Box::new(stt::CollectingSink::new()),
//!     Box::new(NoListener),
//! )?;
//! session.feed(samples)?;          // 16 kHz mono i16, from the live tap
//! let outcome = session.finish()?;
//! # let _ = outcome;
//! # Ok(())
//! # }
//! ```
//!
//! # The two channels, and why there are two
//!
//! SPEC §2.5 splits the output in half and the split is load-bearing:
//!
//! * **Finalized** utterances go to the [`crate::TranscriptSink`], which owns
//!   the line format and the append. They reach disk.
//! * **Volatile** hypotheses go to the [`LiveListener`] and **nowhere else**.
//!   They live in UI memory, they are replaced constantly, and they are thrown
//!   away if they never finalize. Promoting one to disk would mean rewriting a
//!   line in an append-only file.
//!
//! The listener also sees the finals, because it has to: a final is what tells
//! the pane to clear that speaker's volatile tail. So the listener is the
//! *whole* live channel and the sink is the *durable* half of it — the sink
//! never sees a volatile, which is the invariant that matters.
//!
//! # The tail contract
//!
//! Agreed with Nia for the Phase 2 pane, and enforced by [`LiveEmitter`]:
//!
//! 1. Every update carries its [`Speaker`], so the UI never has to know which
//!    engine or subprocess a line came from.
//! 2. **At most one live hypothesis per speaker.** Each [`LiveUpdate::Volatile`]
//!    replaces that speaker's previous one; a [`LiveUpdate::Final`] clears the
//!    tail and appends a settled line; a [`LiveUpdate::Dropped`] clears the tail
//!    without appending anything.
//! 3. Volatiles are coalesced in Rust to [`SessionOptions::volatile_per_sec`]
//!    (~5/sec by default), so the UI is not paying an IPC hop per frame nobody
//!    would see.
//! 4. Every update carries a `seq` from a counter shared across the whole
//!    meeting — a stable React key that survives a webview reload, and a way to
//!    discard an update that arrives out of order.
//!
//! # The silence gate, live
//!
//! Thirty seconds of quiet must produce zero finalized lines *and* leave no
//! stale tail on screen. Zero lines is the same guarantee as the batch path and
//! comes from the same place ([`crate::vad`]). The tail is the new half: if a
//! hypothesis was shown and never finalized, [`SttSession::finish`] emits
//! [`LiveUpdate::Dropped`] so the pane clears it. A session that just stops
//! talking leaves the last thing it guessed on screen forever, which is the
//! streaming-shaped version of the hallucination bug.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::sink::TranscriptSink;
use crate::vad::{
    EarshotVad, FRAME_SAMPLES, SAMPLE_RATE, SegmentConfig, Segmenter, SpeechSpan, Vad, pad_span,
};
use crate::{Error, Speaker, Utterance, collapse_whitespace};

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

/// One line on its way to the live pane.
///
/// `start_sec` is fractional here, unlike [`Utterance::start_sec`]. The pane can
/// show sub-second placement while the `[HH:MM:SS]` line format cannot, and
/// truncating early would throw the precision away before anyone could use it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LiveLine {
    /// Meeting-global, monotonic. Stable React key.
    pub seq: u64,
    pub speaker: Speaker,
    /// Seconds from the start of the recording.
    pub start_sec: f64,
    /// Already whitespace-collapsed and known non-empty.
    pub text: String,
}

impl LiveLine {
    /// The durable form of this line, for a [`TranscriptSink`].
    ///
    /// Only ever called on a final — a volatile has no durable form.
    pub fn to_utterance(&self) -> Utterance {
        Utterance {
            // Truncating to whole seconds matches the [HH:MM:SS] line format;
            // rounding would put an utterance a fraction before its own audio.
            start_sec: self.start_sec.max(0.0) as u64,
            speaker: self.speaker,
            text: self.text.clone(),
        }
    }
}

/// What the live pane is told.
///
/// Tagged for serde so it can go straight over a Tauri event channel without a
/// second translation layer inventing its own names.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LiveUpdate {
    /// Replace this speaker's live hypothesis with this line.
    Volatile(LiveLine),
    /// This speaker's tail is settled: clear it and append this line. The same
    /// line — minus `seq` and the fractional second — also reached the sink.
    Final(LiveLine),
    /// Clear this speaker's tail without appending anything. Sent when a
    /// hypothesis is withdrawn and at [`SttSession::finish`] time, so a guess
    /// that never finalized cannot sit on screen for the rest of the meeting.
    Dropped { speaker: Speaker, seq: u64 },
}

impl LiveUpdate {
    pub fn speaker(&self) -> Speaker {
        match self {
            LiveUpdate::Volatile(line) | LiveUpdate::Final(line) => line.speaker,
            LiveUpdate::Dropped { speaker, .. } => *speaker,
        }
    }

    pub fn seq(&self) -> u64 {
        match self {
            LiveUpdate::Volatile(line) | LiveUpdate::Final(line) => line.seq,
            LiveUpdate::Dropped { seq, .. } => *seq,
        }
    }
}

/// Where live updates go.
///
/// The Phase 2 implementation forwards to a Tauri event channel. Nothing in
/// this crate knows that, and nothing here may block: the listener is called
/// from whatever thread is feeding audio.
pub trait LiveListener: Send {
    fn on_update(&mut self, update: &LiveUpdate);
}

/// Any closure is a listener, so callers rarely need a named type.
impl<F: FnMut(&LiveUpdate) + Send> LiveListener for F {
    fn on_update(&mut self, update: &LiveUpdate) {
        self(update)
    }
}

/// Throws every update away. For batch-shaped callers and tests.
pub struct NoListener;

impl LiveListener for NoListener {
    fn on_update(&mut self, _update: &LiveUpdate) {}
}

/// Records every update, shareable so a test can read it mid-session.
#[derive(Debug, Clone, Default)]
pub struct CollectingListener(Arc<std::sync::Mutex<Vec<LiveUpdate>>>);

impl CollectingListener {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn updates(&self) -> Vec<LiveUpdate> {
        self.0.lock().expect("listener mutex").clone()
    }

    /// Only the volatile hypotheses, in order.
    pub fn volatiles(&self) -> Vec<LiveLine> {
        self.updates()
            .into_iter()
            .filter_map(|update| match update {
                LiveUpdate::Volatile(line) => Some(line),
                _ => None,
            })
            .collect()
    }

    /// Only the settled lines, in order.
    pub fn finals(&self) -> Vec<LiveLine> {
        self.updates()
            .into_iter()
            .filter_map(|update| match update {
                LiveUpdate::Final(line) => Some(line),
                _ => None,
            })
            .collect()
    }

    /// Is a volatile tail still showing for `speaker` after everything seen so
    /// far? This is the assertion the live silence gate is made of.
    pub fn tail_for(&self, speaker: Speaker) -> Option<String> {
        let mut tail = None;
        for update in self.updates() {
            if update.speaker() != speaker {
                continue;
            }
            match update {
                LiveUpdate::Volatile(line) => tail = Some(line.text),
                LiveUpdate::Final(_) | LiveUpdate::Dropped { .. } => tail = None,
            }
        }
        tail
    }
}

impl LiveListener for CollectingListener {
    fn on_update(&mut self, update: &LiveUpdate) {
        self.0.lock().expect("listener mutex").push(update.clone());
    }
}

/// A [`TranscriptSink`] that two live sessions can write to at once.
///
/// One meeting is two tracks and therefore two sessions, but one
/// `transcript.md`. Wrap the real sink once and hand each session a clone.
#[derive(Clone)]
pub struct SharedSink(Arc<std::sync::Mutex<dyn TranscriptSink + Send>>);

impl SharedSink {
    pub fn new<S: TranscriptSink + Send + 'static>(sink: S) -> Self {
        Self(Arc::new(std::sync::Mutex::new(sink)))
    }
}

impl TranscriptSink for SharedSink {
    fn write(&mut self, utterance: &Utterance) -> Result<(), Error> {
        self.0.lock().expect("sink mutex").write(utterance)
    }

    fn flush(&mut self) -> Result<(), Error> {
        self.0.lock().expect("sink mutex").flush()
    }
}

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
        self.0.lock().expect("collector mutex").clone()
    }

    pub fn len(&self) -> usize {
        self.0.lock().expect("collector mutex").len()
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
            .expect("collector mutex")
            .push(utterance.clone());
        Ok(())
    }
}

/// How a live session behaves. Everything here is engine-independent.
#[derive(Debug, Clone)]
pub struct SessionOptions {
    /// Which track this session is transcribing. Fixed for its whole life:
    /// one track is one speaker (L5).
    pub speaker: Speaker,
    /// Shared across both tracks of one meeting, so `seq` is meeting-global.
    /// Cloned from the same [`SeqCounter`] for both sessions.
    pub seq: SeqCounter,
    /// Ceiling on volatile updates per second, per speaker. Excess updates are
    /// coalesced — the newest hypothesis is kept and the ones it superseded are
    /// dropped, so the tail is never stale, only less twitchy.
    pub volatile_per_sec: f64,
}

impl SessionOptions {
    /// Defaults from the agreed contract: ~5 volatile updates a second, and a
    /// fresh per-meeting sequence.
    pub fn new(speaker: Speaker) -> Self {
        Self {
            speaker,
            seq: SeqCounter::new(),
            volatile_per_sec: 5.0,
        }
    }

    /// Both tracks of one meeting, sharing one sequence.
    pub fn pair() -> (Self, Self) {
        let seq = SeqCounter::new();
        (
            Self {
                speaker: Speaker::You,
                seq: seq.clone(),
                volatile_per_sec: 5.0,
            },
            Self {
                speaker: Speaker::Others,
                seq,
                volatile_per_sec: 5.0,
            },
        )
    }

    pub fn with_seq(mut self, seq: SeqCounter) -> Self {
        self.seq = seq;
        self
    }

    pub fn with_volatile_per_sec(mut self, rate: f64) -> Self {
        self.volatile_per_sec = rate;
        self
    }

    fn min_volatile_interval(&self) -> Duration {
        if self.volatile_per_sec <= 0.0 {
            // Zero or negative means "no volatile tail at all"; the emitter
            // reads that off `volatile_per_sec` directly, so this is only a
            // guard against dividing by zero.
            return Duration::MAX;
        }
        Duration::from_secs_f64(1.0 / self.volatile_per_sec)
    }
}

impl From<Speaker> for SessionOptions {
    fn from(speaker: Speaker) -> Self {
        Self::new(speaker)
    }
}

/// What a live session produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOutcome {
    pub speaker: Speaker,
    /// Utterances written to the sink. The silence gate asserts this is 0.
    pub finalized: usize,
    /// Did a volatile hypothesis exist at `finish()` and get thrown away?
    /// True is normal — someone was mid-word when the meeting ended. It is
    /// recorded so the gate can assert the pane was told to clear it.
    pub discarded_volatile: bool,
    /// Whole seconds of audio fed in.
    pub audio_sec: u64,
    pub engine: &'static str,
}

/// A transcription that is still running.
///
/// Sessions are single-threaded from the caller's point of view: [`Self::feed`]
/// must be called in audio order and never concurrently with itself. They are
/// `Send` so the tap can own one on its own thread.
pub trait SttSession: Send {
    /// Which engine is behind this session.
    fn engine_name(&self) -> &'static str;

    /// Hand over the next block of 16 kHz mono PCM from the live tap.
    ///
    /// Blocks for as long as the engine needs, so the caller must not be
    /// holding the audio callback's lock. Any size is accepted, including
    /// sizes that do not divide a VAD frame.
    fn feed(&mut self, samples: &[i16]) -> Result<(), Error>;

    /// Stop, finalize what can be finalized, and throw away what cannot.
    ///
    /// A volatile hypothesis that never finalized is **discarded, never
    /// promoted** — it is a guess, and the append-only file has no way to take
    /// a line back. The pane is told with [`LiveUpdate::Dropped`].
    fn finish(self: Box<Self>) -> Result<SessionOutcome, Error>;
}

/// Owns the tail contract so no engine has to re-implement it.
///
/// Every session drives one of these. It holds the listener, the sequence, the
/// coalescing clock, and the one-hypothesis-per-speaker rule, which is why
/// those four things behave identically on the whisper path, the Apple path and
/// the replay path.
pub struct LiveEmitter {
    speaker: Speaker,
    seq: SeqCounter,
    listener: Box<dyn LiveListener>,
    /// Zero or less means the caller does not want a volatile tail at all.
    volatile_per_sec: f64,
    min_interval: Duration,
    /// The newest hypothesis, not yet delivered because of the rate cap.
    pending: Option<(f64, String)>,
    /// The text last actually delivered, so an unchanged hypothesis costs
    /// nothing. `None` means no tail is showing.
    showing: Option<String>,
    last_sent_at: Option<Instant>,
    finalized: usize,
}

impl LiveEmitter {
    pub fn new(options: &SessionOptions, listener: Box<dyn LiveListener>) -> Self {
        Self {
            speaker: options.speaker,
            seq: options.seq.clone(),
            listener,
            volatile_per_sec: options.volatile_per_sec,
            min_interval: options.min_volatile_interval(),
            pending: None,
            showing: None,
            last_sent_at: None,
            finalized: 0,
        }
    }

    pub fn speaker(&self) -> Speaker {
        self.speaker
    }

    pub fn finalized(&self) -> usize {
        self.finalized
    }

    /// Is a hypothesis currently on screen, or waiting to be?
    pub fn has_tail(&self) -> bool {
        self.showing.is_some() || self.pending.is_some()
    }

    /// Would a hypothesis offered right now actually be delivered?
    ///
    /// Engines ask before *producing* one. On the whisper path a hypothesis
    /// costs an inference pass over the open span, and paying for one the rate
    /// cap is about to swallow is the difference between a live pane and a hot
    /// laptop. Answers `false` when the tail is switched off entirely.
    pub fn wants_volatile(&self) -> bool {
        if self.volatile_per_sec <= 0.0 {
            return false;
        }
        match self.last_sent_at {
            None => true,
            Some(last) => Instant::now().duration_since(last) >= self.min_interval,
        }
    }

    /// Offer a new hypothesis for this speaker. Coalesced, never queued.
    ///
    /// Whitespace-only text is treated as "no hypothesis" and withdraws the
    /// tail rather than showing a blank line.
    pub fn volatile(&mut self, start_sec: f64, raw: &str) {
        if self.volatile_per_sec <= 0.0 {
            return;
        }
        let Some(text) = collapse_whitespace(raw) else {
            self.withdraw();
            return;
        };
        // Replacing a hypothesis with the same hypothesis is not an update.
        if self.showing.as_deref() == Some(text.as_str()) {
            self.pending = None;
            return;
        }
        self.pending = Some((start_sec, text));
        self.poll();
    }

    /// Deliver the pending hypothesis if the rate cap now allows it.
    ///
    /// Sessions call this on every `feed`, so a hypothesis held back by the cap
    /// is delivered as soon as it is allowed rather than waiting for the next
    /// one to arrive.
    pub fn poll(&mut self) {
        if self.pending.is_none() {
            return;
        }
        let now = Instant::now();
        if let Some(last) = self.last_sent_at
            && now.duration_since(last) < self.min_interval
        {
            return;
        }
        let (start_sec, text) = self.pending.take().expect("checked just above");
        self.last_sent_at = Some(now);
        self.showing = Some(text.clone());
        let update = LiveUpdate::Volatile(LiveLine {
            seq: self.seq.next(),
            speaker: self.speaker,
            start_sec,
            text,
        });
        self.listener.on_update(&update);
    }

    /// Settle a line: clear the tail, tell the pane, and write to the sink.
    ///
    /// Returns `false` if the text collapsed to nothing, in which case nothing
    /// was emitted and nothing was written — the last line of defence against
    /// an engine finalizing an empty segment.
    pub fn finalize(
        &mut self,
        start_sec: f64,
        raw: &str,
        sink: &mut dyn TranscriptSink,
    ) -> Result<bool, Error> {
        let Some(text) = collapse_whitespace(raw) else {
            return Ok(false);
        };

        // The final supersedes any hypothesis, delivered or not. Dropping the
        // pending one silently is correct: the Final update clears the tail.
        self.pending = None;
        self.showing = None;

        let line = LiveLine {
            seq: self.seq.next(),
            speaker: self.speaker,
            start_sec: start_sec.max(0.0),
            text,
        };
        sink.write(&line.to_utterance())?;
        self.finalized += 1;
        self.listener.on_update(&LiveUpdate::Final(line));
        Ok(true)
    }

    /// Take the hypothesis back without settling it.
    ///
    /// Returns whether there was one. This is what keeps a stale tail off the
    /// screen, and it is called from `finish()` on every engine.
    pub fn withdraw(&mut self) -> bool {
        let had_pending = self.pending.take().is_some();
        let was_showing = self.showing.take().is_some();
        if was_showing {
            // Only tell the pane about a tail it was actually shown. A pending
            // hypothesis the rate cap swallowed was never on screen.
            self.listener.on_update(&LiveUpdate::Dropped {
                speaker: self.speaker,
                seq: self.seq.next(),
            });
        }
        had_pending || was_showing
    }
}

/// A chunk of audio the VAD has decided is worth transcribing.
///
/// Already padded, already cut out of the stream, and positioned on the
/// recording's timeline rather than the chunk's.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadySpan {
    /// Seconds from the start of the recording to the first sample.
    pub start_sec: f64,
    pub samples: Vec<i16>,
}

impl ReadySpan {
    pub fn duration_sec(&self) -> f64 {
        self.samples.len() as f64 / SAMPLE_RATE as f64
    }
}

/// Turns a live sample stream into the spans an engine is allowed to see.
///
/// This is [`crate::vad::detect_speech`] for audio that has not finished
/// arriving: the same [`Segmenter`], the same [`SegmentConfig`], the same
/// hallucination guard — silence yields no spans, so a live engine is no more
/// able to invent a line than a batch one is. What it adds is a buffer that
/// remembers only as much audio as a span could still need, so a four-hour
/// meeting does not sit in RAM.
///
/// Every streaming engine drives one. Sharing it is what makes the silence
/// gate a property of the crate rather than of each engine's own care.
pub struct SpanAssembler {
    config: SegmentConfig,
    vad: Box<dyn Vad>,
    segmenter: Segmenter,
    /// Audio still in reach of a span. `start` is `buffer[0]`'s absolute
    /// sample index in the recording.
    buffer: Vec<i16>,
    start: usize,
    /// Absolute end of the last span handed out. Padding may not reach back
    /// past it: that audio is already in a finalized line, and repeating it
    /// would repeat the words.
    released: usize,
    /// Absolute count of samples fed so far.
    fed: usize,
}

impl SpanAssembler {
    pub fn new(config: SegmentConfig, vad: Box<dyn Vad>) -> Self {
        Self {
            config,
            vad,
            segmenter: Segmenter::new(config),
            buffer: Vec::new(),
            start: 0,
            released: 0,
            fed: 0,
        }
    }

    /// The default detector, which is what every engine actually uses.
    pub fn with_default_vad(config: SegmentConfig) -> Self {
        Self::new(config, Box::new(EarshotVad::new()))
    }

    /// Add the next block of 16 kHz mono PCM; get back any spans it settled.
    ///
    /// Usually empty — a span only settles when the speaker stops, or when a
    /// monologue hits [`SegmentConfig::max_speech_frames`].
    pub fn push(&mut self, samples: &[i16]) -> Vec<ReadySpan> {
        self.buffer.extend_from_slice(samples);
        self.fed += samples.len();

        let mut ready = Vec::new();
        loop {
            let frame_start = self.segmenter.frames_scored() * FRAME_SAMPLES;
            // Frames are scored in stream order and never re-scored, so the
            // next one is always at or after the front of the buffer.
            let Some(offset) = frame_start.checked_sub(self.start) else {
                break;
            };
            if offset + FRAME_SAMPLES > self.buffer.len() {
                break;
            }

            let frame = &self.buffer[offset..offset + FRAME_SAMPLES];
            let is_speech = self.vad.score(frame) >= self.config.threshold;
            if let Some(raw) = self.segmenter.push(is_speech) {
                ready.push(self.release(raw));
            }
        }

        self.trim();
        ready
    }

    /// The utterance in progress, if someone is mid-sentence.
    ///
    /// This is what a volatile hypothesis is made of. It is *not* released:
    /// the same audio comes back in the settled span later, because a
    /// hypothesis is a guess and the final pass has to see the whole thing.
    pub fn open(&self) -> Option<ReadySpan> {
        let raw = self.segmenter.open_span()?;
        Some(self.cut(pad_span(raw, &self.config, self.released, self.fed)))
    }

    pub fn has_open(&self) -> bool {
        self.segmenter.open_span().is_some()
    }

    /// End of stream: settle whatever was still open.
    pub fn finish(&mut self) -> Option<ReadySpan> {
        let raw = self.segmenter.finish()?;
        Some(self.release(raw))
    }

    /// Whole seconds of audio fed in.
    pub fn fed_sec(&self) -> u64 {
        self.fed as u64 / SAMPLE_RATE as u64
    }

    /// How much audio is being held. The live-memory assertion reads this.
    pub fn buffered_samples(&self) -> usize {
        self.buffer.len()
    }

    /// Pad a raw span, mark its audio spent, and cut it out.
    fn release(&mut self, raw: SpeechSpan) -> ReadySpan {
        let floor = self.released.max(self.start);
        let span = pad_span(raw, &self.config, floor, self.fed);
        self.released = span.end_sample;
        self.cut(span)
    }

    fn cut(&self, span: SpeechSpan) -> ReadySpan {
        let from = span
            .start_sample
            .saturating_sub(self.start)
            .min(self.buffer.len());
        let to = span
            .end_sample
            .saturating_sub(self.start)
            .clamp(from, self.buffer.len());
        ReadySpan {
            start_sec: span.start_sample as f64 / SAMPLE_RATE as f64,
            samples: self.buffer[from..to].to_vec(),
        }
    }

    /// Drop audio no span can reach any more.
    ///
    /// The earliest sample still in play is the open span's start, or — if
    /// nothing is open — the next frame to be scored, since a span could open
    /// there. Either way it is the context pad that decides how far back the
    /// engine may still look.
    fn trim(&mut self) {
        let pad = self.config.pad_frames * FRAME_SAMPLES;
        let earliest = match self.segmenter.open_span() {
            Some(open) => open.start_sample,
            None => self.segmenter.frames_scored() * FRAME_SAMPLES,
        }
        .saturating_sub(pad);

        let keep_from = earliest.clamp(self.start, self.start + self.buffer.len());
        let drop = keep_from - self.start;
        if drop > 0 {
            self.buffer.drain(..drop);
            self.start = keep_from;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No rate cap at all: `1.0 / INFINITY` is a zero minimum interval, so
    /// every hypothesis is delivered. Tests that are about the *tail* rule use
    /// this so they cannot fail on how fast the machine ran them; the two
    /// tests that are about the cap itself set a real rate.
    const UNCAPPED: f64 = f64::INFINITY;

    fn emitter(listener: CollectingListener, rate: f64) -> LiveEmitter {
        let options = SessionOptions::new(Speaker::You).with_volatile_per_sec(rate);
        LiveEmitter::new(&options, Box::new(listener))
    }

    #[test]
    fn a_volatile_never_reaches_the_sink() {
        let seen = CollectingListener::new();
        let mut emitter = emitter(seen.clone(), UNCAPPED);
        let sink = SharedCollector::new();

        emitter.volatile(1.0, "sessions are");
        emitter.volatile(1.0, "sessions are still");

        assert!(sink.is_empty(), "a volatile must never be persisted");
        assert_eq!(seen.volatiles().len(), 2);
    }

    #[test]
    fn a_final_clears_the_tail_and_is_written_once() {
        let seen = CollectingListener::new();
        let mut emitter = emitter(seen.clone(), UNCAPPED);
        let mut sink = SharedCollector::new();

        emitter.volatile(1.0, "sessions are still");
        emitter
            .finalize(1.0, "Sessions are still in memory.", &mut sink)
            .unwrap();

        assert_eq!(sink.len(), 1);
        assert_eq!(
            sink.lines(),
            ["[00:00:01] You: Sessions are still in memory."]
        );
        assert_eq!(seen.tail_for(Speaker::You), None, "a final clears the tail");
        assert!(!emitter.has_tail());
    }

    #[test]
    fn withdrawing_clears_a_tail_that_was_shown() {
        let seen = CollectingListener::new();
        let mut emitter = emitter(seen.clone(), UNCAPPED);

        emitter.volatile(1.0, "sessions are");
        assert_eq!(seen.tail_for(Speaker::You).as_deref(), Some("sessions are"));

        assert!(emitter.withdraw(), "there was a tail to withdraw");
        assert_eq!(
            seen.tail_for(Speaker::You),
            None,
            "a withdrawn hypothesis must not stay on screen"
        );
        assert!(matches!(
            seen.updates().last(),
            Some(LiveUpdate::Dropped { .. })
        ));
    }

    #[test]
    fn withdrawing_nothing_says_nothing() {
        let seen = CollectingListener::new();
        let mut emitter = emitter(seen.clone(), UNCAPPED);
        assert!(!emitter.withdraw());
        assert!(seen.updates().is_empty(), "no tail, no Dropped update");
    }

    #[test]
    fn the_rate_cap_coalesces_instead_of_queueing() {
        let seen = CollectingListener::new();
        // 1/sec: the first goes straight out, the rest are held.
        let mut emitter = emitter(seen.clone(), 1.0);

        for text in ["s", "se", "ses", "sess", "sessions"] {
            emitter.volatile(1.0, text);
        }

        let volatiles = seen.volatiles();
        assert_eq!(volatiles.len(), 1, "four updates were coalesced away");
        assert_eq!(volatiles[0].text, "s");
        // The newest is pending, not lost: a later poll delivers *it*, not the
        // stale ones in between.
        assert!(emitter.has_tail());
    }

    #[test]
    fn a_coalesced_hypothesis_is_the_newest_one_not_the_oldest() {
        let seen = CollectingListener::new();
        let mut options = SessionOptions::new(Speaker::You);
        options.volatile_per_sec = 1000.0;
        let mut emitter = LiveEmitter::new(&options, Box::new(seen.clone()));

        // Force the cap on after the first delivery.
        emitter.volatile(1.0, "first");
        emitter.min_interval = Duration::from_secs(3600);
        emitter.volatile(1.0, "second");
        emitter.volatile(1.0, "third");
        // Lift it and poll, the way a session does on every feed.
        emitter.min_interval = Duration::ZERO;
        emitter.poll();

        let texts: Vec<_> = seen.volatiles().into_iter().map(|l| l.text).collect();
        assert_eq!(texts, ["first", "third"], "the superseded guess is dropped");
    }

    #[test]
    fn an_unchanged_hypothesis_is_not_an_update() {
        let seen = CollectingListener::new();
        let mut emitter = emitter(seen.clone(), UNCAPPED);

        emitter.volatile(1.0, "sessions are");
        emitter.volatile(1.0, "sessions   are");
        emitter.volatile(1.0, "sessions are\n");

        assert_eq!(
            seen.volatiles().len(),
            1,
            "whitespace-only differences are the same hypothesis"
        );
    }

    #[test]
    fn empty_text_is_never_a_line_and_never_a_tail() {
        let seen = CollectingListener::new();
        let mut emitter = emitter(seen.clone(), UNCAPPED);
        let mut sink = SharedCollector::new();

        emitter.volatile(1.0, "something");
        emitter.volatile(1.0, "   \n ");
        assert_eq!(
            seen.tail_for(Speaker::You),
            None,
            "blank withdraws the tail"
        );

        assert!(!emitter.finalize(1.0, "  ", &mut sink).unwrap());
        assert!(sink.is_empty());
    }

    #[test]
    fn seq_is_meeting_global_across_both_speakers() {
        let seen = CollectingListener::new();
        let (mine, theirs) = SessionOptions::pair();
        let mut mic = LiveEmitter::new(&mine, Box::new(seen.clone()));
        let mut system = LiveEmitter::new(&theirs, Box::new(seen.clone()));
        let mut sink = SharedCollector::new();

        mic.finalize(1.0, "Morning.", &mut sink).unwrap();
        system
            .finalize(2.0, "Morning everyone.", &mut sink)
            .unwrap();
        mic.finalize(3.0, "Shall we start?", &mut sink).unwrap();

        let seqs: Vec<_> = seen.finals().into_iter().map(|line| line.seq).collect();
        assert_eq!(seqs, [0, 1, 2], "two tracks, one sequence, no collisions");
    }

    #[test]
    fn a_rate_of_zero_turns_the_tail_off_entirely() {
        let seen = CollectingListener::new();
        let mut emitter = emitter(seen.clone(), 0.0);
        emitter.volatile(1.0, "not wanted");
        assert!(seen.updates().is_empty());
        assert!(!emitter.has_tail());
    }

    #[test]
    fn the_live_line_serializes_with_its_kind() {
        let json = serde_json::to_string(&LiveUpdate::Volatile(LiveLine {
            seq: 7,
            speaker: Speaker::Others,
            start_sec: 12.5,
            text: "and the API".into(),
        }))
        .unwrap();
        assert!(json.contains("\"kind\":\"volatile\""), "{json}");
        assert!(json.contains("\"speaker\":\"others\""), "{json}");
        assert!(json.contains("\"seq\":7"), "{json}");
    }

    #[test]
    fn a_dropped_update_serializes_for_the_pane() {
        let json = serde_json::to_string(&LiveUpdate::Dropped {
            speaker: Speaker::You,
            seq: 3,
        })
        .unwrap();
        assert_eq!(json, r#"{"kind":"dropped","speaker":"you","seq":3}"#);
    }
}
