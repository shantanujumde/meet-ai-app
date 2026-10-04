//! The default output on Linux (TUR-65): the default sink's active port,
//! `device.bus`, `device.form_factor` and Bluetooth profile, over the
//! PulseAudio protocol (PipeWire answers it through pipewire-pulse, as
//! `activity.rs` and `devices.rs` read). Never starts a sound server
//! (`NOAUTOSPAWN`): with none running, the read is an error and the warning
//! stays off.

// Adapted from github.com/fastrepl/anarlog/crates/audio-device/src/linux.rs @ 93deb8642e75a0a2f8ece1bed186da4362213edd (MIT)

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use libpulse_binding::callbacks::ListResult;
use libpulse_binding::context::introspect::SinkInfo;
use libpulse_binding::context::{Context, FlagSet as ContextFlagSet};
use libpulse_binding::mainloop::standard::Mainloop;

use super::activity::{iterate, read_error, wait_for_ready};
use crate::Error;
use crate::headphones::OutputDevice;
use crate::platform::headphones::parse::{PulseSink, pulse_form, pulse_transport};

/// The longest the sink read may take.
const READ_TIMEOUT: Duration = Duration::from_secs(2);

/// The server's default sink, or `None` when it has none.
pub(crate) fn default_output_info() -> Result<Option<OutputDevice>, Error> {
    let mut mainloop =
        Mainloop::new().ok_or_else(|| read_error("could not create a PulseAudio main loop"))?;
    let mut context = Context::new(&mainloop, "meet-ai")
        .ok_or_else(|| read_error("could not create a PulseAudio context"))?;
    context
        .connect(None, ContextFlagSet::NOAUTOSPAWN, None)
        .map_err(|error| read_error(format!("could not reach the sound server: {error}")))?;
    let result = default_sink(&mut mainloop, &context);
    context.disconnect();
    Ok(result?.map(|sink| OutputDevice {
        name: sink
            .description
            .clone()
            .unwrap_or_else(|| sink.name.clone()),
        transport: pulse_transport(&sink),
        form: pulse_form(&sink),
    }))
}

/// The sink lookup as its callback fills it in.
#[derive(Default)]
struct Lookup {
    sink: Option<PulseSink>,
    completed: bool,
    failed: bool,
}

fn default_sink(mainloop: &mut Mainloop, context: &Context) -> Result<Option<PulseSink>, Error> {
    wait_for_ready(mainloop, context)?;
    let lookup = Rc::new(RefCell::new(Lookup::default()));
    let into = Rc::clone(&lookup);
    let mut operation =
        context
            .introspect()
            .get_sink_info_by_name("@DEFAULT_SINK@", move |result| {
                let mut lookup = into.borrow_mut();
                match result {
                    ListResult::Item(info) => lookup.sink = Some(sink_from(info)),
                    ListResult::End => lookup.completed = true,
                    ListResult::Error => {
                        lookup.completed = true;
                        lookup.failed = true;
                    }
                }
            });
    let deadline = Instant::now() + READ_TIMEOUT;
    loop {
        {
            let mut lookup = lookup.borrow_mut();
            if lookup.completed {
                // No default sink is an error from the server, not a failure
                // to read: there is simply nothing to warn about.
                return Ok(if lookup.failed {
                    None
                } else {
                    lookup.sink.take()
                });
            }
        }
        let step = if Instant::now() >= deadline {
            Err(read_error("timed out reading the default sink"))
        } else {
            iterate(mainloop, "reading the default sink")
        };
        if let Err(error) = step {
            operation.cancel();
            return Err(error);
        }
    }
}

fn sink_from(info: &SinkInfo<'_>) -> PulseSink {
    let props = &info.proplist;
    PulseSink {
        name: info.name.as_deref().unwrap_or_default().to_owned(),
        description: info.description.as_deref().map(str::to_owned),
        active_port: info
            .active_port
            .as_ref()
            .and_then(|port| port.name.as_deref())
            .map(str::to_owned),
        bus: props.get_str("device.bus"),
        form_factor: props.get_str("device.form_factor"),
        bluetooth_profile: props
            .get_str("bluetooth.protocol")
            .or_else(|| props.get_str("api.bluez5.profile")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_output_reads_or_fails_without_panicking() {
        // A CI runner usually has no sound server; either answer is fine.
        let _ = default_output_info();
    }
}
