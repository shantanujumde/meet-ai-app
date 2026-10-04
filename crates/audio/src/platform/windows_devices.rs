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
//! * Followed output (TUR-95): the output read is not only the default. A
//!   call app may play through another endpoint (a headset that is not the
//!   default), so [`default_output_device`] reads which render endpoint other
//!   apps are playing to (`windows/render_in_use.rs`, the choice in
//!   `windows_render_choice.rs`) and runs *that* id through the same
//!   [`DeviceWatch`]. The loopback opens whatever the watch follows
//!   ([`followed_output_endpoint`]).
//! * COM: kept loaded for the life of the process ([`keep_com_loaded`]), so
//!   `cpal`'s process-wide device enumerator never outlives it.

use std::sync::{Mutex, Once, PoisonError};

use cpal::InputCallbackInfo;
use cpal::traits::{DeviceTrait, HostTrait};
use windows::Win32::System::Com::CoIncrementMTAUsage;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

use super::windows::render_endpoint_to_follow;
use crate::loopback::clock::qpc_to_ns;
use crate::loopback::follower::{DeviceWatch, endpoint_key};
use crate::{AudioSource, Error};

/// A default endpoint, as [`endpoint_key`] of its WASAPI endpoint id.
pub(crate) type DeviceId = u64;

static OUTPUT_WATCH: Mutex<DeviceWatch> = Mutex::new(DeviceWatch::new());
static INPUT_WATCH: Mutex<DeviceWatch> = Mutex::new(DeviceWatch::new());

/// Keeps COM loaded in this process from the first call on, until it exits.
///
/// `cpal` 0.18.2 creates one `IMMDeviceEnumerator` for the whole process
/// (the `ENUMERATOR` static in `host/wasapi/device.rs`), while every thread
/// that uses it initializes COM for itself and uninitializes it when the
/// thread ends, as `ComGuard` does after each session read. Once the last
/// thread with COM uninitializes, COM shuts down and unloads `MMDevAPI.dll`,
/// and the next default-device read through that enumerator is an access
/// violation (the Windows `audio --lib` crash, TUR-95). One MTA usage count,
/// never released, keeps COM up however threads come and go. Must run
/// before `cpal` first reads a device: [`host`], [`mic_source`],
/// [`start_sound`] and `ComGuard` call it.
pub(crate) fn keep_com_loaded() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: takes no pointers. The cookie is deliberately never handed
        // to `CoDecrementMTAUsage`, so the count lasts until the process ends.
        if let Err(e) = unsafe { CoIncrementMTAUsage() } {
            tracing::warn!("could not keep COM loaded for the audio devices: {e}");
        }
    });
}

/// `cpal`'s host, with COM kept loaded first ([`keep_com_loaded`]). Every
/// `cpal` device read in the Windows platform code starts here.
pub(crate) fn host() -> cpal::Host {
    keep_com_loaded();
    cpal::default_host()
}

/// The shared `cpal` microphone (`other.rs`), with COM kept loaded first.
pub(crate) fn mic_source() -> Box<dyn AudioSource> {
    keep_com_loaded();
    super::other::mic_source()
}

/// The shared start sound, played through `cpal`, with COM kept loaded first.
pub(crate) fn start_sound() {
    keep_com_loaded();
    super::other::start_sound();
}

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

/// The render endpoint to follow now: the one other apps are playing to
/// (TUR-95), or the default when the session read fails.
fn output_endpoint_read() -> Option<String> {
    render_endpoint_to_follow().or_else(|| endpoint_id(host().default_output_device()))
}

/// The id of the render endpoint the loopback follows, once a switch is
/// confirmed; what [`default_output_device`] reports a key of.
pub(crate) fn followed_output_endpoint() -> Option<String> {
    let mut watch = OUTPUT_WATCH.lock().unwrap_or_else(PoisonError::into_inner);
    watch.poll(host_now_ns(), output_endpoint_read)
}

/// The id of `cpal`'s default render endpoint right now, unwatched.
pub(crate) fn current_default_output_endpoint() -> Option<String> {
    endpoint_id(host().default_output_device())
}

/// The render endpoint the loopback follows (the default, or the one other
/// apps play to), once a switch is confirmed.
pub(crate) fn default_output_device() -> Result<DeviceId, Error> {
    watch(&OUTPUT_WATCH, output_endpoint_read)
}

/// The default capture endpoint the microphone follows, the same way.
pub(crate) fn default_input_device() -> Result<DeviceId, Error> {
    watch(&INPUT_WATCH, || endpoint_id(host().default_input_device()))
}

#[cfg(test)]
mod tests {
    use windows::Win32::System::Com::{
        APTTYPE, APTTYPE_MTA, APTTYPEQUALIFIER, APTTYPEQUALIFIER_IMPLICIT_MTA, CoGetApartmentType,
    };

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

    #[test]
    fn com_stays_loaded_once_the_devices_are_read() {
        keep_com_loaded();
        // A thread that never initialized COM is in the implicit MTA only
        // while some MTA usage count is held; with none, it has no COM.
        let apartment = std::thread::spawn(|| {
            let mut kind = APTTYPE::default();
            let mut qualifier = APTTYPEQUALIFIER::default();
            // SAFETY: both pointers are to live locals the call writes.
            unsafe { CoGetApartmentType(&mut kind, &mut qualifier) }.map(|()| (kind, qualifier))
        })
        .join()
        .expect("the apartment read does not panic");
        assert_eq!(apartment, Ok((APTTYPE_MTA, APTTYPEQUALIFIER_IMPLICIT_MTA)));
    }

    #[test]
    fn device_reads_on_threads_that_come_and_go_do_not_crash() {
        // The TUR-95 crash: one thread reads (cpal caches its enumerator),
        // ends, and the next thread's read went through an unloaded
        // MMDevAPI.dll. Each read here runs on a fresh thread, after a
        // render-session read that initializes and uninitializes COM.
        for _ in 0..4 {
            std::thread::spawn(|| {
                let output = default_output_device();
                let default = current_default_output_endpoint();
                let input = default_input_device();
                eprintln!("output {output:?}, default {default:?}, input {input:?}");
            })
            .join()
            .expect("a device read does not panic");
        }
    }
}
