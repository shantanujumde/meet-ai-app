//! What quitting finishes before the process ends (TUR-160).
//!
//! Every agent CLI the app started is stopped, and the recording is finished
//! whatever phase it is in ([`Recorder::finish_for_quit`]), so a quit leaves
//! a complete meeting folder and no orphan process.
//!
//! It runs off the main thread when it can: "Stop and quit" holds the quit,
//! runs [`finish_work`] on a thread of its own and quits again once it is
//! done ([`finish_then_quit`]), so the window stays responsive while the
//! recording closes. Logout, shutdown and an updater restart go straight to
//! `RunEvent::Exit`, which cannot be held; there [`on_exit`] does the same
//! work on the main thread, bounded, and logs how long it took. After a held
//! quit it finds nothing left to do.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager as _};

use super::Lifecycle;
use crate::agent_setup::TestRuns;
use crate::recording::Recorder;
use crate::sync::SyncRuns;

/// How long quitting waits for each kind of agent run to be stopped, like
/// `agent_run`'s own.
const QUIT_WAIT: Duration = Duration::from_secs(3);

/// Stop every agent run, then finish the recording. Bounded: at most
/// [`QUIT_WAIT`] per kind of run (cancelling kills the CLI at once, so
/// usually far less), then the recorder's own bounds.
pub fn finish_work(app: &AppHandle) {
    // TUR-10: first, so the stop below starts no notes run, and a run
    // already going has its agent stopped before it answers.
    crate::agent_run::shutdown(app);
    if let Some(runs) = app.try_state::<SyncRuns>() {
        runs.shutdown(QUIT_WAIT);
    }
    if let Some(runs) = app.try_state::<TestRuns>() {
        runs.shutdown(QUIT_WAIT);
    }
    // TUR-97: a normal quit mid-recording (⌘Q, the menu bar's Quit, a
    // logout asking apps to quit) used to leave the files exactly as a
    // `kill -9` does. A no-op when nothing is recording; a hard kill never
    // reaches this, which is what the checkpoints are for.
    if let Some(recorder) = app.try_state::<Recorder>() {
        let phase = recorder.finish_for_quit(app);
        tracing::info!(?phase, "recorder finished for quit");
    }
}

/// `RunEvent::Exit`, the last event before the process ends: [`finish_work`]
/// on the main thread, since nothing can hold it now. Skipped after a held
/// quit already did it, so a bound it hit is not waited out twice.
pub fn on_exit(app: &AppHandle) {
    let done = app
        .try_state::<Lifecycle>()
        .is_some_and(|state| state.work_finished.load(Ordering::SeqCst));
    if done {
        return;
    }
    let started = Instant::now();
    finish_work(app);
    tracing::info!(
        elapsed_ms = started.elapsed().as_millis(),
        "finished the work left at exit"
    );
}

/// A held quit: [`finish_work`] on a thread of its own, then quit again,
/// which now goes through ([`super::ExitRequest::finished`]).
pub(super) fn finish_then_quit(app: &AppHandle) {
    let handle = app.clone();
    let spawned = std::thread::Builder::new()
        .name("meet-ai-quit".to_owned())
        .spawn(move || {
            let started = Instant::now();
            finish_work(&handle);
            tracing::info!(
                elapsed_ms = started.elapsed().as_millis(),
                "finished the work left at quit"
            );
            done_then_quit(&handle);
        });
    if let Err(error) = spawned {
        // The quit still happens; `on_exit` does the work on the main thread.
        tracing::warn!(%error, "could not spawn the quit thread; finishing at exit");
        done_then_quit(app);
    }
}

fn done_then_quit(app: &AppHandle) {
    if let Some(state) = app.try_state::<Lifecycle>() {
        state.work_finished.store(true, Ordering::SeqCst);
    }
    super::request_quit(app);
}
