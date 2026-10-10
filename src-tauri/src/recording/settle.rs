//! Quitting waits for the recorder to settle (TUR-160).
//!
//! [`Recorder::stop`] only stops a `Recording`. A quit that landed while the
//! recorder was `Starting` (Core Audio opening the channels) or `Stopping`
//! (the transcript's last lines) used to let the process end under it: a
//! meeting folder with empty audio, listed as Interrupted next launch, or a
//! transcript cut short with the engine threads killed. So the quit path
//! waits, bounded, for the phase to settle to `Idle` or `Recording`, stops a
//! recording, and waits for a stop already going to finish ([`finish`]).
//!
//! The waiting is on a condvar the recorder's own lock signals ([`Watched`]),
//! so it ends the moment the phase changes, never on a poll.

use std::ops::{Deref, DerefMut};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tauri::AppHandle;

use super::{Phase, Recorder};
use crate::live_transcript::STOP_TIMEOUT;
use crate::lock::lock_or_recover;

/// How long quitting waits for each phase to move on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Bounds {
    /// For `Starting` to become `Recording` (or `Idle`, a failed start).
    pub starting: Duration,
    /// For `Stopping` to become `Idle`.
    pub stopping: Duration,
}

/// The quit path's bounds: Core Audio is given 15 s to open, and a stop the
/// transcript's own [`STOP_TIMEOUT`] plus 5 s to close the files.
pub(crate) const QUIT_BOUNDS: Bounds = Bounds {
    starting: Duration::from_secs(15),
    stopping: Duration::from_secs(STOP_TIMEOUT.as_secs() + 5),
};

/// What [`finish`] needs from a recorder, so it is testable with a fake one.
pub(crate) trait Settle {
    fn phase(&self) -> Phase;
    /// Wait up to `bound` while the phase is `phase`; the phase after.
    fn wait_while_in(&self, phase: Phase, bound: Duration) -> Phase;
    /// [`Recorder::stop`]: stops a `Recording`, a no-op otherwise.
    fn stop(&self);
}

/// Let a start or a stop that is going finish, then stop a recording, so the
/// app can end with every meeting folder complete. Returns the phase it left
/// the recorder in: `Idle`, unless a bound was hit (logged at warn).
pub(crate) fn finish(recorder: &impl Settle, bounds: Bounds) -> Phase {
    let mut phase = recorder.phase();
    if phase == Phase::Starting {
        phase = settle(recorder, phase, bounds.starting);
    }
    if phase != Phase::Stopping {
        // `Recording` is stopped here. `Idle`, and a start that never
        // settled, make this a no-op: there is nothing it can stop.
        recorder.stop();
        phase = recorder.phase();
    }
    if phase == Phase::Stopping {
        // A stop from elsewhere (the button, ⌘⇧R, a failed tick) got there
        // first, or is still going.
        phase = settle(recorder, phase, bounds.stopping);
    }
    phase
}

/// Wait up to `bound` for the recorder to leave `phase`, saying so when it
/// does not.
fn settle(recorder: &impl Settle, phase: Phase, bound: Duration) -> Phase {
    let after = recorder.wait_while_in(phase, bound);
    if after == phase {
        tracing::warn!(
            ?phase,
            waited_ms = bound.as_millis(),
            "the recorder did not settle before quitting; quitting anyway"
        );
    }
    after
}

impl Recorder {
    /// For the quit path: [`finish`] with [`QUIT_BOUNDS`]. Blocks for as long
    /// as the start or stop it waits for, at most the two bounds plus one
    /// stop.
    pub fn finish_for_quit(&self, app: &AppHandle) -> Phase {
        finish(
            &Quitting {
                recorder: self,
                app,
            },
            QUIT_BOUNDS,
        )
    }

    /// Wait up to `bound` while the phase is `phase`; the phase after.
    fn wait_while_in(&self, phase: Phase, bound: Duration) -> Phase {
        self.inner
            .wait_while(bound, |inner| inner.status.phase == phase)
            .status
            .phase
    }
}

/// The app's recorder as a [`Settle`].
struct Quitting<'a> {
    recorder: &'a Recorder,
    app: &'a AppHandle,
}

impl Settle for Quitting<'_> {
    fn phase(&self) -> Phase {
        self.recorder.status().phase
    }

    fn wait_while_in(&self, phase: Phase, bound: Duration) -> Phase {
        self.recorder.wait_while_in(phase, bound)
    }

    fn stop(&self) {
        if let Err(error) = self.recorder.stop(self.app) {
            tracing::error!(message = %error.message, "could not finish the recording on quit");
        }
    }
}

/// A value behind a lock that tells waiters every time it was held, so they
/// can wait for it to change ([`Watched::wait_while`]) without polling.
#[derive(Debug)]
pub(super) struct Watched<T> {
    value: Mutex<T>,
    changed: Condvar,
}

impl<T> Watched<T> {
    pub(super) fn new(value: T) -> Self {
        Self {
            value: Mutex::new(value),
            changed: Condvar::new(),
        }
    }

    /// Lock it, recovering from a poisoned lock like
    /// [`lock_or_recover`]. Waiters hear when the guard is dropped.
    pub(super) fn lock(&self) -> Guard<'_, T> {
        Guard {
            guard: lock_or_recover(&self.value),
            changed: &self.changed,
        }
    }

    /// Wait up to `bound` while `condition` holds, and return the value as
    /// it is then, still locked.
    fn wait_while(
        &self,
        bound: Duration,
        condition: impl FnMut(&mut T) -> bool,
    ) -> MutexGuard<'_, T> {
        let (guard, _) = self
            .changed
            .wait_timeout_while(lock_or_recover(&self.value), bound, condition)
            .unwrap_or_else(PoisonError::into_inner);
        guard
    }
}

/// [`Watched::lock`]'s guard.
pub(super) struct Guard<'a, T> {
    guard: MutexGuard<'a, T>,
    changed: &'a Condvar,
}

impl<T> Deref for Guard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.guard
    }
}

impl<T> DerefMut for Guard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.guard
    }
}

impl<T> Drop for Guard<'_, T> {
    fn drop(&mut self) {
        // Still holding the lock: a waiter wakes, then takes the lock once
        // the field drops just after this.
        self.changed.notify_all();
    }
}

#[cfg(test)]
mod tests;
