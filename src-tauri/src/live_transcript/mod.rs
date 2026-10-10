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
//!   §3.4 line format. They also go to the window. They land in the order
//!   they settle, which is not quite the order they were said: each track
//!   settles at its own pace. So once both sessions have finished, the file is
//!   sorted by time once (SPEC A19), before Stop or the notes run read it.
//! * **Volatiles** go to the window and nowhere else (SPEC §2.5). They are
//!   never written, so a guess that never settles can never become a line.
//! * **Every** [`LiveUpdate`] is emitted as-is on [`TRANSCRIPT_UPDATE_EVENT`], and kept in
//!   a small in-memory board so a window opened mid-meeting can catch up with
//!   [`LiveTranscript::snapshot`] (the `live_transcript` command).
//!
//! # Transcription is never allowed to cost the recording
//!
//! Audio is the one thing that cannot be recovered later, and the WAVs stay in
//! the meeting folder whatever the transcript does. So nothing here can stop,
//! stall, or fail a recording:
//!
//! * The tee never blocks capture (see `audio::tee`). If the engine falls
//!   behind, transcription loses frames, the WAV does not. The count goes
//!   out with the final [`Status`], and retention keeps the audio of a
//!   transcript with such gaps (TUR-148).
//! * An engine that will not start, or fails mid-meeting, ends *transcription*
//!   — both tracks, so the window's "transcription stopped" is simply true —
//!   and says so on [`TRANSCRIPT_STATUS_EVENT`] in a sentence the user can read. The
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
//! With `transcription.live: false` none of the above runs while the meeting
//! records; Stop transcribes the saved WAVs instead ([`after_stop`], TUR-137).
//!
//! # Timestamps
//!
//! `start_sec` on a live line is the position in the tee, which is the position
//! in the WAV — the tee pads its own gaps so the two never drift apart. That is
//! the same WAV-frame timeline the batch path after Stop reads ([`after_stop`]),
//! so a meeting transcribed either way agrees about when things were said.

mod board;
#[cfg(test)]
mod e2e;
#[cfg(test)]
mod fakes;
// TUR-137: `transcription.live: false`, the whole meeting at Stop.
mod after_stop;
mod sink;
mod supervise;
#[cfg(test)]
mod tests;

use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use audio::tee::TeeFeed;
use serde::Serialize;
use stt::registry;
use stt::{LiveLine, LiveUpdate, Speaker, SttEngine};

#[cfg(test)]
use self::board::STALE_GUESS;
use self::board::{Board, Lines, Scope};
use self::supervise::supervise;
use crate::events::{TRANSCRIPT_STATUS_EVENT, TRANSCRIPT_UPDATE_EVENT};
use crate::lock::lock_or_recover;
/// How long Stop waits for the engines to flush their last lines.
///
/// Generous next to what a healthy engine needs — the Apple sidecar finalizes
/// within ~4 s of a span ending (TUR-31) and whisper settles one last span —
/// and short enough that a wedged engine costs the user a pause, not a hang.
pub const STOP_TIMEOUT: Duration = Duration::from_secs(10);

/// Where live transcription is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
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

/// What [`TRANSCRIPT_STATUS_EVENT`] carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct Status {
    pub state: State,
    /// `apple-speech` or `whisper`, once one has been opened.
    pub engine: Option<String>,
    /// A sentence for the user when `state` is `failed`.
    pub detail: Option<String>,
    /// Frames of this meeting the live engines never got, because the tee's
    /// queue was full (the engine fell behind). More than zero means
    /// `transcript.md` has gaps only the WAVs can fill, so retention keeps
    /// the audio (TUR-148). Not sent to the window.
    #[serde(skip)]
    pub dropped_frames: u64,
}

impl Status {
    pub(super) fn idle() -> Self {
        Self {
            state: State::Idle,
            engine: None,
            detail: None,
            dropped_frames: 0,
        }
    }
}

/// What the `live_transcript` command returns: enough for a window opened
/// mid-meeting to draw exactly what an always-open one shows.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct Snapshot {
    pub status: Status,
    /// Every settled line of the current meeting, in arrival order.
    pub finals: Vec<LiveLine>,
    /// At most one live hypothesis per speaker.
    pub volatile: Vec<LiveLine>,
}

/// Where updates and status changes go: the window in the app, a collector in
/// tests. Both calls come from engine threads and must not block.
pub trait Notify: Send + Sync + 'static {
    fn update(&self, update: &LiveUpdate);
    fn status(&self, status: &Status);
    /// Transcription failed while the meeting is still recording; `detail`
    /// is the sentence `status` carried. Once per meeting (TUR-161).
    fn failed(&self, _detail: &str) {}
}

