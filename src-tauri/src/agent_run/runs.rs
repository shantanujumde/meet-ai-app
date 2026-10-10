//! Which meetings have a notes run going, one at most per meeting, each on a
//! thread of its own. No Tauri in here: what the window and the notification
//! hear goes through a [`Sink`], so tests can stand in for both.

use std::collections::{HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use agent::CancelHandle;

use super::{Failure, State, Status, failure};

/// Where a run's news goes.
pub trait Sink: Send + Sync {
    /// The run's status changed. Called in order, once per change.
    fn status(&self, status: &Status);
    /// The notes and `tasks` tasks are on disk.
    fn notes_ready(&self, meeting_id: &str, tasks: u32);
}

/// The run itself: given the run's cancel handle, writes the notes and
/// returns how many tasks it wrote.
pub type Work = Box<dyn FnOnce(&CancelHandle) -> Result<u32, Failure> + Send>;

/// Every notes run since launch. Managed Tauri state.
#[derive(Default)]
pub struct AgentRuns {
    shared: Arc<Shared>,
}

#[derive(Default)]
struct Shared {
    inner: Mutex<Inner>,
    /// Signalled whenever a run ends, for [`AgentRuns::shutdown`].
    ended: Condvar,
}

#[derive(Default)]
struct Inner {
    runs: HashMap<String, Entry>,
    /// Meetings whose transcript is still being finished after Stop
    /// ([`AgentRuns::transcript_pending`]).
    pending: HashSet<String>,
    /// The app is quitting: start nothing, report nothing.
    closed: bool,
}

/// `meeting_id`'s transcript is still being finished after Stop, until this
/// is dropped. "Make notes now" is refused meanwhile (TUR-168).
pub struct Pending {
    shared: Arc<Shared>,
    meeting_id: String,
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.shared.lock().pending.remove(&self.meeting_id);
    }
}

struct Entry {
    state: State,
    /// Set while the run is going.
    cancel: Option<CancelHandle>,
}

impl AgentRuns {
    /// Where `meeting_id`'s run stands.
    pub fn status(&self, meeting_id: &str) -> Status {
        status_in(&self.shared.lock(), meeting_id)
    }

    /// The app is quitting.
    pub fn is_closed(&self) -> bool {
        self.shared.lock().closed
    }

    /// Mark `meeting_id`'s transcript as still being finished, from Stop
    /// until the returned mark is dropped.
    pub fn transcript_pending(&self, meeting_id: &str) -> Pending {
        self.shared.lock().pending.insert(meeting_id.to_owned());
        Pending {
            shared: Arc::clone(&self.shared),
            meeting_id: meeting_id.to_owned(),
        }
    }

    /// Whether `meeting_id`'s transcript is still being finished after Stop.
    pub fn is_transcript_pending(&self, meeting_id: &str) -> bool {
        self.shared.lock().pending.contains(meeting_id)
    }

