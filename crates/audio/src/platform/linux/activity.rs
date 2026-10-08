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
//!
//! [`mic_users`] keeps the source outputs instead of only counting them
//! (TUR-142): each running one's pid, binary and `application.name` go to
//! [`crate::mic_users::name_apps`]. With no sound server the answer is
//! [`MicUsers::NotSupported`].

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
use crate::mic_users::{MicUsers, ProcessFacts, name_apps};

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
    let (capture, render) = both_listings()?;
    Ok(reading_without_our_own(&capture.streams, &render.streams))
}

/// The apps with a running source output, named, without meet-ai.
/// [`MicUsers::NotSupported`] when the sound server cannot be read.
pub(crate) fn mic_users() -> MicUsers {
    match both_listings() {
        Ok((capture, render)) => {
            MicUsers::Supported(apps_from(&capture, &render, std::process::id()))
        }
        Err(error) => {
            tracing::debug!(%error, "could not list the apps using a mic");
            MicUsers::NotSupported
        }
    }
}

/// The running source outputs as processes for [`name_apps`]; `playing`
/// when the same pid has a running sink input.
// Adapted from github.com/fastrepl/anarlog/crates/detect/src/list/linux.rs @ 259a04ee2e1447dfed150ed08f0a1bb69909b836 (MIT)
fn apps_from(capture: &Listing, render: &Listing, own_pid: u32) -> Vec<crate::mic_users::MicApp> {
    let playing = |pid: Option<u32>| {
        pid.is_some_and(|pid| {
            crate::activity::other_apps(&render.streams, own_pid).any(|s| s.pid == Some(pid))
        })
    };
    let users = capture
        .streams
        .iter()
        .zip(&capture.labels)
        .filter(|(stream, _)| stream.active && stream.pid != Some(0))
        .map(|(stream, label)| ProcessFacts {
            pid: stream.pid.unwrap_or_default(),
            exe: stream.name.clone(),
            label: label.clone(),
            output: playing(stream.pid),
            ..ProcessFacts::default()
        })
        .collect();
    name_apps(users, own_pid, |_| None)
}

/// Connect, read both stream lists, disconnect.
fn both_listings() -> Result<(Listing, Listing), Error> {
    let mut mainloop =
        Mainloop::new().ok_or_else(|| read_error("could not create a PulseAudio main loop"))?;
    let mut context = Context::new(&mainloop, "meet-ai")
        .ok_or_else(|| read_error("could not create a PulseAudio context"))?;
    context
        .connect(None, ContextFlagSet::NOAUTOSPAWN, None)
        .map_err(|error| read_error(format!("could not reach the sound server: {error}")))?;
    let result = read_streams(&mut mainloop, &context);
    context.disconnect();
    result
}

pub(super) fn read_error(message: impl Into<String>) -> Error {
    Error::DeviceRead(message.into())
}

/// The source outputs (capture) and sink inputs (render), once connected.
fn read_streams(mainloop: &mut Mainloop, context: &Context) -> Result<(Listing, Listing), Error> {
    wait_for_ready(mainloop, context)?;
    let introspect = context.introspect();

    let capture = Rc::new(RefCell::new(Listing::default()));
    let into = Rc::clone(&capture);
    let operation = introspect.get_source_output_info_list(move |result| {
        into.borrow_mut()
            .take(item(result, |info| (info.corked, &info.proplist)))
    });
    wait_for_listing(mainloop, &capture, operation, "source outputs")?;

    let render = Rc::new(RefCell::new(Listing::default()));
    let into = Rc::clone(&render);
    let operation = introspect.get_sink_input_info_list(move |result| {
        into.borrow_mut()
            .take(item(result, |info| (info.corked, &info.proplist)))
    });
    wait_for_listing(mainloop, &render, operation, "sink inputs")?;

    let capture = std::mem::take(&mut *capture.borrow_mut());
    let render = std::mem::take(&mut *render.borrow_mut());
    Ok((capture, render))
}

/// One stream list as its callback fills it in.
#[derive(Default)]
struct Listing {
    streams: Vec<AppStream>,
    /// Each stream's `application.name`, in the same order (TUR-142).
    labels: Vec<Option<String>>,
    completed: bool,
    failed: bool,
}

/// One callback result of a stream list, down to what [`Listing`] keeps.
enum Item<'a> {
    Stream(bool, &'a Proplist),
    End,
    Error,
}

/// One stream-list callback result, down to what [`Listing`] keeps. The same
/// for source outputs and sink inputs.
fn item<'a, T>(result: ListResult<&'a T>, fields: fn(&'a T) -> (bool, &'a Proplist)) -> Item<'a> {
    match result {
        ListResult::Item(info) => {
            let (corked, proplist) = fields(info);
            Item::Stream(corked, proplist)
        }
        ListResult::End => Item::End,
        ListResult::Error => Item::Error,
    }
}

impl Listing {
    /// One callback result: a stream, or the end of the list.
    fn take(&mut self, item: Item<'_>) {
        match item {
            Item::Stream(corked, proplist) => {
                self.streams.push(stream_from(corked, proplist));
                self.labels.push(proplist.get_str("application.name"));
            }
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

pub(super) fn wait_for_ready(mainloop: &mut Mainloop, context: &Context) -> Result<(), Error> {
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

pub(super) fn iterate(mainloop: &mut Mainloop, what: &str) -> Result<(), Error> {
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

    fn listing(streams: Vec<(AppStream, Option<&str>)>) -> Listing {
        let (streams, labels) = streams
            .into_iter()
            .map(|(stream, label)| (stream, label.map(str::to_string)))
            .unzip();
        Listing {
            streams,
            labels,
            completed: true,
            failed: false,
        }
    }

    fn stream(pid: u32, name: &str, active: bool) -> AppStream {
        AppStream {
            pid: Some(pid),
            name: name.to_string(),
            active,
        }
    }

    #[test]
    fn running_source_outputs_become_named_apps_without_ours() {
        let capture = listing(vec![
            (stream(10, "zoom", true), Some("ZOOM VoiceEngine")),
            (stream(11, "chrome", true), Some("Google Chrome")),
            (stream(12, "obs", false), Some("OBS")),
            (stream(4242, "meet-ai", true), None),
        ]);
        let render = listing(vec![(stream(10, "zoom", true), None)]);
        let apps = apps_from(&capture, &render, 4242);
        let named: Vec<(&str, bool)> = apps.iter().map(|a| (a.name.as_str(), a.playing)).collect();
        assert_eq!(named, [("Google Chrome", false), ("Zoom", true)]);
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