impl<R: tauri::Runtime> Notify for tauri::AppHandle<R> {
    fn update(&self, update: &LiveUpdate) {
        use tauri::Emitter as _;
        // A window that has gone away is not a transcription failure.
        if let Err(error) = self.emit(TRANSCRIPT_UPDATE_EVENT, update) {
            tracing::debug!(%error, "could not send a live transcript line to the window");
        }
    }

    fn status(&self, status: &Status) {
        use tauri::Emitter as _;
        if let Err(error) = self.emit(TRANSCRIPT_STATUS_EVENT, status) {
            tracing::warn!(%error, "could not tell the window about a transcription status change");
        }
    }

    fn failed(&self, detail: &str) {
        crate::notify::transcription_failed(self, detail);
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
    let mut environment =
        crate::engine::discover(crate::engine::DEFAULT_LOCALE, &transcription.model);
    environment.spoken = stt::languages::spoken(&transcription.language);
    let (selection, engine) =
        registry::select(transcription.engine, &environment).map_err(|error| error.to_string())?;
    tracing::info!(
        engine = selection.engine.name(),
        reason = %selection.reason,
        language = %transcription.language,
        "opening live transcription"
    );
    Ok(engine)
}

/// How a meeting is transcribed: which engine, and whether while it records
/// or only after Stop (`transcription.live`, TUR-137).
pub struct Plan {
    open: OpenEngine,
    live: bool,
}

impl Plan {
    /// What `config.jsonc` asks for, read now, so a change takes effect on
    /// the next recording.
    pub fn configured() -> Self {
        Self {
            open: Box::new(open_configured_engine),
            live: crate::config::transcription().live,
        }
    }

    /// Lines while the meeting records, from `open`'s engine.
    pub fn live(open: OpenEngine) -> Self {
        Self { open, live: true }
    }
}

impl From<OpenEngine> for Plan {
    fn from(open: OpenEngine) -> Self {
        Self::live(open)
    }
}

/// Managed Tauri state: the board behind the `live_transcript` command.
#[derive(Clone, Default)]
pub struct LiveTranscript {
    board: Arc<Mutex<Board>>,
}

impl LiveTranscript {
    /// What the window should be showing right now.
    pub fn snapshot(&self) -> Snapshot {
        let board = lock_or_recover(&self.board);
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
    ///
    /// With a [`Plan`] that is not live (`transcription.live: false`) no
    /// engine opens and the feeds are let go at once, which costs capture
    /// nothing (`audio::tee`). The WAVs are transcribed at Stop instead, see
    /// [`Transcription::finish_final`].
    pub fn start(
        &self,
        notify: Arc<dyn Notify>,
        transcript: PathBuf,
        tracks: Vec<(Speaker, TeeFeed)>,
        plan: impl Into<Plan>,
    ) -> Transcription {
        let Plan { open, live } = plan.into();
        // Not transcribing live, nothing can fail while the meeting records.
        let stopping = Arc::new(AtomicBool::new(!live));
        let generation = {
            let mut board = lock_or_recover(&self.board);
            board.generation += 1;
            board.status = Status::idle();
            board.lines = Lines::default();
            board.generation
        };
        let scope = Scope {
            board: Arc::clone(&self.board),
            generation,
            notify,
            stopping: Arc::clone(&stopping),
        };
        scope.notify.status(&Status::idle());

        if !live {
            drop(tracks);
            tracing::info!("transcription.live is off; the meeting is transcribed after Stop");
            return Transcription {
                scope,
                work: Work::AfterStop(open),
                transcript,
            };
        }

        let transcript_path = transcript.clone();
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
            work: Work::Live { stopping, done },
            transcript: transcript_path,
        }
    }
}

/// Appended to every failure sentence, because it is the thing the user most
/// needs to know and the thing a bare error message never says. Only what is
/// true (TUR-161): there is no command that transcribes a meeting again from
/// its audio, so this does not promise one.
pub(super) const STILL_RECORDING: &str =
    "Recording continues, and its audio is kept in the meeting folder.";

/// One meeting's transcription, held by the recorder until Stop.
pub struct Transcription {
    scope: Scope,
    work: Work,
    /// The meeting's `transcript.md`, inside its meeting folder.
    transcript: PathBuf,
}

/// What Stop has to finish.
enum Work {
    /// Engines running alongside the recording.
    Live {
        stopping: Arc<AtomicBool>,
        done: mpsc::Receiver<()>,
    },
    /// Nothing yet: the saved audio is transcribed at Stop with this engine.
    AfterStop(OpenEngine),
}

impl Transcription {
    /// The meeting's `transcript.md`. Its folder is the meeting's, so this
    /// also names the meeting (TUR-10's notes run).
    pub fn transcript(&self) -> &Path {
        &self.transcript
    }

