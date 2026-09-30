//! Live transcription while a meeting records: lines on screen and in
//! `transcript.md` as they settle (TUR-96, SPEC §5 Phase 2).
//!
//! Everything this needs already existed and was tested; this module is the
//! wiring. [`crate::recording`] hands it one [`TeeFeed`] per captured track —
//! the live copy of each channel's 16 kHz frames that TUR-31 settled on — and
//! it opens one `stt` session per track, against whichever engine Settings
//! chose, sharing one [`SeqCounter`] and one [`MarkdownSink`]:
//!
//! * **Finals** go to `transcript.md` through the sink, which owns the SPEC
//!   §3.4 line format. They also go to the window.
//! * **Volatiles** go to the window and nowhere else (SPEC §2.5). They are
//!   never written, so a guess that never settles can never become a line.
//! * **Every** [`LiveUpdate`] is emitted as-is on [`UPDATE_EVENT`], and kept in
//!   a small in-memory board so a window opened mid-meeting can catch up with
//!   [`LiveTranscript::snapshot`] (the `live_transcript` command).
//!
//! # Transcription is never allowed to cost the recording
//!
//! Audio is the one thing that cannot be recovered later; a transcript can be
//! re-run from the WAVs. So nothing here can stop, stall, or fail a recording:
//!
//! * The tee never blocks capture (see `audio::tee`). If the engine falls
//!   behind, transcription loses frames, the WAV does not.
//! * An engine that will not start, or fails mid-meeting, ends *transcription*
//!   — both tracks, so the window's "transcription stopped" is simply true —
//!   and says so on [`STATUS_EVENT`] in a sentence the user can read. The
//!   recording carries on untouched.
//! * Stop waits for the engines to flush their last lines, but only for
//!   [`STOP_TIMEOUT`]. A wedged engine is abandoned (its thread leaks, the way
//!   `audio::mic` leaks a thread blocked in Core Audio) rather than holding the
//!   Stop button hostage.
//!
//! Nothing here reaches the network: `stt` has no HTTP client in its
//! dependency graph, and engine selection is filesystem-only apart from the
//! local `meet-stt --probe` (L9–L11).
//!
//! # Timestamps
//!
//! `start_sec` on a live line is the position in the tee, which is the position
//! in the WAV — the tee pads its own gaps so the two never drift apart. That is
//! the same WAV-frame timeline a batch re-run reads, so a re-transcribed
//! meeting and the live one agree about when things were said.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use audio::tee::TeeFeed;
use serde::Serialize;
use stt::registry;
use stt::{
    LiveLine, LiveUpdate, MarkdownSink, SeqCounter, SessionOptions, SharedSink, Speaker, SttEngine,
    SttSession, TranscriptSink, Utterance,
};

/// Every [`LiveUpdate`], serialized as `stt` defines it: `kind` is
/// `volatile`/`final`/`dropped`, `speaker` is `you`/`others`.
pub const UPDATE_EVENT: &str = "transcript://update";

/// Every change of [`Status`].
pub const STATUS_EVENT: &str = "transcript://status";

/// How long Stop waits for the engines to flush their last lines.
///
/// Generous next to what a healthy engine needs — the Apple sidecar finalizes
/// within ~4 s of a span ending (TUR-31) and whisper settles one last span —
/// and short enough that a wedged engine costs the user a pause, not a hang.
pub const STOP_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a guess may sit on screen, in the track's own audio, with the
/// engine neither updating, settling nor withdrawing it.
///
/// Apple's model guesses at room tone (one "I", measured on
/// `room-tone-30s.wav`) and then says nothing until the stream ends, which
/// would leave a phantom "still speaking" line up for a whole quiet stretch —
/// the live form of the hallucination TUR-67 guards against. Longer than the
/// ~4 s Apple takes to settle a real utterance after it ends (TUR-31), so a
/// genuine guess is replaced by its final line, not withdrawn first; and if
/// one ever is, the final still lands, because a final always appends.
const STALE_GUESS: Duration = Duration::from_secs(6);

/// How often a feeding thread looks up from an empty queue to see whether the
/// recording has stopped.
const FEED_POLL: Duration = Duration::from_millis(100);

/// Where live transcription is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Not transcribing: no meeting yet, or the engine is still loading.
    Idle,
    /// Lines are being produced.
    Running,
    /// The meeting ended and every line that settled is in `transcript.md`.
    Stopped,
    /// Transcription ended early. `detail` says why; the recording did not.
    Failed,
}

/// What [`STATUS_EVENT`] carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    pub state: State,
    /// `apple-speech` or `whisper`, once one has been opened.
    pub engine: Option<String>,
    /// A sentence for the user when `state` is `failed`.
    pub detail: Option<String>,
}

impl Status {
    fn idle() -> Self {
        Self {
            state: State::Idle,
            engine: None,
            detail: None,
        }
    }
}

/// What the `live_transcript` command returns: enough for a window opened
/// mid-meeting to draw exactly what an always-open one shows.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub status: Status,
    /// Every settled line of the current meeting, in arrival order.
    pub finals: Vec<LiveLine>,
    /// At most one live hypothesis per speaker.
    pub volatile: Vec<LiveLine>,
}

/// The pane's model, kept in Rust: settled lines plus one tail per speaker.
///
/// The same three rules the pane follows (`stt::session`'s tail contract), so
/// a snapshot and a replay of every event always agree.
#[derive(Debug, Default)]
struct Lines {
    finals: Vec<LiveLine>,
    you: Option<LiveLine>,
    others: Option<LiveLine>,
}

impl Lines {
    fn tail(&mut self, speaker: Speaker) -> &mut Option<LiveLine> {
        match speaker {
            Speaker::You => &mut self.you,
            Speaker::Others => &mut self.others,
        }
    }

    fn apply(&mut self, update: &LiveUpdate) {
        match update {
            LiveUpdate::Volatile(line) => *self.tail(line.speaker) = Some(line.clone()),
            LiveUpdate::Final(line) => {
                *self.tail(line.speaker) = None;
                self.finals.push(line.clone());
            }
            LiveUpdate::Dropped { speaker, .. } => *self.tail(*speaker) = None,
        }
    }

    fn volatile(&self) -> Vec<LiveLine> {
        self.you.iter().chain(&self.others).cloned().collect()
    }
}

/// The state [`LiveTranscript::snapshot`] reads and the engine threads write.
#[derive(Debug)]
struct Board {
    /// Bumped at every meeting start and when a meeting is sealed. A thread
    /// only writes while this still matches the value it started with, so a
    /// wedged engine abandoned at Stop cannot scribble on the next meeting.
    generation: u64,
    status: Status,
    lines: Lines,
}

