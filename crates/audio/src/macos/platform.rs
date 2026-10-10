//! The macOS side of [`crate::platform`]: the functions the rest of the crate
//! reaches through that seam, each a thin call into the Core Audio and
//! AVFoundation code in this folder.

use crate::activity::DeviceActivity;
use crate::permission_check::{self, ChannelResult, ChannelState};
use crate::{AudioSource, Error};

pub(crate) use super::device_watch::{default_input_device, default_output_device};
pub(crate) use super::input_devices::input_devices;

/// A Core Audio device id, as [`default_output_device`] returns it.
pub(crate) type DeviceId = objc2_core_audio::AudioObjectID;

/// [`device_activity`] gives a real reading here.
#[cfg(test)]
pub(crate) const DEVICE_ACTIVITY: bool = true;

/// Property reads need no sound server here.
#[cfg(test)]
pub(crate) const DEVICE_ACTIVITY_NEEDS_SERVER: bool = false;

/// The resampler's f32 golden hashes (`resample.rs` tests) were taken here.
#[cfg(test)]
pub(crate) const F32_GOLDEN_HASHES: bool = true;

/// Host time "now", in the same clock domain as the process tap's
/// `AudioTimeStamp.mHostTime` converted via `AudioConvertHostTimeToNanos`
/// (contract revision 3, §0: "mach_absolute_time, converted to ns via
/// mach_timebase_info"). `AudioGetCurrentHostTime` returns exactly the value
/// `mach_absolute_time()` would — same underlying counter — so calling it
/// from the mic callback keeps both channels' anchors comparable without
/// needing to reconcile two different clock epochs. `cpal` does not document
/// its per-callback `InputCallbackInfo` timestamp as sharing that domain, so
/// [`input_callback_ns`] only takes it when it is plausible against this.
pub(crate) fn host_now_ns() -> u64 {
    // SAFETY: both calls read current host-clock state; neither takes a
    // pointer or has a precondition beyond "the audio HAL is initialized",
    // which it is by the time any cpal stream callback can run.
    unsafe {
        objc2_core_audio::AudioConvertHostTimeToNanos(objc2_core_audio::AudioGetCurrentHostTime())
    }
}

/// When the microphone callback's first frame was captured, on the tap's
/// clock (TUR-151). Before, the callback stamped itself with
/// [`host_now_ns`] when it ran, later than the capture by at least the
/// packet, while the tap stamps `inInputTime`, its capture time.
///
/// `cpal` 0.18.2's Core Audio input callback builds `callback` from the
/// callback's `AudioTimeStamp.mHostTime` scaled by `mach_timebase_info`
/// (`src/host/coreaudio/mod.rs` `host_time_to_stream_instant`,
/// `macos/device.rs` input callback), the same ns domain as
/// `AudioConvertHostTimeToNanos`. Its `capture` takes a further latency
/// estimate off that, which the tap's time does not, so `callback` is used.
/// That this time is the first frame's capture is to be verified on
/// hardware (`docs/manual-checks/worktree-tur151.md`); a time that is not
/// plausible against [`host_now_ns`] is ignored by the caller
/// (`crate::capture_clock::first_frame_ns`), which then falls back to the
/// callback's own time less the packet.
pub(crate) fn input_callback_ns(info: &cpal::InputCallbackInfo) -> Option<u64> {
    let ns = u64::try_from(info.timestamp().callback.as_nanos()).ok()?;
    (ns != 0).then_some(ns)
}

/// The Core Audio process tap.
pub(crate) fn system_source() -> Option<Box<dyn AudioSource>> {
    Some(Box::new(super::tap::SystemSource::new()))
}

/// The `cpal` microphone.
pub(crate) fn mic_source() -> Box<dyn AudioSource> {
    Box::new(crate::mic::MicSource::new())
}

/// Core Audio's "is anyone running this device" flags.
pub(crate) fn device_activity() -> Result<DeviceActivity, Error> {
    super::activity::read()
}

/// The Core Audio process list, named (TUR-142).
pub(crate) fn mic_users() -> crate::mic_users::MicUsers {
    super::activity::mic_users()
}

/// Ask macOS directly whether the microphone is authorized, without opening
/// any stream.
///
/// `AVCaptureDevice.authorizationStatus(for: .audio)` is a public, documented,
/// synchronous TCC query — a different mechanism entirely from the process
/// tap's `OSStatus`/sample-payload path that FINDINGS §10.1 proved lies on
/// denial. It reads the same `kTCCServiceMicrophone` record `cpal`'s
/// CoreAudio path is ultimately gated by, for this same process, so a
/// `Denied`/`Restricted` answer here is authoritative.
fn mic_authorization_status() -> objc2_av_foundation::AVAuthorizationStatus {
    use objc2_av_foundation::{AVCaptureDevice, AVMediaTypeAudio};
    // SAFETY: `AVMediaTypeAudio` is an Apple-provided static that is always
    // present once the AVFoundation image is loaded; `authorizationStatusForMediaType`
    // reads TCC state and has no other preconditions.
    unsafe {
        let media_type =
            // quality: allow-unwrap moved as is from permission_check.rs; the static is always set
            AVMediaTypeAudio.expect("AVFoundation always provides the AVMediaTypeAudio constant");
        AVCaptureDevice::authorizationStatusForMediaType(media_type)
    }
}

/// A stored `Denied`/`Restricted` microphone decision, if there is one.
///
/// `None` covers `Authorized` and `NotDetermined` alike: neither says anything
/// about the mic until a stream is actually opened.
pub(crate) fn stored_mic_denial() -> Option<ChannelResult> {
    use objc2_av_foundation::AVAuthorizationStatus;
    let status = mic_authorization_status();
    if status == AVAuthorizationStatus::Denied {
        return Some(ChannelResult {
            state: ChannelState::Denied,
            detail: "macOS reports the microphone permission as explicitly denied \
                     (AVAuthorizationStatusDenied)"
                .into(),
        });
    }
    if status == AVAuthorizationStatus::Restricted {
        return Some(ChannelResult {
            state: ChannelState::Denied,
            detail: "macOS reports the microphone as restricted (parental controls or an \
                     MDM profile), which this client cannot change"
                .into(),
        });
    }
    None
}

/// Open the `cpal` microphone briefly.
pub(crate) fn check_mic() -> ChannelResult {
    permission_check::check_mic_with(mic_source())
}

/// The chime closed loop against the process tap.
pub(crate) fn check_system() -> ChannelResult {
    permission_check::check_system_with(Box::new(super::tap::SystemSource::new()))
}

/// The chime already sounded as [`check_system`]'s positive control.
pub(crate) fn start_sound() {}

/// A denied process tap delivers bit-exact zeros (FINDINGS §10.1), so only
/// the chime can tell a denial from a quiet Mac, and the check that runs
/// during a recording listens for it (TUR-136).
pub(crate) const SILENT_SYSTEM_DENIAL: bool = true;