    /// Finish the meeting's transcription and report where it ended up.
    ///
    /// Call after the recording itself has stopped, so every frame is already
    /// in the tee queues: each engine is fed what is left, finishes (flushing
    /// its finals and dropping any unsettled guess), and the final status goes
    /// out. Returns within `timeout` even if an engine never does.
    pub fn finish(self, timeout: Duration) -> Status {
        self.end(timeout, false).0
    }

    /// [`Self::finish`], plus a way to learn when `transcript.md` is final.
    ///
    /// Stop gives up after `timeout`, but an engine it gave up on can still be
    /// writing its last lines. Anything that reads the file afterwards — the
    /// notes run (TUR-17) — waits on the [`TranscriptFinal`] instead of
    /// assuming Stop's timeout means the file is complete.
    ///
    /// When the meeting was not transcribed live, this is where it is: the
    /// WAVs are transcribed whole on a thread of their own
    /// ([`after_stop`]), Stop waits up to `timeout` for that as it would for
    /// live engines, and the [`TranscriptFinal`] resolves only once it is
    /// done, so the notes run never reads a transcript that is not there yet.
    pub fn finish_final(self, timeout: Duration) -> (Status, TranscriptFinal) {
        self.end(timeout, true)
    }

    /// `marks_late` is whether a transcription after Stop that outlasts
    /// `timeout` marks the meeting's audio complete itself: the caller of
    /// [`Self::finish_final`] does that from the status it gets back, and
    /// that status cannot say yet.
    fn end(self, timeout: Duration, marks_late: bool) -> (Status, TranscriptFinal) {
        let (stopping, done) = match self.work {
            Work::Live { stopping, done } => (stopping, done),
            Work::AfterStop(open) => {
                return after_stop::finish(self.scope, self.transcript, open, timeout, marks_late);
            }
        };
        stopping.store(true, Ordering::Release);
        let waited = done.recv_timeout(timeout);
        match waited {
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
        // Only a timeout leaves the supervisor running; it still says when it
        // ends, on the same channel, so hand that on.
        let pending = matches!(waited, Err(RecvTimeoutError::Timeout)).then_some(done);
        (
            self.scope.seal(),
            TranscriptFinal {
                pending,
                patient: false,
            },
        )
    }
}

/// Tells when `transcript.md` is final: every engine has finished, and nothing
/// more will be written to it.
///
/// Usually that is already true when Stop returns. It is not when Stop gave up
/// on a slow engine, which may still settle a line or two afterwards, or when
/// the whole meeting is still being transcribed after Stop.
pub struct TranscriptFinal {
    /// The supervisor's "I have ended" channel, while it has not yet said so.
    /// `None` once the file is final.
    pending: Option<mpsc::Receiver<()>>,
    /// The whole meeting is being transcribed after Stop. That takes as long
    /// as the meeting's audio needs, not a slow engine's last few seconds, so
    /// [`Self::wait`] waits for it in full. An engine error or a panic still
    /// ends it.
    patient: bool,
}

impl TranscriptFinal {
    /// Whether `transcript.md` is final, waiting up to `timeout` for it.
    ///
    /// True at once when it already is. A `false` can be followed by a later
    /// wait that succeeds: a slow engine is still worth waiting for. A
    /// transcription after Stop is waited for however long it takes, past
    /// `timeout`.
    pub fn wait(&mut self, timeout: Duration) -> bool {
        let Some(done) = &self.pending else {
            return true;
        };
        let waited = if self.patient {
            // No cap, on purpose (TUR-137): notes come only after the full
            // transcript, and a batch engine's error or panic ends this wait.
            // Its start and end are logged at info, so a long wait shows.
            done.recv().map_err(|_| RecvTimeoutError::Disconnected)
        } else {
            done.recv_timeout(timeout)
        };
        match waited {
            // A supervisor that went away without saying so — it panicked —
            // will not write anything more either.
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                self.pending = None;
                true
            }
            Err(RecvTimeoutError::Timeout) => false,
        }
    }
}
