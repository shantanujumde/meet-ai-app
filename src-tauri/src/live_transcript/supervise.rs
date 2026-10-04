//! The meeting's transcription thread: open the engine, start one session per
//! track, feed them until the recording stops, then report.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use audio::tee::TeeFeed;
use stt::{LiveUpdate, MarkdownSink, SeqCounter, SessionOptions, SharedSink, Speaker, SttSession};

use super::board::Scope;
use super::sink::{Durable, sort_by_time};
use super::{OpenEngine, STILL_RECORDING};

/// How often a feeding thread looks up from an empty queue to see whether the
/// recording has stopped.
const FEED_POLL: Duration = Duration::from_millis(100);

/// Which track, in words a failure sentence can use.
fn track_name(speaker: Speaker) -> &'static str {
    match speaker {
        Speaker::You => "your microphone",
        Speaker::Others => "the other people on the call",
    }
}

/// The meeting's transcription thread: open the engine, start one session per
/// track, feed them until the recording stops, then report.
pub(super) fn supervise(
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
    // Sorted before the status settles and before the caller hears this
    // thread end, so Stop and the notes run (`TranscriptFinal`) both get the
    // file in time order. Only when every session has let go of the sink: a
    // line appended to the old file after the rename would be lost.
    if sink.is_last() {
        drop(sink);
        sort_by_time(&transcript);
    } else {
        tracing::warn!(
            "a speech engine still holds transcript.md, so it was left in the order written"
        );
    }
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
