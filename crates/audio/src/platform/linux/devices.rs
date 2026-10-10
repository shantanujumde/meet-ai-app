//! The Linux default-device watch (TUR-38).
//!
//! `cpal`'s PipeWire host records the default sink through a virtual
//! `sink_default` device whose name never changes, and PipeWire moves the
//! stream to a new default by itself, so the session would never hear of a
//! headset plugged in. The sound server's own idea of the default sink and
//! source is read here instead, over the PulseAudio protocol (PipeWire
//! answers it through pipewire-pulse, `linux/activity.rs` reads the same
//! way), and run through [`DeviceWatch`] as on Windows: a look every
//! `DEVICE_CHECK_INTERVAL`, a switch once two looks agree. The looks cost
//! nothing: one connection, subscribed to the server's change events, keeps
//! the answer current (`linux/server_watch.rs`, TUR-172).
//!
//! A sink removed under a running stream (`cpal`'s `DeviceNotAvailable`)
//! also counts: the loopback's error callback calls [`note_stream_lost`],
//! and the output id carries a [`LossWatch`] generation, so the session sees
//! a changed device and calls `reopen_segment` either way.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use libpulse_binding::context::Context;
use libpulse_binding::mainloop::standard::Mainloop;

use super::activity::{iterate, read_error, wait_for_ready};
use super::clock::host_now_ns;
use super::server_watch;
use crate::Error;
use crate::loopback::follower::{DeviceWatch, LossWatch, endpoint_key};

/// A default device: [`endpoint_key`] of its sound-server name, and (for the
/// output) how many lost streams have been reported so far.
pub(crate) type DeviceId = (u64, u64);

static OUTPUT_WATCH: Mutex<DeviceWatch> = Mutex::new(DeviceWatch::new());
static INPUT_WATCH: Mutex<DeviceWatch> = Mutex::new(DeviceWatch::new());
static LOSS_WATCH: Mutex<LossWatch> = Mutex::new(LossWatch::new());
static STREAM_LOSSES: AtomicU64 = AtomicU64::new(0);

/// The longest the server-info read may take.
const READ_TIMEOUT: Duration = Duration::from_secs(2);

/// The sound server's default sink and source names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Defaults {
    sink: Option<String>,
    source: Option<String>,
}

/// A capture stream died under us (its device went away); the next output
/// read reports a new generation.
pub(super) fn note_stream_lost() {
    STREAM_LOSSES.fetch_add(1, Ordering::Relaxed);
}

/// The default sink the loopback follows, once a switch is confirmed.
pub(crate) fn default_output_device() -> Result<DeviceId, Error> {
    let key = watch(&OUTPUT_WATCH, |d| d.sink)?;
    let generation = LOSS_WATCH
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .poll(host_now_ns(), STREAM_LOSSES.load(Ordering::Relaxed));
    Ok((key, generation))
}

/// The default source the microphone follows, the same way.
pub(crate) fn default_input_device() -> Result<DeviceId, Error> {
    Ok((watch(&INPUT_WATCH, |d| d.source)?, 0))
}

fn watch(cell: &Mutex<DeviceWatch>, pick: fn(Defaults) -> Option<String>) -> Result<u64, Error> {
    let mut watch = cell.lock().unwrap_or_else(PoisonError::into_inner);
    watch
        .poll(host_now_ns(), || server_watch::current().and_then(pick))
        .map(|name| endpoint_key(&name))
        .ok_or_else(|| Error::NoDevice("no default sound-server device".into()))
}

/// One read of the server's defaults over `context`'s connection.
// Adapted from github.com/fastrepl/anarlog/crates/audio-actual/src/speaker/linux.rs @ 93deb8642e75a0a2f8ece1bed186da4362213edd (MIT)
pub(super) fn server_defaults(
    mainloop: &mut Mainloop,
    context: &Context,
) -> Result<Defaults, Error> {
    wait_for_ready(mainloop, context)?;
    let answer: Rc<RefCell<Option<Defaults>>> = Rc::new(RefCell::new(None));
    let into = Rc::clone(&answer);
    let mut operation = context.introspect().get_server_info(move |info| {
        *into.borrow_mut() = Some(Defaults {
            sink: info.default_sink_name.as_ref().map(|name| name.to_string()),
            source: info
                .default_source_name
                .as_ref()
                .map(|name| name.to_string()),
        });
    });
    let deadline = Instant::now() + READ_TIMEOUT;
    loop {
        if let Some(read) = answer.borrow_mut().take() {
            return Ok(read);
        }
        let step = if Instant::now() >= deadline {
            Err(read_error("timed out reading the default devices"))
        } else {
            iterate(mainloop, "reading the default devices")
        };
        if let Err(error) = step {
            operation.cancel();
            return Err(error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_device_reads_answer_or_fail_without_panicking() {
        // A CI runner usually has no sound server; either answer is fine.
        let output = default_output_device();
        let input = default_input_device();
        eprintln!("default output: {output:?}, default input: {input:?}");
        if let Ok(id) = output {
            assert_eq!(
                default_output_device().ok(),
                Some(id),
                "stable between reads"
            );
        }
    }
}
