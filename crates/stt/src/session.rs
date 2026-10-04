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
use std::time::{Duration, Instant};

use crate::sink::TranscriptSink;
pub use crate::span_assembler::{ReadySpan, SpanAssembler};
use crate::{Error, Speaker, Utterance, collapse_whitespace};

mod seq_counter;
pub use seq_counter::SeqCounter;

/// One line on its way to the live pane.
///
/// `start_sec` is fractional here, unlike [`Utterance::start_sec`]. The pane can
/// show sub-second placement while the `[HH:MM:SS]` line format cannot, and
/// truncating early would throw the precision away before anyone could use it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct LiveLine {
    /// Meeting-global, monotonic. Stable React key.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub seq: u64,
    pub speaker: Speaker,
    /// Seconds from the start of the recording.
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
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

#[cfg(test)]
mod tests;
