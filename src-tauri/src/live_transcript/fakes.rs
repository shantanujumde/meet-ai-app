//! Test doubles for the live transcript: a collecting notifier and a fake engine.

#![cfg(test)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use stt::{
    LiveEmitter, LiveUpdate, SessionOptions, SessionOutcome, Speaker, SttEngine, SttSession,
    TranscriptSink,
};

use super::*;

#[derive(Default)]
pub(super) struct CollectingNotify {
    pub(super) updates: Mutex<Vec<LiveUpdate>>,
    pub(super) statuses: Mutex<Vec<Status>>,
}

impl CollectingNotify {
    pub(super) fn states(&self) -> Vec<State> {
        self.statuses
            .lock()
            .unwrap()
            .iter()
            .map(|s| s.state)
            .collect()
    }

    pub(super) fn last_status(&self) -> Status {
        self.statuses
            .lock()
            .unwrap()
            .last()
            .cloned()
            .expect("a status")
    }
}

impl Notify for CollectingNotify {
    fn update(&self, update: &LiveUpdate) {
        self.updates.lock().unwrap().push(update.clone());
    }
    fn status(&self, status: &Status) {
        self.statuses.lock().unwrap().push(status.clone());
    }
}

#[derive(Clone)]
pub(super) enum Mode {
    /// A guess, then a settled line, for every chunk fed.
    Echo,
    /// As `Echo`, but the given feed call (1-based) fails.
    FailOnFeed(usize),
    /// As `Echo`, but `finish` never comes back in any useful time.
    WedgeOnFinish,
    /// As `Echo`, but the given feed call (1-based) panics — a bug in an
    /// engine, which is still only a transcription failure.
    PanicOnFeed(usize),
    /// As `Echo`, but opening this speaker's session fails.
    FailSessionFor(Speaker),
    /// As `Echo`, but the first feed blocks until the gate opens, and
    /// then settles a line — an engine that comes back far too late.
    WedgeOnFeed(Gate),
    /// One guess on the first feed, then nothing ever again: Apple's
    /// model guessing at room tone and never taking it back.
    GuessOnce,
}

/// A latch a test opens to let a wedged fake engine carry on.
#[derive(Clone, Default)]
pub(super) struct Gate(Arc<(Mutex<bool>, std::sync::Condvar)>);

impl Gate {
    pub(super) fn open(&self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }

    pub(super) fn wait(&self) {
        let mut open = self.0.0.lock().unwrap();
        while !*open {
            open = self.0.1.wait(open).unwrap();
        }
    }
}

/// A deterministic engine: no model, no sidecar, no audio analysis. The
/// real engines are tested in `crates/stt`; what is under test here is
/// the wiring around them.
pub(super) struct FakeEngine(pub(super) Mode);

impl SttEngine for FakeEngine {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn transcribe(
        &mut self,
        _wav: &Path,
        _speaker: Speaker,
        _sink: &mut dyn TranscriptSink,
    ) -> Result<(), stt::Error> {
        Ok(())
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn start_session(
        &mut self,
        options: SessionOptions,
        sink: Box<dyn TranscriptSink + Send>,
        listener: Box<dyn stt::LiveListener>,
    ) -> Result<Box<dyn SttSession>, stt::Error> {
        if let Mode::FailSessionFor(speaker) = self.0
            && speaker == options.speaker
        {
            return Err(stt::Error::Engine("no session for this track".into()));
        }
        // Uncapped, so the test never depends on how fast it ran.
        let options = options.with_volatile_per_sec(f64::INFINITY);
        Ok(Box::new(FakeSession {
            speaker: options.speaker,
            emitter: LiveEmitter::new(&options, listener),
            sink,
            fed: 0,
            mode: self.0.clone(),
        }))
    }
}

pub(super) struct FakeSession {
    speaker: Speaker,
    emitter: LiveEmitter,
    sink: Box<dyn TranscriptSink + Send>,
    fed: usize,
    mode: Mode,
}

impl SttSession for FakeSession {
    fn engine_name(&self) -> &'static str {
        "fake"
    }

    fn feed(&mut self, _samples: &[i16]) -> Result<(), stt::Error> {
        self.fed += 1;
        match &self.mode {
            Mode::FailOnFeed(n) if *n == self.fed => {
                return Err(stt::Error::Engine("the fake engine fell over".into()));
            }
            Mode::PanicOnFeed(n) if *n == self.fed => panic!("the fake engine has a bug"),
            Mode::WedgeOnFeed(gate) if self.fed == 1 => gate.wait(),
            Mode::GuessOnce => {
                if self.fed == 1 {
                    self.emitter.volatile(0.0, "I");
                }
                return Ok(());
            }
            _ => {}
        }
        let at = self.fed as f64;
        self.emitter.volatile(at, "hearing something");
        self.emitter.finalize(
            at,
            &format!("{} line {}.", self.speaker.label(), self.fed),
            self.sink.as_mut(),
        )?;
        // Leave a guess showing, so `finish` has a tail to drop.
        self.emitter.volatile(at, "and then");
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<SessionOutcome, stt::Error> {
        if let Mode::WedgeOnFinish = &self.mode {
            std::thread::sleep(Duration::from_secs(30));
        }
        let discarded_volatile = self.emitter.withdraw();
        self.sink.flush()?;
        Ok(SessionOutcome {
            speaker: self.speaker,
            finalized: self.emitter.finalized(),
            discarded_volatile,
            audio_sec: 0,
            engine: "fake",
        })
    }
}

pub(super) fn fake(mode: Mode) -> OpenEngine {
    Box::new(move || Ok(Box::new(FakeEngine(mode)) as Box<dyn SttEngine>))
}

pub(super) fn temp_transcript(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "meet-ai-live-transcript-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("transcript.md")
}

pub(super) fn wait_for(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub(super) fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}
