//! A live session with no microphone and no model behind it.
//!
//! The Phase 2 pane needs three things that do not exist yet at the same time:
//! a recorder producing audio, an engine producing text, and a session object
//! joining the two. This module supplies the last two from a file, so the pane
//! can be built, demoed and regression-tested on its own.
//!
//! ```no_run
//! # use std::path::Path;
//! # use stt::{Speaker, SessionOptions};
//! # use stt::replay::{ReplayEngine, ReplayOptions, replay_track};
//! # fn demo() -> Result<(), stt::Error> {
//! let mut engine = ReplayEngine::new();
//! let outcome = replay_track(
//!     Path::new("crates/audio/fixtures/two-speaker-60s/mic.wav"),
//!     &mut engine,
//!     SessionOptions::new(Speaker::You),
//!     Box::new(stt::CollectingSink::new()),
//!     Box::new(|update: &stt::LiveUpdate| println!("{update:?}")),
//!     &ReplayOptions::realtime(),
//! )?;
//! # let _ = outcome;
//! # Ok(())
//! # }
//! ```
//!
//! # What is real and what is not
//!
//! Real: the [`crate::vad`] gate, the [`crate::session::SpanAssembler`] buffer,
//! the [`LiveEmitter`] tail contract, the `seq` counter, the sink writes, and
//! the wall-clock pacing. **Silence produces zero lines here for exactly the
//! reason it does on the whisper path** — the VAD opens no span, so the script
//! is never consulted. That is the point of building the demo engine on the
//! shared machinery instead of on a timer.
//!
//! Not real: the words. A [`ReplayEngine`] does no recognition at all; it reads
//! the next line off a script each time the detector settles an utterance, and
//! grows a prefix of that line as a volatile hypothesis while the speaker is
//! still going. Nobody should read a replay transcript and believe it.

use std::path::Path;
use std::time::{Duration, Instant};

use crate::session::{
    LiveEmitter, LiveListener, NoListener, SessionOptions, SessionOutcome, SharedCollector,
    SpanAssembler, SttSession,
};
use crate::sink::TranscriptSink;
use crate::vad::{SAMPLE_RATE, SegmentConfig};
use crate::{Error, Speaker, SttEngine};

/// What the mic track says in the built-in demo.
///
/// Deliberately meeting-shaped and deliberately dull: the pane is being judged
/// on layout, wrapping and update behaviour, so the text should not be the
/// interesting thing on screen. Long enough to wrap, short enough to read.
pub const DEMO_SCRIPT_YOU: &[&str] = &[
    "Morning — can everyone hear me?",
    "Right, the only thing I want to land today is the live pane.",
    "The session object is in, so there is something to render against now.",
    "Partials never touch disk, so a reload can only ever lose a guess.",
    "Let's pick it up again on Thursday.",
];

/// What the system track says in the built-in demo.
pub const DEMO_SCRIPT_OTHERS: &[&str] = &[
    "Morning, yes, loud and clear.",
    "That works for me — I have the layout mostly done.",
    "Does each speaker get its own line, or do they interleave?",
    "One live line per speaker, replaced in place. Got it.",
    "Thursday is fine.",
];

/// Roughly how fast the demo speaker talks, for growing a hypothesis.
///
/// Only affects how many words of the upcoming line are shown as a volatile
/// while the detector still has the span open. Conversational English sits
/// around 2.5 words a second.
const DEFAULT_WORDS_PER_SEC: f64 = 2.5;

/// A scripted engine that recognizes nothing.
///
/// Implements the full [`SttEngine`] surface, including [`SttEngine::transcribe`],
/// so it can stand in anywhere a real engine goes — a demo build, a UI test, or
/// a machine with no model downloaded.
#[derive(Debug, Clone)]
pub struct ReplayEngine {
    you: Vec<String>,
    others: Vec<String>,
    segmentation: SegmentConfig,
    words_per_sec: f64,
}

impl Default for ReplayEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ReplayEngine {
    /// Not in [`crate::registry::Kind`] on purpose: this engine is a
    /// development tool, and `config.jsonc` must not be able to select
    /// something that invents a transcript.
    pub const NAME: &'static str = "replay";

    /// The built-in two-sided demo script.
    pub fn new() -> Self {
        Self {
            you: DEMO_SCRIPT_YOU.iter().map(|s| s.to_string()).collect(),
            others: DEMO_SCRIPT_OTHERS.iter().map(|s| s.to_string()).collect(),
            segmentation: SegmentConfig::default(),
            words_per_sec: DEFAULT_WORDS_PER_SEC,
        }
    }

