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
//! * **Every** [`LiveUpdate`] is emitted as-is on [`TRANSCRIPT_UPDATE_EVENT`], and kept in
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
//! # Timestamps
//!
//! `start_sec` on a live line is the position in the tee, which is the position
//! in the WAV — the tee pads its own gaps so the two never drift apart. That is
//! the same WAV-frame timeline a batch re-run reads, so a re-transcribed
//! meeting and the live one agree about when things were said.

mod board;
#[cfg(all(test, target_os = "macos"))]
mod e2e;
#[cfg(test)]
mod fakes;
mod sink;
mod supervise;
#[cfg(test)]
mod tests;

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
}

impl Status {
    pub(super) fn idle() -> Self {
        Self {
            state: State::Idle,
            engine: None,
            detail: None,
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
    pub fn start(
        &self,
        notify: Arc<dyn Notify>,
        transcript: PathBuf,
        tracks: Vec<(Speaker, TeeFeed)>,
        open: OpenEngine,
    ) -> Transcription {
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
pub(super) const STILL_RECORDING: &str = "Recording continues, and the meeting can be transcribed from its \
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
