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
mod tests {
    use super::*;
    use crate::session::{CollectingListener, SharedCollector};
    use crate::vad::FRAME_SAMPLES;

    /// A tone loud enough for the real detector, at 16 kHz.
    fn speech(seconds: f64) -> Vec<i16> {
        let total = (seconds * SAMPLE_RATE as f64) as usize;
        (0..total)
            .map(|index| {
                let t = index as f64 / SAMPLE_RATE as f64;
                // Two formant-ish tones plus a syllable envelope. Not speech,
                // but nothing in this module claims to recognize it — it only
                // has to read as voiced to `earshot`.
                let carrier = (2.0 * std::f64::consts::PI * 140.0 * t).sin() * 0.6
                    + (2.0 * std::f64::consts::PI * 700.0 * t).sin() * 0.3;
                let envelope = 0.5 + 0.5 * (2.0 * std::f64::consts::PI * 4.0 * t).sin();
                (carrier * envelope * 12_000.0) as i16
            })
            .collect()
    }

    fn silence(seconds: f64) -> Vec<i16> {
        vec![0; (seconds * SAMPLE_RATE as f64) as usize]
    }

    fn session(
        engine: &mut ReplayEngine,
        sink: SharedCollector,
        listener: CollectingListener,
    ) -> Box<dyn SttSession> {
        engine
            .start_session(
                SessionOptions::new(Speaker::You).with_volatile_per_sec(f64::INFINITY),
                Box::new(sink),
                Box::new(listener),
            )
            .expect("the replay engine streams")
    }

    #[test]
    fn thirty_seconds_of_silence_produces_no_lines_and_no_tail() {
        // The canonical bug of this role, in its streaming shape.
        let mut engine = ReplayEngine::new();
        let sink = SharedCollector::new();
        let seen = CollectingListener::new();
        let mut live = session(&mut engine, sink.clone(), seen.clone());

        replay_samples(&silence(30.0), live.as_mut(), &ReplayOptions::instant()).unwrap();
        let outcome = live.finish().unwrap();

        assert_eq!(outcome.finalized, 0, "silence must not settle anything");
        assert!(sink.is_empty(), "silence reached disk: {:?}", sink.lines());
        assert!(seen.finals().is_empty());
        assert!(
            seen.volatiles().is_empty(),
            "the detector opened no span, so there was nothing to guess about"
        );
        assert_eq!(seen.tail_for(Speaker::You), None, "no stale tail");
        assert!(!outcome.discarded_volatile);
        assert_eq!(outcome.audio_sec, 30);
    }

    #[test]
    fn a_hypothesis_that_never_settles_is_dropped_not_promoted() {
        // Speech still going when the meeting ends, cut short enough that the
        // minimum-length rule throws the span away. The guess was on screen;
        // it must come off, and it must not reach disk.
        let short = SegmentConfig {
            min_speech_frames: 10_000,
            ..SegmentConfig::default()
        };
        let mut engine = ReplayEngine::new().with_segmentation(short);
        let sink = SharedCollector::new();
        let seen = CollectingListener::new();
        let mut live = session(&mut engine, sink.clone(), seen.clone());

        replay_samples(&speech(3.0), live.as_mut(), &ReplayOptions::instant()).unwrap();
        assert!(
            seen.tail_for(Speaker::You).is_some(),
            "a hypothesis should be showing mid-utterance"
        );

        let outcome = live.finish().unwrap();
        assert!(outcome.discarded_volatile, "there was a guess to discard");
        assert_eq!(outcome.finalized, 0);
        assert!(sink.is_empty(), "a guess reached disk: {:?}", sink.lines());
        assert_eq!(
            seen.tail_for(Speaker::You),
            None,
            "finish must clear the tail or it sits on screen forever"
        );
    }

    #[test]
    fn speech_then_silence_settles_a_line_and_clears_the_tail() {
        let mut engine = ReplayEngine::new();
        let sink = SharedCollector::new();
        let seen = CollectingListener::new();
        let mut live = session(&mut engine, sink.clone(), seen.clone());

        let mut pcm = speech(2.0);
        pcm.extend(silence(2.0));
        replay_samples(&pcm, live.as_mut(), &ReplayOptions::instant()).unwrap();
        let outcome = live.finish().unwrap();

        assert_eq!(outcome.finalized, 1, "one utterance, one line");
        assert_eq!(sink.len(), 1);
        assert_eq!(sink.utterances()[0].text, DEMO_SCRIPT_YOU[0]);
        assert!(
            !seen.volatiles().is_empty(),
            "the pane should have seen the guess grow"
        );
        assert_eq!(seen.tail_for(Speaker::You), None, "the final clears it");
        assert!(!outcome.discarded_volatile);
    }

