//! The Windows host clock and default-device watch (TUR-37).
//!
//! * Clock: `QueryPerformanceCounter`, on the same 100 ns grid `cpal` puts
//!   WASAPI's capture times on, so [`host_now_ns`], the microphone's
//!   [`input_callback_ns`] and the loopback's packet times are one domain and
//!   the two tracks' anchors can be compared (contract §11).
//! * Device watch: `cpal` 0.18.2 opens default-device streams on the virtual
//!   default endpoint, so Windows reroutes them by itself when the default
//!   changes (a headset plugged in), and the session would never hear of it.
//!   [`default_output_device`] and [`default_input_device`] read the current
//!   default endpoint's id through `cpal` and run it through
//!   [`DeviceWatch`]: a read every `DEVICE_CHECK_INTERVAL`, a switch once two
//!   reads agree. The session's tick then reopens the segment as on macOS.

use std::sync::{Mutex, PoisonError};

use cpal::InputCallbackInfo;
use cpal::traits::{DeviceTrait, HostTrait};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

use crate::Error;
use crate::loopback::clock::qpc_to_ns;
use crate::loopback::follower::{DeviceWatch, endpoint_key};

/// A default endpoint, as [`endpoint_key`] of its WASAPI endpoint id.
pub(crate) type DeviceId = u64;

static OUTPUT_WATCH: Mutex<DeviceWatch> = Mutex::new(DeviceWatch::new());
static INPUT_WATCH: Mutex<DeviceWatch> = Mutex::new(DeviceWatch::new());

/// The performance counter now, in ns. 0 if the counter cannot be read,
/// which Microsoft documents never happens on Windows XP or later.
pub(crate) fn host_now_ns() -> u64 {
    let mut counter: i64 = 0;
    let mut frequency: i64 = 0;
    // SAFETY: both calls only write the `i64` they are handed.
    let read = unsafe {
        QueryPerformanceFrequency(&mut frequency)
            .and_then(|()| QueryPerformanceCounter(&mut counter))
    };
    if read.is_err() {
        return 0;
    }
    qpc_to_ns(
        u64::try_from(counter).unwrap_or(0),
        u64::try_from(frequency).unwrap_or(0),
    )
}

/// When the callback's first frame was captured: WASAPI's `GetBuffer` QPC
/// position, the clock both tracks' `position()` use on Windows.
pub(crate) fn input_callback_ns(info: &InputCallbackInfo) -> Option<u64> {
    let ns = u64::try_from(info.timestamp().capture.as_nanos()).ok()?;
    (ns != 0).then_some(ns)
}

/// The id of the endpoint `cpal`'s default device resolves to right now.
fn endpoint_id(device: Option<cpal::Device>) -> Option<String> {
    Some(device?.id().ok()?.id().to_string())
}

fn watch(
    cell: &Mutex<DeviceWatch>,
    read: impl FnOnce() -> Option<String>,
) -> Result<DeviceId, Error> {
    let mut watch = cell.lock().unwrap_or_else(PoisonError::into_inner);
    watch
        .poll(host_now_ns(), read)
        .map(|id| endpoint_key(&id))
        .ok_or_else(|| Error::NoDevice("no default audio endpoint".into()))
}

/// The default render endpoint the loopback follows, once a switch is confirmed.
pub(crate) fn default_output_device() -> Result<DeviceId, Error> {
    watch(&OUTPUT_WATCH, || {
        endpoint_id(cpal::default_host().default_output_device())
    })
}

/// The default capture endpoint the microphone follows, the same way.
pub(crate) fn default_input_device() -> Result<DeviceId, Error> {
    watch(&INPUT_WATCH, || {
        endpoint_id(cpal::default_host().default_input_device())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_clock_moves_forward_on_the_100_ns_grid() {
        let a = host_now_ns();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let b = host_now_ns();
        assert!(a > 0, "QueryPerformanceCounter read 0");
        assert!(b >= a + 4_000_000, "{a} then {b}");
        assert_eq!(a % 100, 0);
        assert_eq!(b % 100, 0);
    }

    #[test]
    fn the_device_reads_answer_or_fail_without_panicking() {
        // A CI runner usually has no audio endpoint; either answer is fine.
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
