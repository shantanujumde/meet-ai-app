//! Is anything using the default mic or speakers right now? (TUR-31)
//!
//! The one entry point for the audio-activity meeting signal, on every
//! platform: [`device_activity`]. The reads themselves are OS code and live in
//! `crate::macos::activity`, reached through `crate::platform` (SPEC §8.2);
//! elsewhere this returns
//! [`Error::Unsupported`], the way [`crate::mic`] and
//! [`crate::permission_check`] do, so callers need no platform checks.
//!
//! macOS reads device properties. Windows and Linux have no such property, so
//! they list the apps' audio streams instead (WASAPI sessions, PulseAudio or
//! PipeWire stream lists) and turn them into a reading with the shared
//! [`activity_from_streams`]: an active stream that is not meet-ai's own
//! counts (TUR-60).

use crate::Error;

/// One reading of the default devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeviceActivity {
    /// Some process is running I/O on the default input device (a mic).
    pub input_running: bool,
    /// Some process is running I/O on the default output device.
    pub output_running: bool,
}

/// Whether the current default input and output devices are in use by any
/// process. Property reads only: no stream, no tap, no permission prompt.
/// [`Error::Unsupported`] on a platform without an implementation yet.
pub fn device_activity() -> Result<DeviceActivity, Error> {
    crate::platform::device_activity()
}

/// One app's audio stream, as the OS's stream list reports it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AppStream {
    /// The process that owns it, when the OS says. `0` is the system itself
    /// (Windows' system-sounds session), never an app on a call.
    pub pid: Option<u32>,
    /// The program's name, for logs only: `Zoom.exe`, `zoom`.
    pub name: String,
    /// The stream is running now: a WASAPI session in the `Active` state, an
    /// uncorked PulseAudio stream.
    pub active: bool,
}

/// The streams in `streams` that count as another app using the device:
/// running, owned by an app, and not by `own_pid` (meet-ai's own recording).
/// A stream with no pid counts: nothing says it is ours.
pub fn other_apps(streams: &[AppStream], own_pid: u32) -> impl Iterator<Item = &AppStream> {
    streams
        .iter()
        .filter(move |stream| stream.active && !matches!(stream.pid, Some(0)))
        .filter(move |stream| stream.pid != Some(own_pid))
}

/// A reading from the capture (mic) and render (speaker) streams: each side is
/// running when [`other_apps`] finds one there.
pub fn activity_from_streams(
    capture: &[AppStream],
    render: &[AppStream],
    own_pid: u32,
) -> DeviceActivity {
    DeviceActivity {
        input_running: other_apps(capture, own_pid).next().is_some(),
        output_running: other_apps(render, own_pid).next().is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whatever this machine is doing, the call answers: a reading on macOS,
    /// `Unsupported` elsewhere — never a device error for a healthy machine.
    #[test]
    fn device_activity_answers_on_this_machine() {
        let result = device_activity();
        if crate::platform::DEVICE_ACTIVITY_NEEDS_SERVER {
            // Windows and Linux ask the sound server, which a CI runner may
            // not have: an error is fine there, `Unsupported` is not.
            assert!(!matches!(result, Err(Error::Unsupported)), "{result:?}");
        } else if crate::platform::DEVICE_ACTIVITY {
            assert!(result.is_ok(), "{result:?}");
        } else {
            assert!(matches!(result, Err(Error::Unsupported)), "{result:?}");
        }
    }

    const OURS: u32 = 4242;

    fn stream(pid: Option<u32>, name: &str, active: bool) -> AppStream {
        AppStream {
            pid,
            name: name.to_string(),
            active,
        }
    }

    #[test]
    fn another_apps_active_streams_make_a_reading() {
        let capture = [stream(Some(10), "Zoom.exe", true)];
        let render = [stream(Some(10), "Zoom.exe", true)];
        assert_eq!(
            activity_from_streams(&capture, &render, OURS),
            DeviceActivity {
                input_running: true,
                output_running: true,
            }
        );
    }

    #[test]
    fn our_own_recording_is_not_a_call() {
        let capture = [stream(Some(OURS), "meet-ai", true)];
        let render = [stream(Some(10), "chrome", true)];
        assert_eq!(
            activity_from_streams(&capture, &render, OURS),
            DeviceActivity {
                input_running: false,
                output_running: true,
            }
        );
        // Ours plus a real call: the call still counts.
        let capture = [
            stream(Some(OURS), "meet-ai", true),
            stream(Some(11), "zoom", true),
        ];
        let names: Vec<&str> = other_apps(&capture, OURS)
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, ["zoom"]);
    }

    #[test]
    fn idle_and_system_streams_do_not_count() {
        let capture = [
            stream(Some(10), "Zoom.exe", false),
            stream(Some(0), "System Sounds", true),
        ];
        assert_eq!(
            activity_from_streams(&capture, &capture, OURS),
            DeviceActivity::default()
        );
        assert_eq!(
            activity_from_streams(&[], &[], OURS),
            DeviceActivity::default()
        );
    }

    #[test]
    fn a_stream_without_a_pid_counts() {
        let capture = [stream(None, "", true)];
        assert!(activity_from_streams(&capture, &[], OURS).input_running);
    }
}
