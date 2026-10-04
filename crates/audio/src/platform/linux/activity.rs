//! Is another app using a mic or the speakers? The sound server's answer
//! (TUR-60).
//!
//! Linux has no per-device "running somewhere" flag either, but the sound
//! server lists every stream: a *source output* is an app recording from a
//! source (a mic), a *sink input* is an app playing to a sink. An uncorked one
//! is running. Each carries `application.process.id` and
//! `application.process.binary`, so meet-ai's own recording is dropped by pid
//! in [`crate::activity::activity_from_streams`].
//!
//! This speaks the PulseAudio protocol, which PipeWire desktops answer through
//! pipewire-pulse. It never starts a sound server (`NOAUTOSPAWN`): with none
//! running, the read is an error and the detection loop treats it as quiet.

// Adapted from github.com/fastrepl/anarlog/crates/detect/src/list/linux.rs @ 93deb8642e75a0a2f8ece1bed186da4362213edd (MIT)

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use libpulse_binding::callbacks::ListResult;
use libpulse_binding::context::{Context, FlagSet as ContextFlagSet, State};
use libpulse_binding::mainloop::standard::{IterateResult, Mainloop};
use libpulse_binding::operation::Operation;
use libpulse_binding::proplist::Proplist;

use crate::Error;
use crate::activity::{AppStream, DeviceActivity, reading_without_our_own};

/// [`device_activity`] gives a real reading here.
#[cfg(test)]
pub(crate) const DEVICE_ACTIVITY: bool = true;

/// A CI runner has no PulseAudio or PipeWire running.
#[cfg(test)]
pub(crate) const DEVICE_ACTIVITY_NEEDS_SERVER: bool = true;

/// The longest one step (connecting, one list) may take. The detection loop
/// reads every 2 s, so a stuck server costs a reading, not the loop.
const OPERATION_TIMEOUT: Duration = Duration::from_secs(2);

/// How long to sleep when the main loop had nothing to do.
const IDLE_SLEEP: Duration = Duration::from_millis(10);

/// Whether another app has a running stream from a source (a mic) and to a
/// sink (speakers, headphones).
pub(crate) fn device_activity() -> Result<DeviceActivity, Error> {
    let mut mainloop =
        Mainloop::new().ok_or_else(|| read_error("could not create a PulseAudio main loop"))?;
    let mut context = Context::new(&mainloop, "meet-ai")
        .ok_or_else(|| read_error("could not create a PulseAudio context"))?;
    context
        .connect(None, ContextFlagSet::NOAUTOSPAWN, None)
        .map_err(|error| read_error(format!("could not reach the sound server: {error}")))?;
    let result = read_streams(&mut mainloop, &context);
    context.disconnect();
    let (capture, render) = result?;
    Ok(reading_without_our_own(&capture, &render))
}

fn read_error(message: impl Into<String>) -> Error {
    Error::DeviceRead(message.into())
}

/// The source outputs (capture) and sink inputs (render), once connected.
fn read_streams(
    mainloop: &mut Mainloop,
    context: &Context,
) -> Result<(Vec<AppStream>, Vec<AppStream>), Error> {
    wait_for_ready(mainloop, context)?;
    let introspect = context.introspect();

    let capture = Rc::new(RefCell::new(Listing::default()));
    let into = Rc::clone(&capture);
    let operation = introspect.get_source_output_info_list(move |result| {
        into.borrow_mut()
            .take(result.map(|info| (info.corked, &info.proplist)))
    });
    wait_for_listing(mainloop, &capture, operation, "source outputs")?;

    let render = Rc::new(RefCell::new(Listing::default()));
    let into = Rc::clone(&render);
    let operation = introspect.get_sink_input_info_list(move |result| {
        into.borrow_mut()
            .take(result.map(|info| (info.corked, &info.proplist)))
    });
    wait_for_listing(mainloop, &render, operation, "sink inputs")?;

    let capture = std::mem::take(&mut capture.borrow_mut().streams);
    let render = std::mem::take(&mut render.borrow_mut().streams);
    Ok((capture, render))
}

/// One stream list as its callback fills it in.
#[derive(Default)]
struct Listing {
    streams: Vec<AppStream>,
    completed: bool,
    failed: bool,
}

/// One callback result of a stream list, down to what [`Listing`] keeps.
enum Item<'a> {
    Stream(bool, &'a Proplist),
    End,
    Error,
}

/// `ListResult` has no `map`; this is it, for both stream-info types.
trait MapItem<'a, T> {
    fn map(self, f: impl FnOnce(&'a T) -> (bool, &'a Proplist)) -> Item<'a>;
}