impl Default for Board {
    fn default() -> Self {
        Self {
            generation: 0,
            status: Status::idle(),
            lines: Lines::default(),
        }
    }
}

/// Where updates and status changes go: the window in the app, a collector in
/// tests. Both calls come from engine threads and must not block.
pub trait Notify: Send + Sync + 'static {
    fn update(&self, update: &LiveUpdate);
    fn status(&self, status: &Status);
}

impl<R: tauri::Runtime> Notify for tauri::AppHandle<R> {
    fn update(&self, update: &LiveUpdate) {
        use tauri::Emitter as _;
        // A window that has gone away is not a transcription failure.
        if let Err(error) = self.emit(UPDATE_EVENT, update) {
            tracing::debug!(%error, "could not send a live transcript line to the window");
        }
    }

    fn status(&self, status: &Status) {
        use tauri::Emitter as _;
        if let Err(error) = self.emit(STATUS_EVENT, status) {
            tracing::warn!(%error, "could not tell the window about a transcription status change");
        }
    }
}

/// Builds the engine, on the thread that will own it. `Box<dyn SttEngine>` is
/// not `Send`, so it is constructed where it lives rather than moved there.
pub type OpenEngine = Box<dyn FnOnce() -> Result<Box<dyn SttEngine>, String> + Send>;

/// The engine the user chose in Settings, built the same way the settings
/// screen resolves it, so "engine switch is a config change only" holds for
/// the live path too.
pub fn open_configured_engine() -> Result<Box<dyn SttEngine>, String> {
    let transcription = crate::config::transcription();
    // `engine::discover`, not `Environment::discover`: the model lives under the
    // meetings root the user chose, and Settings looks there too — the two must
    // never disagree about whether a model is installed.
    let environment = crate::engine::discover(crate::engine::DEFAULT_LOCALE, &transcription.model);
    let (selection, engine) =
        registry::select(transcription.engine, &environment).map_err(|error| error.to_string())?;
    tracing::info!(
        engine = selection.engine.name(),
        reason = %selection.reason,
        "opening live transcription"
    );
    Ok(engine)
}

/// Managed Tauri state: the board behind the `live_transcript` command.
#[derive(Clone, Default)]
pub struct LiveTranscript {
    board: Arc<Mutex<Board>>,
}

impl LiveTranscript {
    /// What the window should be showing right now.
    pub fn snapshot(&self) -> Snapshot {
        let board = lock(&self.board);
        Snapshot {
            status: board.status.clone(),
            finals: board.lines.finals.clone(),
            volatile: board.lines.volatile(),
        }
    }

    /// Start transcribing a meeting that is already recording.
    ///
    /// Returns immediately. Clearing the previous meeting's lines happens here,
    /// synchronously; opening the engine — a `meet-stt --probe`, or loading a
    /// whisper model that takes seconds — happens on a thread of its own, and
    /// the tee queues the audio that arrives meanwhile. `tracks` is one feed
    /// per channel that is actually capturing.
    pub fn start(
        &self,
        notify: Arc<dyn Notify>,
        transcript: PathBuf,
        tracks: Vec<(Speaker, TeeFeed)>,
        open: OpenEngine,
    ) -> Transcription {
        let generation = {
            let mut board = lock(&self.board);
            board.generation += 1;
            board.status = Status::idle();
            board.lines = Lines::default();
            board.generation
        };
        let scope = Scope {
            board: Arc::clone(&self.board),
            generation,
            notify,
        };
        scope.notify.status(&Status::idle());

        let stopping = Arc::new(AtomicBool::new(false));
        let (done_tx, done) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("meet-ai-live-transcript".to_string())
            .spawn({
                let scope = scope.clone();
                let stopping = Arc::clone(&stopping);
                move || {
                    supervise(&scope, transcript, tracks, open, &stopping);
                    let _ = done_tx.send(());
                }
            });
        if let Err(error) = spawned {
            // `done_tx` went down with the closure, so `finish` sees this as
            // an unexpected stop without waiting out the timeout.
            tracing::error!(%error, "could not spawn the live transcription thread");
            scope.fail(format!(
                "Live transcription could not start: {error}. {STILL_RECORDING}"
            ));
        }

        Transcription {
            scope,
            stopping,
            done,
        }
    }
}

/// Appended to every failure sentence, because it is the thing the user most
/// needs to know and the thing a bare error message never says.
const STILL_RECORDING: &str = "Recording continues, and the meeting can be transcribed from its \
                               saved audio afterwards.";

/// One meeting's transcription, held by the recorder until Stop.
pub struct Transcription {
    scope: Scope,
    stopping: Arc<AtomicBool>,
    done: mpsc::Receiver<()>,
}

impl Transcription {
    /// Finish the meeting's transcription and report where it ended up.
    ///
    /// Call after the recording itself has stopped, so every frame is already
    /// in the tee queues: each engine is fed what is left, finishes (flushing
    /// its finals and dropping any unsettled guess), and the final status goes
    /// out. Returns within `timeout` even if an engine never does.
    pub fn finish(self, timeout: Duration) -> Status {
        self.stopping.store(true, Ordering::Release);
        match self.done.recv_timeout(timeout) {
            Ok(()) => {}
            Err(RecvTimeoutError::Timeout) => {
                tracing::warn!(
                    timeout_s = timeout.as_secs_f64(),
                    "live transcription did not finish in time; abandoning it"
                );
                self.scope.fail(format!(
                    "Live transcription did not finish within {} seconds of the recording \
                     stopping, so the last few lines may be missing from transcript.md. The \
                     recording itself is complete.",
                    timeout.as_secs()
                ));
            }
            Err(RecvTimeoutError::Disconnected) => {
                // The supervisor ended without saying so — it panicked, or never
                // spawned. Only speak up if nothing already explained it.
                if !self.scope.is_settled() {
                    self.scope.fail(format!(
                        "Live transcription stopped unexpectedly. {STILL_RECORDING}"
                    ));
                }
            }
        }
        self.scope.seal()
    }
}

/// Everything an engine thread needs to write to the board and the window
/// safely: which board, which meeting, and where events go.
#[derive(Clone)]
struct Scope {
    board: Arc<Mutex<Board>>,
    generation: u64,
    notify: Arc<dyn Notify>,
}

impl Scope {
    /// Change the board if this meeting still owns it. The lock is released
    /// before anything is emitted, so a slow window never holds up an engine
    /// thread's peer or the snapshot command.
    fn with_board<T>(&self, change: impl FnOnce(&mut Board) -> T) -> Option<T> {
        let mut board = lock(&self.board);
        (board.generation == self.generation).then(|| change(&mut board))
    }