    /// Say something else on one track. An empty script leaves it unchanged,
    /// because a session with nothing to say would look identical to a broken
    /// detector.
    pub fn with_script<I, S>(mut self, speaker: Speaker, lines: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let lines: Vec<String> = lines.into_iter().map(Into::into).collect();
        if lines.is_empty() {
            return self;
        }
        match speaker {
            Speaker::You => self.you = lines,
            Speaker::Others => self.others = lines,
        }
        self
    }

    pub fn with_segmentation(mut self, segmentation: SegmentConfig) -> Self {
        self.segmentation = segmentation;
        self
    }

    fn script_for(&self, speaker: Speaker) -> Vec<String> {
        match speaker {
            Speaker::You => self.you.clone(),
            Speaker::Others => self.others.clone(),
        }
    }
}

impl SttEngine for ReplayEngine {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn transcribe(
        &mut self,
        wav: &Path,
        speaker: Speaker,
        sink: &mut dyn TranscriptSink,
    ) -> Result<(), Error> {
        // Batch is live with the clock removed, which is a useful thing to be
        // able to say out loud: if these two disagreed, the live pane and the
        // saved transcript would disagree too.
        let pcm = crate::read_wav_16k_mono(wav)?;
        let collected = SharedCollector::new();
        let mut session = ReplaySession::new(
            self,
            // No tail: a batch caller has no pane to put one in, and this is
            // the flag an engine reads to skip producing hypotheses at all.
            SessionOptions::new(speaker).with_volatile_per_sec(0.0),
            Box::new(collected.clone()),
            Box::new(NoListener),
        );
        replay_samples(&pcm, &mut session, &ReplayOptions::instant())?;
        Box::new(session).finish()?;

        for utterance in collected.utterances() {
            sink.write(&utterance)?;
        }
        sink.flush()
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn start_session(
        &mut self,
        options: SessionOptions,
        sink: Box<dyn TranscriptSink + Send>,
        listener: Box<dyn LiveListener>,
    ) -> Result<Box<dyn SttSession>, Error> {
        Ok(Box::new(ReplaySession::new(self, options, sink, listener)))
    }
}

/// One scripted track, live.
pub struct ReplaySession {
    assembler: SpanAssembler,
    emitter: LiveEmitter,
    sink: Box<dyn TranscriptSink + Send>,
    script: Vec<String>,
    /// Which line the *next* settled span gets. Also the line the current
    /// volatile hypothesis is a prefix of, which is what keeps the guess and
    /// the settled line consistent.
    cursor: usize,
    words_per_sec: f64,
}

impl ReplaySession {
    fn new(
        engine: &ReplayEngine,
        options: SessionOptions,
        sink: Box<dyn TranscriptSink + Send>,
        listener: Box<dyn LiveListener>,
    ) -> Self {
        Self {
            assembler: SpanAssembler::with_default_vad(engine.segmentation),
            emitter: LiveEmitter::new(&options, listener),
            sink,
            script: engine.script_for(options.speaker),
            cursor: 0,
            words_per_sec: engine.words_per_sec,
        }
    }

    /// The line the current utterance will settle as.
    ///
    /// Wraps around, so a replay longer than the script keeps producing rather
    /// than going quiet halfway through a demo.
    fn current_line(&self) -> Option<&str> {
        if self.script.is_empty() {
            return None;
        }
        Some(self.script[self.cursor % self.script.len()].as_str())
    }

    /// As much of the current line as the speaker has plausibly said so far.
    fn hypothesis(&self, elapsed_sec: f64) -> Option<String> {
        let line = self.current_line()?;
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.is_empty() {
            return None;
        }
        let spoken = ((elapsed_sec * self.words_per_sec).ceil() as usize).clamp(1, words.len());
        Some(words[..spoken].join(" "))
    }
}

impl SttSession for ReplaySession {
    fn engine_name(&self) -> &'static str {
        ReplayEngine::NAME
    }