impl<'a, T> MapItem<'a, T> for ListResult<&'a T> {
    fn map(self, f: impl FnOnce(&'a T) -> (bool, &'a Proplist)) -> Item<'a> {
        match self {
            ListResult::Item(info) => {
                let (corked, proplist) = f(info);
                Item::Stream(corked, proplist)
            }
            ListResult::End => Item::End,
            ListResult::Error => Item::Error,
        }
    }
}

impl Listing {
    /// One callback result: a stream, or the end of the list.
    fn take(&mut self, item: Item<'_>) {
        match item {
            Item::Stream(corked, proplist) => self.streams.push(stream_from(corked, proplist)),
            Item::End => self.completed = true,
            Item::Error => {
                self.completed = true;
                self.failed = true;
            }
        }
    }
}

/// One stream from its corked flag and property list.
fn stream_from(corked: bool, proplist: &Proplist) -> AppStream {
    AppStream {
        pid: proplist
            .get_str("application.process.id")
            .and_then(|pid| pid.trim().parse().ok()),
        name: proplist
            .get_str("application.process.binary")
            .or_else(|| proplist.get_str("application.name"))
            .unwrap_or_default(),
        active: !corked,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Readiness {
    Pending,
    Ready,
    Failed,
}

fn readiness(state: State) -> Readiness {
    match state {
        State::Ready => Readiness::Ready,
        State::Failed | State::Terminated => Readiness::Failed,
        _ => Readiness::Pending,
    }
}

fn wait_for_ready(mainloop: &mut Mainloop, context: &Context) -> Result<(), Error> {
    let deadline = Instant::now() + OPERATION_TIMEOUT;
    loop {
        match readiness(context.get_state()) {
            Readiness::Ready => return Ok(()),
            Readiness::Failed => return Err(read_error("the sound server refused the connection")),
            Readiness::Pending => {}
        }
        if Instant::now() >= deadline {
            return Err(read_error("timed out connecting to the sound server"));
        }
        iterate(mainloop, "connecting")?;
    }
}

fn wait_for_listing<T: ?Sized>(
    mainloop: &mut Mainloop,
    listing: &RefCell<Listing>,
    mut operation: Operation<T>,
    what: &str,
) -> Result<(), Error> {
    let deadline = Instant::now() + OPERATION_TIMEOUT;
    loop {
        {
            let listing = listing.borrow();
            if listing.completed {
                return if listing.failed {
                    Err(read_error(format!(
                        "the sound server could not list {what}"
                    )))
                } else {
                    Ok(())
                };
            }
        }
        let step = if Instant::now() >= deadline {
            Err(read_error(format!("timed out listing {what}")))
        } else {
            iterate(mainloop, what)
        };
        if let Err(error) = step {
            operation.cancel();
            return Err(error);
        }
    }
}

fn iterate(mainloop: &mut Mainloop, what: &str) -> Result<(), Error> {
    match mainloop.iterate(false) {
        IterateResult::Success(0) => std::thread::sleep(IDLE_SLEEP),
        IterateResult::Success(_) => {}
        IterateResult::Quit(code) => {
            return Err(read_error(format!(
                "the PulseAudio main loop quit while {what}: {code:?}"
            )));
        }
        IterateResult::Err(error) => {
            return Err(read_error(format!(
                "the PulseAudio main loop failed while {what}: {error:?}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_ready_context_is_ready() {
        for state in [
            State::Unconnected,
            State::Connecting,
            State::Authorizing,
            State::SettingName,
        ] {
            assert_eq!(readiness(state), Readiness::Pending);
        }
        assert_eq!(readiness(State::Ready), Readiness::Ready);
        assert_eq!(readiness(State::Failed), Readiness::Failed);
        assert_eq!(readiness(State::Terminated), Readiness::Failed);
    }

    #[test]
    fn a_stream_takes_its_pid_and_binary_from_the_proplist() {
        let mut props = Proplist::new().expect("a proplist");
        props
            .set_str("application.process.id", "1234")
            .expect("sets");
        props
            .set_str("application.process.binary", "zoom")
            .expect("sets");
        props.set_str("application.name", "Zoom").expect("sets");
        assert_eq!(
            stream_from(false, &props),
            AppStream {
                pid: Some(1234),
                name: "zoom".to_string(),
                active: true,
            }
        );
        assert!(!stream_from(true, &props).active);
    }

    #[test]
    fn a_stream_without_process_properties_still_counts() {
        let props = Proplist::new().expect("a proplist");
        assert_eq!(
            stream_from(false, &props),
            AppStream {
                pid: None,
                name: String::new(),
                active: true,
            }
        );
    }
}