    /// The listener body. Called from engine threads, never blocks.
    fn update(&self, update: &LiveUpdate) {
        if self.with_board(|board| board.lines.apply(update)).is_some() {
            self.notify.update(update);
        }
    }

    fn set_status(&self, status: Status) {
        let changed = self.with_board(|board| {
            // A failure is the last word on a meeting: a track that finishes
            // cleanly after its partner failed must not paper over it.
            if board.status.state == State::Failed {
                return None;
            }
            board.status = status;
            Some(board.status.clone())
        });
        if let Some(Some(status)) = changed {
            self.notify.status(&status);
        }
    }

    fn running(&self, engine: &str) {
        self.set_status(Status {
            state: State::Running,
            engine: Some(engine.to_string()),
            detail: None,
        });
    }

    /// Stop with a reason. Logged here, so every failure path is logged once.
    /// Only the first failure is reported: the second is almost always the
    /// first one's consequence.
    fn fail(&self, detail: String) {
        let changed = self.with_board(|board| {
            if board.status.state == State::Failed {
                return None;
            }
            board.status.state = State::Failed;
            board.status.detail = Some(detail);
            Some(board.status.clone())
        });
        if let Some(Some(status)) = changed {
            tracing::warn!(
                detail = status.detail.as_deref().unwrap_or_default(),
                "live transcription stopped"
            );
            self.notify.status(&status);
        }
    }

    /// The meeting ended normally. A failure already on the board stays, and
    /// is sent again so the window's last word is the true one either way.
    fn settle(&self) {
        let status = self.with_board(|board| {
            if board.status.state != State::Failed {
                board.status.state = State::Stopped;
                board.status.detail = None;
            }
            board.status.clone()
        });
        if let Some(status) = status {
            self.notify.status(&status);
        }
    }

    /// Withdraw a guess its engine has left unchanged for [`STALE_GUESS`] of
    /// this track's audio.
    ///
    /// `guess` is this track's memory of which guess it has been watching
    /// and how far into the audio it first saw it. The check and the
    /// withdrawal happen under one lock, so a guess the engine replaces in
    /// the meantime is never the one taken down.
    fn expire_stale_guess(
        &self,
        speaker: Speaker,
        fed: u64,
        guess: &mut Option<(u64, u64)>,
        seq: &SeqCounter,
    ) {
        let stale_after = STALE_GUESS.as_secs() * u64::from(stt::vad::SAMPLE_RATE);
        let dropped = self.with_board(|board| {
            let showing = board.lines.tail(speaker).as_ref().map(|line| line.seq);
            match (showing, *guess) {
                (None, _) => {
                    *guess = None;
                    None
                }
                (Some(now), Some((watched, since))) if now == watched => {
                    if fed.saturating_sub(since) < stale_after {
                        return None;
                    }
                    let update = LiveUpdate::Dropped {
                        speaker,
                        seq: seq.next(),
                    };
                    board.lines.apply(&update);
                    *guess = None;
                    Some(update)
                }
                (Some(now), _) => {
                    *guess = Some((now, fed));
                    None
                }
            }
        });
        if let Some(Some(update)) = dropped {
            tracing::debug!(
                speaker = speaker.label(),
                "withdrew a guess the engine never settled"
            );
            self.notify.update(&update);
        }
    }

    fn is_settled(&self) -> bool {
        self.with_board(|board| matches!(board.status.state, State::Stopped | State::Failed))
            .unwrap_or(true)
    }

    /// Take the meeting's ownership of the board away, keeping what it shows.
    /// Anything an abandoned thread tries to write afterwards is ignored.
    fn seal(&self) -> Status {
        let mut board = lock(&self.board);
        if board.generation == self.generation {
            board.generation += 1;
        }
        board.status.clone()
    }
}

fn lock(board: &Mutex<Board>) -> MutexGuard<'_, Board> {
    // Same recovery as `recording::Recorder::lock`: a panic elsewhere must not
    // take the snapshot command down with it.
    board
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Makes every finalized line durable the moment it settles.
///
/// `MarkdownSink` buffers, and only flushes when a session finishes. Live,
/// that would leave `transcript.md` empty for the whole meeting and lose the
/// buffered tail on a crash — the review view reads the file, not this
/// module's memory. One `write(2)` per settled line is nothing.
struct Durable(MarkdownSink);

impl TranscriptSink for Durable {
    fn write(&mut self, utterance: &Utterance) -> Result<(), stt::Error> {
        self.0.write(utterance)?;
        self.0.flush()
    }

    fn flush(&mut self) -> Result<(), stt::Error> {
        self.0.flush()
    }
}

/// Which track, in words a failure sentence can use.
fn track_name(speaker: Speaker) -> &'static str {
    match speaker {
        Speaker::You => "your microphone",
        Speaker::Others => "the other people on the call",
    }
}

/// The meeting's transcription thread: open the engine, start one session per
/// track, feed them until the recording stops, then report.
fn supervise(
    scope: &Scope,
    transcript: PathBuf,
    tracks: Vec<(Speaker, TeeFeed)>,
    open: OpenEngine,
    stopping: &Arc<AtomicBool>,
) {
    // Kept alive until every session has finished. Nothing documents that a
    // session may outlive the engine that started it, so none is asked to.
    let mut engine = match open() {
        Ok(engine) => engine,
        Err(reason) => {
            scope.fail(format!(
                "Live transcription could not start: {reason}. {STILL_RECORDING}"
            ));
            return;
        }
    };
    let engine_name = engine.name();
    let _ = scope.with_board(|board| board.status.engine = Some(engine_name.to_string()));

    let sink = match MarkdownSink::create(&transcript) {
        Ok(sink) => SharedSink::new(Durable(sink)),
        Err(error) => {
            scope.fail(format!(
                "Live transcription could not open transcript.md: {error}. {STILL_RECORDING}"
            ));
            return;
        }
    };

    // One counter for the whole meeting, so `seq` never collides across the
    // two tracks that render into one pane.
    let seq = SeqCounter::new();
    let mut sessions: Vec<(Speaker, Box<dyn SttSession>, TeeFeed)> = Vec::new();
    for (speaker, feed) in tracks {
        let listener = {
            let scope = scope.clone();
            move |update: &LiveUpdate| scope.update(update)
        };
        let options = SessionOptions::new(speaker).with_seq(seq.clone());
        match engine.start_session(options, Box::new(sink.clone()), Box::new(listener)) {
            Ok(session) => sessions.push((speaker, session, feed)),
            Err(error) => {
                scope.fail(format!(
                    "Live transcription could not start for {}: {error}. {STILL_RECORDING}",
                    track_name(speaker)
                ));
                // Close whatever did start, so no sidecar is left running.
                for (_, session, _) in sessions {
                    let _ = session.finish();
                }
                return;
            }
        }
    }

    scope.running(engine_name);

    let abort = Arc::new(AtomicBool::new(false));
    let feeders: Vec<_> = sessions
        .into_iter()
        .filter_map(|(speaker, session, feed)| {
            let spawned = std::thread::Builder::new()
                .name(format!("meet-ai-live-{}", speaker.label().to_lowercase()))
                .spawn({
                    let (scope, stopping, abort, seq) = (
                        scope.clone(),
                        Arc::clone(stopping),
                        Arc::clone(&abort),
                        seq.clone(),
                    );
                    move || {
                        let track = Track {
                            speaker,
                            feed: &feed,
                            seq: &seq,
                        };
                        feed_track(track, session, &stopping, &abort, &scope)
                    }
                });
            match spawned {
                Ok(handle) => Some(handle),
                Err(error) => {
                    abort.store(true, Ordering::Release);
                    scope.fail(format!(
                        "Live transcription could not start for {}: {error}. {STILL_RECORDING}",
                        track_name(speaker)
                    ));
                    None
                }
            }
        })
        .collect();

    for feeder in feeders {
        if feeder.join().is_err() {
            scope.fail(format!(
                "Live transcription stopped unexpectedly. {STILL_RECORDING}"
            ));
        }
    }
    drop(engine);
    scope.settle();
}