    /// Start `work` for `meeting_id` on a thread of its own and return the
    /// running status. A run already going for that meeting is left alone
    /// and its status returned; so is everything once the app is quitting.
    pub fn start(&self, meeting_id: &str, work: Work, sink: Arc<dyn Sink>) -> Status {
        let cancel = CancelHandle::new();
        let running = Status {
            meeting_id: meeting_id.to_owned(),
            state: State::Running,
        };
        {
            let mut inner = self.shared.lock();
            let busy = inner
                .runs
                .get(meeting_id)
                .is_some_and(|entry| entry.state == State::Running);
            if inner.closed || busy {
                return status_in(&inner, meeting_id);
            }
            inner.runs.insert(
                meeting_id.to_owned(),
                Entry {
                    state: State::Running,
                    cancel: Some(cancel.clone()),
                },
            );
            // Under the lock, so this cannot overtake the end of the run.
            sink.status(&running);
        }

        let shared = Arc::clone(&self.shared);
        let id = meeting_id.to_owned();
        let thread_sink = Arc::clone(&sink);
        let spawned = std::thread::Builder::new()
            .name("meet-ai-notes-run".to_owned())
            .spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(|| work(&cancel)))
                    .unwrap_or_else(|_| Err(failure::crashed()));
                shared.end(&id, result, thread_sink.as_ref());
            });
        if let Err(error) = spawned {
            tracing::error!(%error, "could not spawn the notes run thread");
            self.shared.end(
                meeting_id,
                Err(failure::could_not_start(error)),
                sink.as_ref(),
            );
            return self.status(meeting_id);
        }
        running
    }

    /// Record that `meeting_id`'s run cannot begin, without starting one: no
    /// thread, no agent, just the failure, so the meeting view offers Retry.
    /// Ignored while a run is going for that meeting, and once the app is
    /// quitting.
    pub fn fail(&self, meeting_id: &str, failure: Failure, sink: &dyn Sink) {
        self.set_unless_running(meeting_id, State::Failed { failure }, sink);
    }

    /// Record `state` for `meeting_id` and tell the sink, unless a run is
    /// going for that meeting or the app is quitting.
    fn set_unless_running(&self, meeting_id: &str, state: State, sink: &dyn Sink) {
        let mut inner = self.shared.lock();
        let busy = inner
            .runs
            .get(meeting_id)
            .is_some_and(|entry| entry.state == State::Running);
        if inner.closed || busy {
            return;
        }
        inner.runs.insert(
            meeting_id.to_owned(),
            Entry {
                state: state.clone(),
                cancel: None,
            },
        );
        sink.status(&Status {
            meeting_id: meeting_id.to_owned(),
            state,
        });
    }

    /// Stop has been pressed and `meeting_id`'s transcript is still being
    /// written: say so, so the meeting view shows *Finishing the
    /// transcript…* until the run starts or fails. Ignored while a run is
    /// going for that meeting, and once the app is quitting.
    pub fn wait_for_transcript(&self, meeting_id: &str, sink: &dyn Sink) {
        self.set_unless_running(meeting_id, State::WaitingForTranscript, sink);
    }

    /// The wait for the transcript ended with no run to follow (notes were
    /// switched off, say): back to idle, but only if still waiting.
    pub fn stop_waiting(&self, meeting_id: &str, sink: &dyn Sink) {
        let mut inner = self.shared.lock();
        let waiting = inner
            .runs
            .get(meeting_id)
            .is_some_and(|entry| entry.state == State::WaitingForTranscript);
        if inner.closed || !waiting {
            return;
        }
        inner.runs.remove(meeting_id);
        sink.status(&status_in(&inner, meeting_id));
    }

    /// Cancel `meeting_id`'s run, if one is going. The run then ends as
    /// cancelled, which arrives through the sink.
    pub fn cancel(&self, meeting_id: &str) -> Status {
        let inner = self.shared.lock();
        if let Some(cancel) = inner.runs.get(meeting_id).and_then(|e| e.cancel.as_ref()) {
            cancel.cancel();
        }
        status_in(&inner, meeting_id)
    }

    /// The app is quitting: start no more runs, cancel every one going, and
    /// wait up to `wait` for their CLIs to be stopped. Runs that end this way
    /// report nothing and leave no state behind; the agent was stopped before
    /// it answered, so the meeting folder is as it was.
    pub fn shutdown(&self, wait: Duration) {
        let mut inner = self.shared.lock();
        inner.closed = true;
        for entry in inner.runs.values() {
            if let Some(cancel) = &entry.cancel {
                cancel.cancel();
            }
        }
        let (inner, timeout) = self
            .shared
            .ended
            .wait_timeout_while(inner, wait, |inner| {
                inner.runs.values().any(|e| e.state == State::Running)
            })
            .unwrap_or_else(PoisonError::into_inner);
        if timeout.timed_out() {
            let left = inner
                .runs
                .values()
                .filter(|e| e.state == State::Running)
                .count();
            tracing::warn!(left, "notes runs still going at quit");
        }
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Record how `meeting_id`'s run ended and tell the sink.
    fn end(&self, meeting_id: &str, result: Result<u32, Failure>, sink: &dyn Sink) {
        let state = match result {
            Ok(tasks) => State::Done { tasks },
            Err(failure) => State::Failed { failure },
        };
        let mut inner = self.lock();
        if inner.closed {
            // Quitting: the run was stopped on purpose, nothing to show.
            inner.runs.remove(meeting_id);
            drop(inner);
            self.ended.notify_all();
            return;
        }
        inner.runs.insert(
            meeting_id.to_owned(),
            Entry {
                state: state.clone(),
                cancel: None,
            },
        );
        sink.status(&Status {
            meeting_id: meeting_id.to_owned(),
            state: state.clone(),
        });
        drop(inner);
        self.ended.notify_all();
        if let State::Done { tasks } = state {
            sink.notes_ready(meeting_id, tasks);
        }
    }
}

fn status_in(inner: &Inner, meeting_id: &str) -> Status {
    Status {
        meeting_id: meeting_id.to_owned(),
        state: inner
            .runs
            .get(meeting_id)
            .map_or(State::Idle, |entry| entry.state.clone()),
    }
}