    #[test]
    fn the_guess_is_always_a_prefix_of_the_line_it_settles_as() {
        // If these two could disagree, the pane would visibly rewrite itself
        // at every final, which is the thing the tail contract is for.
        let mut engine = ReplayEngine::new();
        let sink = SharedCollector::new();
        let seen = CollectingListener::new();
        let mut live = session(&mut engine, sink.clone(), seen.clone());

        let mut pcm = speech(2.5);
        pcm.extend(silence(2.0));
        replay_samples(&pcm, live.as_mut(), &ReplayOptions::instant()).unwrap();
        live.finish().unwrap();

        let settled = &sink.utterances()[0].text;
        for guess in seen.volatiles() {
            assert!(
                settled.starts_with(&guess.text),
                "{:?} is not a prefix of {settled:?}",
                guess.text
            );
        }
    }

    #[test]
    fn the_buffer_does_not_grow_with_the_meeting() {
        // A four-hour meeting must not be four hours of RAM. Ten minutes of
        // silence is the cheapest way to assert the buffer is pruned.
        let mut assembler = SpanAssembler::with_default_vad(SegmentConfig::default());
        let block = silence(1.0);
        for _ in 0..600 {
            assert!(assembler.push(&block).is_empty());
        }
        assert_eq!(assembler.fed_sec(), 600);
        assert!(
            assembler.buffered_samples() <= block.len() + FRAME_SAMPLES,
            "held {} samples after 10 minutes",
            assembler.buffered_samples()
        );
    }

    #[test]
    fn live_and_batch_produce_the_same_transcript() {
        // If these two disagreed, the live pane and the saved `transcript.md`
        // would disagree, and the user would watch a line change after the
        // meeting ended.
        let mut pcm = speech(2.0);
        pcm.extend(silence(1.5));
        pcm.extend(speech(2.0));
        pcm.extend(silence(1.5));

        let dir = std::env::temp_dir().join(format!("meet-ai-parity-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("mic.wav");
        write_wav(&wav, &pcm);

        let mut engine = ReplayEngine::new();
        let sink = SharedCollector::new();
        let mut live = session(&mut engine, sink.clone(), CollectingListener::new());
        replay_samples(&pcm, live.as_mut(), &ReplayOptions::instant()).unwrap();
        live.finish().unwrap();

        let mut batch = crate::sink::CollectingSink::new();
        engine.transcribe(&wav, Speaker::You, &mut batch).unwrap();

        assert!(!batch.utterances.is_empty(), "the fixture has speech in it");
        assert_eq!(sink.lines(), batch.lines());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_track_replays_end_to_end_through_the_engine_trait() {
        // The shape Nia calls: nothing here names a concrete session type.
        let mut pcm = speech(2.0);
        pcm.extend(silence(1.5));
        let dir = std::env::temp_dir().join(format!("meet-ai-replay-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("mic.wav");
        write_wav(&wav, &pcm);

        let sink = SharedCollector::new();
        let seen = CollectingListener::new();
        let mut engine: Box<dyn SttEngine> = Box::new(ReplayEngine::new());
        let outcome = replay_track(
            &wav,
            engine.as_mut(),
            SessionOptions::new(Speaker::Others),
            Box::new(sink.clone()),
            Box::new(seen.clone()),
            &ReplayOptions::instant(),
        )
        .unwrap();

        assert_eq!(outcome.speaker, Speaker::Others);
        assert_eq!(outcome.engine, ReplayEngine::NAME);
        assert_eq!(outcome.finalized, 1);
        assert_eq!(sink.utterances()[0].text, DEMO_SCRIPT_OTHERS[0]);
        assert_eq!(seen.finals()[0].speaker, Speaker::Others);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_non_streaming_engine_says_so_instead_of_pretending() {
        struct BatchOnly;
        impl SttEngine for BatchOnly {
            fn name(&self) -> &'static str {
                "batch-only"
            }
            fn transcribe(
                &mut self,
                _wav: &Path,
                _speaker: Speaker,
                _sink: &mut dyn TranscriptSink,
            ) -> Result<(), Error> {
                Ok(())
            }
        }

        let mut engine = BatchOnly;
        assert!(!engine.supports_streaming());
        // `Box<dyn SttSession>` is not `Debug`, so match rather than unwrap.
        match engine.start_session(
            SessionOptions::new(Speaker::You),
            Box::new(SharedCollector::new()),
            Box::new(NoListener),
        ) {
            Err(Error::StreamingUnsupported(name)) => assert_eq!(name, "batch-only"),
            Err(other) => panic!("expected StreamingUnsupported, got {other:?}"),
            Ok(_) => panic!("an engine with no streaming path must not open a session"),
        }
    }

    fn write_wav(path: &Path, pcm: &[i16]) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for sample in pcm {
            writer.write_sample(*sample).unwrap();
        }
        writer.finalize().unwrap();
    }
}