/// One captured track, as its feeding thread sees it.
struct Track<'a> {
    speaker: Speaker,
    feed: &'a TeeFeed,
    /// The meeting's counter, for the `Dropped` a stale guess is cleared with.
    seq: &'a SeqCounter,
}

/// Run engine code, turning a panic into an error. A bug in an engine is
/// still only a transcription failure, and it has to be reported while the
/// meeting is on — not discovered when Stop joins a dead thread.
fn guarded<T>(work: impl FnOnce() -> Result<T, stt::Error>) -> Result<T, stt::Error> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).unwrap_or_else(|payload| {
        let what = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown error".to_string());
        Err(stt::Error::Engine(format!(
            "the speech engine crashed ({what})"
        )))
    })
}

/// Feed one track's audio into its session until the recording stops or the
/// meeting's transcription is aborted, then finish the session.
///
/// Runs on its own thread because `feed` blocks for as long as the engine
/// needs — whisper's inference, or the Apple sidecar's pipe filling up — and
/// that wait must never reach capture. The tee queue absorbs it instead.
fn feed_track(
    track: Track<'_>,
    mut session: Box<dyn SttSession>,
    stopping: &AtomicBool,
    abort: &AtomicBool,
    scope: &Scope,
) {
    let Track { speaker, feed, seq } = track;
    let mut failure = None;
    let mut fed: u64 = 0;
    let mut guess: Option<(u64, u64)> = None;
    loop {
        if abort.load(Ordering::Acquire) {
            break;
        }
        match feed.recv_timeout(FEED_POLL) {
            Ok(samples) => {
                fed += samples.len() as u64;
                if let Err(error) = guarded(|| session.feed(&samples)) {
                    failure = Some(error);
                    break;
                }
                scope.expire_stale_guess(speaker, fed, &mut guess, seq);
            }
            // `stopping` is only set once the recording has fully stopped, so
            // an empty queue now means there is nothing left to come.
            Err(RecvTimeoutError::Timeout) if stopping.load(Ordering::Acquire) => break,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    if feed.dropped_frames() > 0 {
        tracing::warn!(
            speaker = speaker.label(),
            dropped_frames = feed.dropped_frames(),
            "the speech engine fell behind; those frames reached the WAV but not the live transcript"
        );
    }

    if let Some(error) = failure {
        // Say so before `finish`, which is itself engine work and could hang.
        abort.store(true, Ordering::Release);
        scope.fail(format!(
            "Live transcription stopped because the speech engine failed on {}: {error}. \
             {STILL_RECORDING}",
            track_name(speaker)
        ));
    }

    match guarded(|| session.finish()) {
        Ok(outcome) => tracing::info!(
            speaker = speaker.label(),
            engine = outcome.engine,
            finalized = outcome.finalized,
            audio_sec = outcome.audio_sec,
            discarded_volatile = outcome.discarded_volatile,
            "live transcription finished"
        ),
        Err(error) => {
            abort.store(true, Ordering::Release);
            scope.fail(format!(
                "Live transcription of {} did not finish cleanly: {error}. The recording itself \
                 is complete, and the meeting can be transcribed from its saved audio.",
                track_name(speaker)
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Instant;

    use stt::{LiveEmitter, SessionOutcome};

    use super::*;

    fn line(seq: u64, speaker: Speaker, text: &str) -> LiveLine {
        LiveLine {
            seq,
            speaker,
            start_sec: seq as f64,
            text: text.into(),
        }
    }

    // --- the board ------------------------------------------------------

    #[test]
    fn a_volatile_replaces_only_that_speakers_tail() {
        let mut lines = Lines::default();
        lines.apply(&LiveUpdate::Volatile(line(0, Speaker::You, "sess")));
        lines.apply(&LiveUpdate::Volatile(line(1, Speaker::Others, "morn")));
        lines.apply(&LiveUpdate::Volatile(line(2, Speaker::You, "sessions are")));

        let volatile = lines.volatile();
        assert_eq!(volatile.len(), 2, "one tail per speaker, never a list");
        assert_eq!(volatile[0].text, "sessions are");
        assert_eq!(volatile[1].text, "morn");
        assert!(
            lines.finals.is_empty(),
            "a volatile is never a settled line"
        );
    }

    #[test]
    fn a_final_clears_its_speakers_tail_and_appends() {
        let mut lines = Lines::default();
        lines.apply(&LiveUpdate::Volatile(line(0, Speaker::You, "sessions are")));
        lines.apply(&LiveUpdate::Volatile(line(1, Speaker::Others, "morn")));
        lines.apply(&LiveUpdate::Final(line(
            2,
            Speaker::You,
            "Sessions are in memory.",
        )));

        assert_eq!(
            lines.finals,
            [line(2, Speaker::You, "Sessions are in memory.")]
        );
        let volatile = lines.volatile();
        assert_eq!(volatile.len(), 1, "the other speaker's tail is untouched");
        assert_eq!(volatile[0].speaker, Speaker::Others);
    }

    #[test]
    fn a_dropped_clears_the_tail_and_appends_nothing() {
        let mut lines = Lines::default();
        lines.apply(&LiveUpdate::Volatile(line(0, Speaker::Others, "I")));
        lines.apply(&LiveUpdate::Dropped {
            speaker: Speaker::Others,
            seq: 1,
        });
        assert!(lines.volatile().is_empty());
        assert!(lines.finals.is_empty());
    }

    // --- the wire shapes the frontend is built against --------------------

    #[test]
    fn the_status_payload_has_the_agreed_shape() {
        let json = serde_json::to_value(Status {
            state: State::Failed,
            engine: Some("whisper".into()),
            detail: Some("It stopped.".into()),
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"state": "failed", "engine": "whisper", "detail": "It stopped."})
        );
        assert_eq!(
            serde_json::to_value(Status::idle()).unwrap(),
            serde_json::json!({"state": "idle", "engine": null, "detail": null})
        );
    }

    #[test]
    fn the_snapshot_payload_has_the_agreed_shape() {
        let live = LiveTranscript::default();
        {
            let mut board = lock(&live.board);
            board
                .lines
                .apply(&LiveUpdate::Final(line(0, Speaker::You, "Morning.")));
            board
                .lines
                .apply(&LiveUpdate::Volatile(line(1, Speaker::Others, "hi")));
        }
        let json = serde_json::to_value(live.snapshot()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "status": {"state": "idle", "engine": null, "detail": null},
                "finals": [{"seq": 0, "speaker": "you", "start_sec": 0.0, "text": "Morning."}],
                "volatile": [{"seq": 1, "speaker": "others", "start_sec": 1.0, "text": "hi"}],
            })
        );
    }

    // --- test doubles -----------------------------------------------------

    #[derive(Default)]
    struct CollectingNotify {
        updates: Mutex<Vec<LiveUpdate>>,
        statuses: Mutex<Vec<Status>>,
    }

    impl CollectingNotify {
        fn states(&self) -> Vec<State> {
            self.statuses
                .lock()
                .unwrap()
                .iter()
                .map(|s| s.state)
                .collect()
        }

        fn last_status(&self) -> Status {
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
    enum Mode {
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
    struct Gate(Arc<(Mutex<bool>, std::sync::Condvar)>);

    impl Gate {
        fn open(&self) {
            *self.0.0.lock().unwrap() = true;
            self.0.1.notify_all();
        }

        fn wait(&self) {
            let mut open = self.0.0.lock().unwrap();
            while !*open {
                open = self.0.1.wait(open).unwrap();
            }
        }
    }

    /// A deterministic engine: no model, no sidecar, no audio analysis. The
    /// real engines are tested in `crates/stt`; what is under test here is
    /// the wiring around them.
    struct FakeEngine(Mode);

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

    struct FakeSession {
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

    fn fake(mode: Mode) -> OpenEngine {
        Box::new(move || Ok(Box::new(FakeEngine(mode)) as Box<dyn SttEngine>))
    }

    fn temp_transcript(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "meet-ai-live-transcript-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("transcript.md")
    }

    fn wait_for(what: &str, condition: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !condition() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_default()
    }

    // --- the lifecycle ----------------------------------------------------

    #[test]
    fn finals_reach_transcript_md_in_the_spec_format_and_the_window_sees_everything() {
        let path = temp_transcript("happy");
        let live = LiveTranscript::default();
        let notify = Arc::new(CollectingNotify::default());
        let (mic_tee, mic_feed) = audio::tee::tee();
        let (sys_tee, sys_feed) = audio::tee::tee();

        let transcription = live.start(
            notify.clone(),
            path.clone(),
            vec![(Speaker::You, mic_feed), (Speaker::Others, sys_feed)],
            fake(Mode::Echo),
        );
        mic_tee.offer(&[1; 160]);
        sys_tee.offer(&[1; 160]);
        mic_tee.offer(&[1; 160]);

        // Mid-meeting: both the file and the snapshot already have the lines —
        // nothing waits for Stop to be flushed.
        wait_for("three settled lines", || live.snapshot().finals.len() == 3);
        let mid = read(&path);
        assert!(
            mid.contains("You: You line 2."),
            "transcript.md so far: {mid:?}"
        );
        assert_eq!(live.snapshot().status.state, State::Running);
        assert_eq!(live.snapshot().status.engine.as_deref(), Some("fake"));

        // The recording stops: the tees go away, then transcription finishes.
        drop((mic_tee, sys_tee));
        let status = transcription.finish(STOP_TIMEOUT);
        assert_eq!(status.state, State::Stopped);
        assert_eq!(
            notify.states(),
            [State::Idle, State::Running, State::Stopped]
        );

        let body = read(&path);
        let mut lines: Vec<&str> = body.lines().collect();
        lines.sort_unstable();
        assert_eq!(
            lines,
            [
                "[00:00:01] Others: Others line 1.",
                "[00:00:01] You: You line 1.",
                "[00:00:02] You: You line 2.",
            ],
            "two tracks, one §3.4 file"
        );

        let updates = notify.updates.lock().unwrap().clone();
        let seqs: std::collections::HashSet<u64> = updates.iter().map(LiveUpdate::seq).collect();
        assert_eq!(
            seqs.len(),
            updates.len(),
            "seq is unique across both tracks"
        );
        // A volatile never reaches disk…
        assert!(!body.contains("hearing something") && !body.contains("and then"));
        // …and the guesses left showing at the end were withdrawn, not kept.
        assert!(
            live.snapshot().volatile.is_empty(),
            "no stale tail after Stop"
        );
        assert!(
            updates
                .iter()
                .any(|update| matches!(update, LiveUpdate::Dropped { .. }))
        );
    }

    #[test]
    fn an_engine_that_will_not_start_fails_transcription_and_nothing_else() {
        let path = temp_transcript("no-engine");
        let live = LiveTranscript::default();
        let notify = Arc::new(CollectingNotify::default());
        let (mic_tee, mic_feed) = audio::tee::tee();

        let transcription = live.start(
            notify.clone(),
            path.clone(),
            vec![(Speaker::You, mic_feed)],
            Box::new(|| Err("no speech engine is ready".to_string())),
        );
        wait_for("the failure", || {
            live.snapshot().status.state == State::Failed
        });

        let detail = live.snapshot().status.detail.unwrap();
        assert!(detail.contains("no speech engine is ready"), "{detail}");
        assert!(detail.contains("Recording continues"), "{detail}");

        // The capture side keeps offering audio into a tee nobody reads any
        // more. That must cost it nothing: no block, no panic, no error.
        for _ in 0..(audio::tee::DEFAULT_CAPACITY_CHUNKS + 10) {
            mic_tee.offer(&[1; 160]);
        }

        let started = Instant::now();
        let status = transcription.finish(STOP_TIMEOUT);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "Stop did not wait"
        );
        assert_eq!(
            status.state,
            State::Failed,
            "the failure stays the last word"
        );
        assert_eq!(read(&path), "", "nothing was written");
    }

    #[test]
    fn a_mid_meeting_failure_stops_both_tracks_and_keeps_what_settled() {
        let path = temp_transcript("mid-failure");
        let live = LiveTranscript::default();
        let notify = Arc::new(CollectingNotify::default());
        let (mic_tee, mic_feed) = audio::tee::tee();
        let (sys_tee, sys_feed) = audio::tee::tee();

        let transcription = live.start(
            notify.clone(),
            path.clone(),
            vec![(Speaker::You, mic_feed), (Speaker::Others, sys_feed)],
            fake(Mode::FailOnFeed(2)),
        );
        mic_tee.offer(&[1; 160]);
        wait_for("the first line", || !live.snapshot().finals.is_empty());
        mic_tee.offer(&[1; 160]); // the one that fails
        wait_for("the failure", || {
            live.snapshot().status.state == State::Failed
        });

        let status = live.snapshot().status;
        let detail = status.detail.unwrap();
        assert!(detail.contains("your microphone"), "{detail}");
        assert!(detail.contains("the fake engine fell over"), "{detail}");
        assert_eq!(status.engine.as_deref(), Some("fake"));

        // The recording is still going; the other track's audio has nowhere
        // to go now, and that is fine.
        sys_tee.offer(&[1; 160]);
        mic_tee.offer(&[1; 160]);

        drop((mic_tee, sys_tee));
        assert_eq!(transcription.finish(STOP_TIMEOUT).state, State::Failed);
        assert!(
            read(&path).contains("[00:00:01] You: You line 1."),
            "what settled before the failure is kept"
        );
        assert!(
            live.snapshot().volatile.is_empty(),
            "the failed track's guess was withdrawn, not left on screen"
        );
    }

    #[test]
    fn a_wedged_engine_cannot_hold_stop_hostage_or_touch_the_next_meeting() {
        let path = temp_transcript("wedged");
        let live = LiveTranscript::default();
        let notify = Arc::new(CollectingNotify::default());
        let (mic_tee, mic_feed) = audio::tee::tee();

        let transcription = live.start(
            notify.clone(),
            path.clone(),
            vec![(Speaker::You, mic_feed)],
            fake(Mode::WedgeOnFinish),
        );
        mic_tee.offer(&[1; 160]);
        wait_for("the first line", || !live.snapshot().finals.is_empty());
        drop(mic_tee);

        let started = Instant::now();
        let status = transcription.finish(Duration::from_millis(200));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "Stop was bounded"
        );
        assert_eq!(status.state, State::Failed);
        assert!(status.detail.unwrap().contains("did not finish"));
        assert!(
            read(&path).contains("You: You line 1."),
            "lines that settled before Stop are already on disk"
        );

        // The next meeting starts while the old engine thread is still stuck.
        let (_tee, feed) = audio::tee::tee();
        let next = live.start(
            notify.clone(),
            temp_transcript("wedged-next"),
            vec![(Speaker::You, feed)],
            Box::new(|| Err("not this time".to_string())),
        );
        assert!(
            live.snapshot().finals.is_empty(),
            "a new meeting starts clean"
        );
        let _ = next.finish(STOP_TIMEOUT);
    }

    #[test]
    fn a_sealed_meeting_ignores_late_writes() {
        let live = LiveTranscript::default();
        let notify = Arc::new(CollectingNotify::default());
        let scope = Scope {
            board: Arc::clone(&live.board),
            generation: lock(&live.board).generation,
            notify: notify.clone(),
        };
        scope.update(&LiveUpdate::Final(line(0, Speaker::You, "In time.")));
        scope.seal();
        scope.update(&LiveUpdate::Final(line(1, Speaker::You, "Too late.")));
        scope.fail("late".into());

        let snapshot = live.snapshot();
        assert_eq!(snapshot.finals, [line(0, Speaker::You, "In time.")]);
        assert_eq!(snapshot.status.state, State::Idle);
        assert_eq!(
            notify.updates.lock().unwrap().len(),
            1,
            "nothing late was emitted"
        );
    }

    #[test]
    fn a_failure_is_not_overwritten_by_a_later_clean_finish() {
        let live = LiveTranscript::default();
        let notify = Arc::new(CollectingNotify::default());
        let scope = Scope {
            board: Arc::clone(&live.board),
            generation: lock(&live.board).generation,
            notify: notify.clone(),
        };
        scope.running("fake");
        scope.fail("first".into());
        scope.fail("second".into());
        scope.running("fake");
        scope.settle();

        assert_eq!(
            notify.states(),
            [State::Running, State::Failed, State::Failed],
            "one failure reported, then re-sent as the final word"
        );
        assert_eq!(notify.last_status().detail.as_deref(), Some("first"));
    }

    // --- adversarial ------------------------------------------------------

    /// One chunk of audio. The fake engine settles one line per chunk, so
    /// the content never matters, only the count.
    fn chunk() -> Vec<i16> {
        vec![1; 160]
    }

    fn final_texts(live: &LiveTranscript) -> Vec<String> {
        live.snapshot()
            .finals
            .into_iter()
            .map(|line| line.text)
            .collect()
    }

    fn finals_of(updates: &[LiveUpdate]) -> impl Iterator<Item = u64> + '_ {
        updates.iter().filter_map(|update| match update {
            LiveUpdate::Final(line) => Some(line.seq),
            _ => None,
        })
    }

    #[test]
    fn an_engine_that_panics_mid_meeting_is_reported_while_the_meeting_is_still_on() {
        let path = temp_transcript("panic");
        let live = LiveTranscript::default();
        let notify = Arc::new(CollectingNotify::default());
        let (mic_tee, mic_feed) = audio::tee::tee();
        let (sys_tee, sys_feed) = audio::tee::tee();

        let transcription = live.start(
            notify.clone(),
            path.clone(),
            vec![(Speaker::You, mic_feed), (Speaker::Others, sys_feed)],
            fake(Mode::PanicOnFeed(2)),
        );
        mic_tee.offer(&chunk());
        wait_for("the first line", || !live.snapshot().finals.is_empty());
        mic_tee.offer(&chunk()); // the one that panics

        // The window has to hear about it now, not at Stop: the user is
        // still in the meeting, looking at a pane that has gone quiet.
        wait_for("the panic to be reported", || {
            live.snapshot().status.state == State::Failed
        });
        let detail = live.snapshot().status.detail.unwrap();
        assert!(detail.contains("your microphone"), "{detail}");
        assert!(detail.contains("crashed"), "{detail}");
        assert!(detail.contains("Recording continues"), "{detail}");

        sys_tee.offer(&chunk());
        drop((mic_tee, sys_tee));
        assert_eq!(transcription.finish(STOP_TIMEOUT).state, State::Failed);
        assert!(read(&path).contains("You: You line 1."));
        assert!(
            live.snapshot().volatile.is_empty(),
            "the crashed track's guess was still withdrawn"
        );
    }

    #[test]
    fn a_track_whose_session_will_not_open_fails_without_leaving_the_other_running() {
        let path = temp_transcript("no-session");
        let live = LiveTranscript::default();
        let notify = Arc::new(CollectingNotify::default());
        let (mic_tee, mic_feed) = audio::tee::tee();
        let (sys_tee, sys_feed) = audio::tee::tee();

        let transcription = live.start(
            notify.clone(),
            path.clone(),
            vec![(Speaker::You, mic_feed), (Speaker::Others, sys_feed)],
            fake(Mode::FailSessionFor(Speaker::Others)),
        );
        wait_for("the failure", || {
            live.snapshot().status.state == State::Failed
        });
        let status = live.snapshot().status;
        assert!(
            status
                .detail
                .as_deref()
                .unwrap()
                .contains("the other people on the call"),
            "{status:?}"
        );
        assert_eq!(status.engine.as_deref(), Some("fake"));

        // Capture carries on into tees nobody reads.
        for _ in 0..(audio::tee::DEFAULT_CAPACITY_CHUNKS + 10) {
            mic_tee.offer(&chunk());
            sys_tee.offer(&chunk());
        }
        drop((mic_tee, sys_tee));
        assert_eq!(transcription.finish(STOP_TIMEOUT).state, State::Failed);
        assert_eq!(read(&path), "");
        assert!(
            !notify.states().contains(&State::Running),
            "never claimed to be transcribing"
        );
    }

    #[test]
    fn a_microphone_only_meeting_transcribes_the_microphone() {
        let path = temp_transcript("mic-only");
        let live = LiveTranscript::default();
        let notify = Arc::new(CollectingNotify::default());
        let (mic_tee, mic_feed) = audio::tee::tee();

        let transcription = live.start(
            notify.clone(),
            path.clone(),
            vec![(Speaker::You, mic_feed)],
            fake(Mode::Echo),
        );
        mic_tee.offer(&chunk());
        mic_tee.offer(&chunk());
        drop(mic_tee);
        assert_eq!(transcription.finish(STOP_TIMEOUT).state, State::Stopped);
        assert_eq!(
            read(&path),
            "[00:00:01] You: You line 1.\n[00:00:02] You: You line 2.\n"
        );
    }

    #[test]
    fn stop_before_the_engine_has_loaded_still_transcribes_what_was_recorded() {
        let path = temp_transcript("slow-load");
        let live = LiveTranscript::default();
        let notify = Arc::new(CollectingNotify::default());
        let (mic_tee, mic_feed) = audio::tee::tee();

        // A model that takes a while to load; the whole meeting is over
        // before it has.
        let transcription = live.start(
            notify.clone(),
            path.clone(),
            vec![(Speaker::You, mic_feed)],
            Box::new(|| {
                std::thread::sleep(Duration::from_millis(300));
                Ok(Box::new(FakeEngine(Mode::Echo)) as Box<dyn SttEngine>)
            }),
        );
        for _ in 0..3 {
            mic_tee.offer(&chunk());
        }
        drop(mic_tee);
        assert_eq!(live.snapshot().status.state, State::Idle, "still loading");

        let status = transcription.finish(STOP_TIMEOUT);
        assert_eq!(status.state, State::Stopped);
        assert_eq!(
            read(&path).lines().count(),
            3,
            "the tee held the audio until the engine was ready"
        );
        assert_eq!(
            notify.states(),
            [State::Idle, State::Running, State::Stopped]
        );
    }

    #[test]
    fn an_engine_that_comes_back_after_stop_gave_up_cannot_reach_the_next_meeting() {
        let first_path = temp_transcript("late-first");
        let live = LiveTranscript::default();
        let notify = Arc::new(CollectingNotify::default());
        let gate = Gate::default();
        let (mic_tee, mic_feed) = audio::tee::tee();

        let first = live.start(
            notify.clone(),
            first_path.clone(),
            vec![(Speaker::You, mic_feed)],
            fake(Mode::WedgeOnFeed(gate.clone())),
        );
        mic_tee.offer(&chunk());
        drop(mic_tee);
        assert_eq!(
            first.finish(Duration::from_millis(200)).state,
            State::Failed
        );

        // The next meeting is running when the first one's engine wakes up.
        let second_path = temp_transcript("late-second");
        let (mic_tee, mic_feed) = audio::tee::tee();
        let second = live.start(
            notify.clone(),
            second_path.clone(),
            vec![(Speaker::You, mic_feed)],
            fake(Mode::Echo),
        );
        mic_tee.offer(&chunk());
        wait_for("the second meeting's line", || {
            live.snapshot().finals.len() == 1
        });
        let updates_before = notify.updates.lock().unwrap().len();

        gate.open();
        // Give the woken engine every chance to misbehave.
        std::thread::sleep(Duration::from_millis(300));

        assert_eq!(final_texts(&live), ["You line 1."]);
        assert_eq!(
            notify.updates.lock().unwrap().len(),
            updates_before,
            "the first meeting's late line was never sent to the window"
        );
        assert_eq!(live.snapshot().status.state, State::Running);
        assert_eq!(read(&second_path), "[00:00:01] You: You line 1.\n");

        drop(mic_tee);
        assert_eq!(second.finish(STOP_TIMEOUT).state, State::Stopped);
    }

    #[test]
    fn rapid_start_stop_cycles_keep_every_meeting_to_itself() {
        let live = LiveTranscript::default();
        for round in 0..25 {
            let path = temp_transcript(&format!("rapid-{round}"));
            let notify = Arc::new(CollectingNotify::default());
            let (mic_tee, mic_feed) = audio::tee::tee();
            let (sys_tee, sys_feed) = audio::tee::tee();
            let transcription = live.start(
                notify.clone(),
                path.clone(),
                vec![(Speaker::You, mic_feed), (Speaker::Others, sys_feed)],
                fake(Mode::Echo),
            );
            assert!(
                live.snapshot().finals.is_empty(),
                "round {round} starts clean"
            );
            // Some rounds stop before any audio, some with a little.
            for _ in 0..(round % 3) {
                mic_tee.offer(&chunk());
                sys_tee.offer(&chunk());
            }
            drop((mic_tee, sys_tee));
            let status = transcription.finish(STOP_TIMEOUT);
            assert_eq!(status.state, State::Stopped, "round {round}");
            let expected = 2 * (round % 3);
            assert_eq!(live.snapshot().finals.len(), expected, "round {round}");
            assert_eq!(read(&path).lines().count(), expected, "round {round}");
            let seqs: Vec<u64> = live.snapshot().finals.iter().map(|l| l.seq).collect();
            assert!(
                seqs.iter().all(|&seq| seq < 20),
                "seq restarts with each meeting: {seqs:?}"
            );
        }
    }

    #[test]
    fn a_snapshot_taken_mid_stream_plus_the_events_after_it_is_every_line_once() {
        let path = temp_transcript("snapshot-race");
        let live = LiveTranscript::default();
        let notify = Arc::new(CollectingNotify::default());
        let (mic_tee, mic_feed) = audio::tee::tee();
        let (sys_tee, sys_feed) = audio::tee::tee();
        let transcription = live.start(
            notify.clone(),
            path.clone(),
            vec![(Speaker::You, mic_feed), (Speaker::Others, sys_feed)],
            fake(Mode::Echo),
        );

        let producer = std::thread::spawn(move || {
            for _ in 0..400 {
                mic_tee.offer(&chunk());
                sys_tee.offer(&chunk());
                std::thread::yield_now();
            }
        });
        // A window opening at an arbitrary moment: it subscribes (so every
        // event from `from` on reaches it), then asks for the snapshot.
        let mut windows = Vec::new();
        while !producer.is_finished() {
            let from = notify.updates.lock().unwrap().len();
            windows.push((from, live.snapshot()));
        }
        producer.join().unwrap();
        assert_eq!(transcription.finish(STOP_TIMEOUT).state, State::Stopped);

        let updates = notify.updates.lock().unwrap().clone();
        let every: std::collections::BTreeSet<u64> = finals_of(&updates).collect();
        assert_eq!(every.len(), 800);
        assert!(windows.len() > 1, "the race was actually exercised");
        for (from, snapshot) in windows {
            let mut seen: std::collections::BTreeSet<u64> =
                snapshot.finals.iter().map(|line| line.seq).collect();
            seen.extend(finals_of(&updates[from..]));
            assert_eq!(
                seen, every,
                "a window that opened at event {from} missed lines"
            );
        }
        assert_eq!(read(&path).lines().count(), 800);
    }

    #[test]
    fn a_long_meeting_keeps_every_line_and_nothing_else() {
        // A few hours' worth of lines. The board grows by lines, never by
        // audio, and the pane and the file end up agreeing on all of them.
        let path = temp_transcript("long");
        let live = LiveTranscript::default();
        let notify = Arc::new(CollectingNotify::default());
        let (mic_tee, mic_feed) = audio::tee::tee();
        let transcription = live.start(
            notify.clone(),
            path.clone(),
            vec![(Speaker::You, mic_feed)],
            fake(Mode::Echo),
        );
        for offered in 0..3000 {
            // Paced to the consumer so the tee never has to drop (that path
            // has its own tests); three updates per chunk from `Echo`.
            while offered > notify.updates.lock().unwrap().len() / 3 + 500 {
                std::thread::yield_now();
            }
            mic_tee.offer(&chunk());
        }
        drop(mic_tee);
        assert_eq!(transcription.finish(STOP_TIMEOUT).state, State::Stopped);
        let snapshot = live.snapshot();
        assert!(snapshot.volatile.is_empty());
        assert_eq!(snapshot.finals.len(), 3000);
        assert_eq!(read(&path).lines().count(), 3000);
    }

    #[test]
    fn a_guess_that_is_never_settled_or_withdrawn_does_not_stay_on_screen() {
        // Apple's model does this over room tone: one "I", then nothing. It
        // must not sit in the pane as someone "still speaking" for the rest
        // of a quiet stretch — the live form of the hallucination bug.
        let path = temp_transcript("stale-guess");
        let live = LiveTranscript::default();
        let notify = Arc::new(CollectingNotify::default());
        let (mic_tee, mic_feed) = audio::tee::tee();
        let transcription = live.start(
            notify.clone(),
            path.clone(),
            vec![(Speaker::You, mic_feed)],
            fake(Mode::GuessOnce),
        );
        let second = vec![0; 16_000];
        mic_tee.offer(&second);
        wait_for("the guess", || !live.snapshot().volatile.is_empty());

        // Still up a few seconds of audio later: a guess may be a guess.
        for _ in 0..3 {
            mic_tee.offer(&second);
        }
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            !live.snapshot().volatile.is_empty(),
            "withdrawn too eagerly"
        );

        for _ in 0..STALE_GUESS.as_secs() {
            mic_tee.offer(&second);
        }
        wait_for("the stale guess to be withdrawn", || {
            live.snapshot().volatile.is_empty()
        });
        let updates = notify.updates.lock().unwrap().clone();
        assert!(
            matches!(
                updates.last(),
                Some(LiveUpdate::Dropped {
                    speaker: Speaker::You,
                    ..
                })
            ),
            "the window was told to clear it: {updates:?}"
        );
        let seqs: Vec<u64> = updates.iter().map(LiveUpdate::seq).collect();
        assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");

        drop(mic_tee);
        assert_eq!(transcription.finish(STOP_TIMEOUT).state, State::Stopped);
        assert_eq!(read(&path), "", "a guess never reaches the file");
    }

    #[test]
    fn a_guess_the_engine_keeps_updating_is_never_withdrawn() {
        // A long monologue: the guess changes with every chunk, so however
        // long it runs it is live, not stale.
        let path = temp_transcript("fresh-guess");
        let live = LiveTranscript::default();
        let notify = Arc::new(CollectingNotify::default());
        let (mic_tee, mic_feed) = audio::tee::tee();
        let transcription = live.start(
            notify.clone(),
            path.clone(),
            vec![(Speaker::You, mic_feed)],
            fake(Mode::Echo),
        );
        for _ in 0..(STALE_GUESS.as_secs() * 3) {
            mic_tee.offer(&vec![0; 16_000]);
        }
        drop(mic_tee);
        assert_eq!(transcription.finish(STOP_TIMEOUT).state, State::Stopped);
        let dropped = notify
            .updates
            .lock()
            .unwrap()
            .iter()
            .filter(|u| matches!(u, LiveUpdate::Dropped { .. }))
            .count();
        assert_eq!(dropped, 1, "only the one `finish` sends");
    }
}

#[cfg(all(test, target_os = "macos"))]
#[path = "live_transcript_e2e.rs"]
mod e2e;