    fn feed(&mut self, samples: &[i16]) -> Result<(), Error> {
        for span in self.assembler.push(samples) {
            let Some(line) = self.current_line().map(str::to_string) else {
                continue;
            };
            if self
                .emitter
                .finalize(span.start_sec, &line, self.sink.as_mut())?
            {
                self.cursor += 1;
            }
        }

        // Only guess when the pane would actually be shown the guess. A real
        // engine pays for inference here; this one pays nothing, but it has to
        // behave the same or it is not a useful stand-in.
        if self.emitter.wants_volatile()
            && let Some(open) = self.assembler.open()
            && let Some(text) = self.hypothesis(open.duration_sec())
        {
            self.emitter.volatile(open.start_sec, &text);
        }

        self.emitter.poll();
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<SessionOutcome, Error> {
        if let Some(span) = self.assembler.finish()
            && let Some(line) = self.current_line().map(str::to_string)
            && self
                .emitter
                .finalize(span.start_sec, &line, self.sink.as_mut())?
        {
            self.cursor += 1;
        }

        // The hypothesis that never settled is thrown away, never promoted.
        let discarded_volatile = self.emitter.withdraw();
        self.sink.flush()?;

        Ok(SessionOutcome {
            speaker: self.emitter.speaker(),
            finalized: self.emitter.finalized(),
            discarded_volatile,
            audio_sec: self.assembler.fed_sec(),
            engine: ReplayEngine::NAME,
        })
    }
}

/// How fast a recording is played back into a session.
#[derive(Debug, Clone)]
pub struct ReplayOptions {
    /// Audio per [`SttSession::feed`] call. Matches the block size a capture
    /// tap would deliver; 100 ms is the same order as CoreAudio's.
    pub chunk: Duration,
    /// Playback rate. `1.0` is real time, `4.0` is four times faster, and
    /// anything `<= 0.0` means "as fast as the CPU allows", which is what the
    /// tests use so they do not take a minute each.
    pub speed: f64,
}

impl Default for ReplayOptions {
    fn default() -> Self {
        Self::realtime()
    }
}

impl ReplayOptions {
    /// Wall-clock pacing, the way a meeting actually arrives. This is the mode
    /// a UI demo wants.
    pub fn realtime() -> Self {
        Self {
            chunk: Duration::from_millis(100),
            speed: 1.0,
        }
    }

    /// No sleeping at all. Same updates, same order, no waiting.
    pub fn instant() -> Self {
        Self {
            chunk: Duration::from_millis(100),
            speed: 0.0,
        }
    }

    pub fn at_speed(speed: f64) -> Self {
        Self {
            speed,
            ..Self::realtime()
        }
    }

    fn chunk_samples(&self) -> usize {
        let samples = (self.chunk.as_secs_f64() * SAMPLE_RATE as f64).round() as usize;
        samples.max(1)
    }
}

/// Feed PCM into a live session at the requested pace.
///
/// Does not finish the session — the caller owns that, because `finish` is what
/// produces the outcome and clears the tail.
pub fn replay_samples(
    pcm: &[i16],
    session: &mut dyn SttSession,
    options: &ReplayOptions,
) -> Result<(), Error> {
    let started = Instant::now();
    let mut fed = 0usize;

    for chunk in pcm.chunks(options.chunk_samples()) {
        fed += chunk.len();
        if options.speed > 0.0 {
            // Deadline from the stream start, not from the last chunk, so the
            // playback does not drift by one scheduler quantum per block.
            let due = Duration::from_secs_f64(fed as f64 / SAMPLE_RATE as f64 / options.speed);
            if let Some(wait) = due.checked_sub(started.elapsed()) {
                std::thread::sleep(wait);
            }
        }
        session.feed(chunk)?;
    }

    Ok(())
}

/// [`replay_samples`] from a 16 kHz mono WAV on disk.
pub fn replay_wav(
    wav: &Path,
    session: &mut dyn SttSession,
    options: &ReplayOptions,
) -> Result<(), Error> {
    let pcm = crate::read_wav_16k_mono(wav)?;
    replay_samples(&pcm, session, options)
}

/// Open a session on any engine, play a WAV through it, and finish it.
///
/// The one call a demo or a test needs. `engine` is a `dyn SttEngine`, so
/// pointing this at [`ReplayEngine`] or at the real whisper engine is a
/// one-word change — which is the property the Phase 1 gate cares about.
pub fn replay_track(
    wav: &Path,
    engine: &mut dyn SttEngine,
    options: SessionOptions,
    sink: Box<dyn TranscriptSink + Send>,
    listener: Box<dyn LiveListener>,
    replay: &ReplayOptions,
) -> Result<SessionOutcome, Error> {
    let pcm = crate::read_wav_16k_mono(wav)?;
    let mut session = engine.start_session(options, sink, listener)?;
    replay_samples(&pcm, session.as_mut(), replay)?;
    session.finish()
}

#[cfg(test)]
mod tests;
