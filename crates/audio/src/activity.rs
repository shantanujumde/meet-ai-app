//! Is anything using the default mic or speakers right now? (TUR-31)
//!
//! The one entry point for the audio-activity meeting signal, on every
//! platform: [`device_activity`]. The reads themselves are OS code and live in
//! `crate::macos::activity`, reached through `crate::platform` (SPEC §8.2);
//! elsewhere this returns
//! [`Error::Unsupported`], the way [`crate::mic`] and
//! [`crate::permission_check`] do, so callers need no platform checks.

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

#[cfg(test)]
mod tests {
    use super::*;

    /// Whatever this machine is doing, the call answers: a reading on macOS,
    /// `Unsupported` elsewhere — never a device error for a healthy machine.
    #[test]
    fn device_activity_answers_on_this_machine() {
        let result = device_activity();
        if crate::platform::DEVICE_ACTIVITY {
            assert!(result.is_ok(), "{result:?}");
        } else {
            assert!(matches!(result, Err(Error::Unsupported)), "{result:?}");
        }
    }
}
