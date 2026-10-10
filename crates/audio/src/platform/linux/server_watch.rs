//! One PulseAudio connection for the default-device watch (TUR-172).
//!
//! The watch used to open a new main loop, context and connection on every
//! read, several times a second while recording. Now one thread of its own
//! (`meet-ai-pulse-defaults`) holds one connection, subscribed to the
//! server's change events: it reads the default sink and source once, then
//! again only when the server says something changed. [`current`] hands out
//! the latest answer without touching the server.
//!
//! The thread starts on the first [`current`], which waits for its first
//! answer (at most [`FIRST_ANSWER`]) so the session's first look at the
//! defaults is a real one, not an empty one that reads as a device change a
//! moment later. A lost connection clears the answer (the server cannot be
//! reached, as before) and is retried every [`RETRY_AFTER`]. The thread lives
//! as long as the app.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Condvar, Mutex, MutexGuard, Once, PoisonError};
use std::time::Duration;

use libpulse_binding::context::subscribe::{Facility, InterestMaskSet, Operation};
use libpulse_binding::context::{Context, FlagSet as ContextFlagSet, State};
use libpulse_binding::mainloop::standard::{IterateResult, Mainloop};

use super::activity::{read_error, wait_for_ready};
use super::devices::{Defaults, server_defaults};
use crate::Error;

/// How long the first [`current`] waits for the thread's first answer: a
/// connection and a read, each with its own 2 s limit, and some slack.
const FIRST_ANSWER: Duration = Duration::from_secs(5);

/// How long after a lost or refused connection the thread tries again.
const RETRY_AFTER: Duration = Duration::from_secs(2);

/// The thread's latest answer.
struct Answer {
    /// The thread has finished at least one attempt, read or failed.
    attempted: bool,
    /// The defaults, or `None` while the server cannot be reached.
    defaults: Option<Defaults>,
}

struct Feed {
    answer: Mutex<Answer>,
    answered: Condvar,
}

impl Feed {
    fn lock(&self) -> MutexGuard<'_, Answer> {
        self.answer.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn publish(&self, defaults: Option<Defaults>) {
        let mut answer = self.lock();
        answer.attempted = true;
        answer.defaults = defaults;
        self.answered.notify_all();
    }
}

static FEED: Feed = Feed {
    answer: Mutex::new(Answer {
        attempted: false,
        defaults: None,
    }),
    answered: Condvar::new(),
};

static START: Once = Once::new();

/// The server's default sink and source as the connection last reported
/// them. `None` when the server cannot be reached.
pub(super) fn current() -> Option<Defaults> {
    START.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("meet-ai-pulse-defaults".into())
            .spawn(run);
        if let Err(error) = spawned {
            tracing::warn!(%error, "could not start the default-device thread");
            FEED.publish(None);
        }
    });
    let answer = FEED.lock();
    let (answer, _) = FEED
        .answered
        .wait_timeout_while(answer, FIRST_ANSWER, |answer| !answer.attempted)
        .unwrap_or_else(PoisonError::into_inner);
    answer.defaults.clone()
}

fn run() {
    loop {
        if let Err(error) = follow() {
            tracing::debug!("default-device connection: {error}");
        }
        FEED.publish(None);
        std::thread::sleep(RETRY_AFTER);
    }
}

/// Connect and [`serve`] until the connection is lost.
fn follow() -> Result<(), Error> {
    let mut mainloop =
        Mainloop::new().ok_or_else(|| read_error("could not create a PulseAudio main loop"))?;
    let mut context = Context::new(&mainloop, "meet-ai")
        .ok_or_else(|| read_error("could not create a PulseAudio context"))?;
    context
        .connect(None, ContextFlagSet::NOAUTOSPAWN, None)
        .map_err(|error| read_error(format!("could not reach the sound server: {error}")))?;
    let result = serve(&mut mainloop, &mut context);
    context.disconnect();
    result
}

/// Whether an event can change the default sink or source: a server change
/// (a new default), or a sink or source added or removed.
fn may_change_defaults(facility: Option<Facility>, operation: Option<Operation>) -> bool {
    match facility {
        Some(Facility::Server) => true,
        Some(Facility::Sink | Facility::Source) => {
            matches!(operation, Some(Operation::New | Operation::Removed))
        }
        _ => false,
    }
}

/// Read the defaults, then wait for change events and read them again after
/// each, until the connection fails.
fn serve(mainloop: &mut Mainloop, context: &mut Context) -> Result<(), Error> {
    wait_for_ready(mainloop, context)?;
    let changed = Rc::new(Cell::new(false));
    let flag = Rc::clone(&changed);
    context.set_subscribe_callback(Some(Box::new(
        move |facility: Option<Facility>, operation: Option<Operation>, _index: u32| {
            if may_change_defaults(facility, operation) {
                flag.set(true);
            }
        },
    )));
    // Subscribed before the first read, so a change during it is not
    // missed. Its own answer does not matter: the operation is dropped, and
    // a refused subscribe only means no re-read until the next connection.
    drop(context.subscribe(
        InterestMaskSet::SERVER | InterestMaskSet::SINK | InterestMaskSet::SOURCE,
        |_| {},
    ));
    loop {
        FEED.publish(Some(server_defaults(mainloop, context)?));
        while !changed.replace(false) {
            match mainloop.iterate(true) {
                IterateResult::Success(_) => {}
                IterateResult::Quit(code) => {
                    return Err(read_error(format!(
                        "the PulseAudio main loop quit: {code:?}"
                    )));
                }
                IterateResult::Err(error) => {
                    return Err(read_error(format!(
                        "the PulseAudio main loop failed: {error:?}"
                    )));
                }
            }
            if context.get_state() != State::Ready {
                return Err(read_error("the sound server connection closed"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_default_changing_events_ask_for_a_read() {
        assert!(may_change_defaults(
            Some(Facility::Server),
            Some(Operation::Changed)
        ));
        assert!(may_change_defaults(
            Some(Facility::Sink),
            Some(Operation::New)
        ));
        assert!(may_change_defaults(
            Some(Facility::Source),
            Some(Operation::Removed)
        ));
        // A volume change, a stream: not a new default.
        assert!(!may_change_defaults(
            Some(Facility::Sink),
            Some(Operation::Changed)
        ));
        assert!(!may_change_defaults(
            Some(Facility::SinkInput),
            Some(Operation::New)
        ));
        assert!(!may_change_defaults(None, None));
    }
}
