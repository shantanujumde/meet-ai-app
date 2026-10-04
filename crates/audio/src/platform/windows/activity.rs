//! Is another app using a mic or the speakers? WASAPI's answer (TUR-60).
//!
//! Windows has no "device is running somewhere" property like Core Audio's, but
//! every app that opens an endpoint gets an audio session, and a session is
//! `Active` while one of its streams runs. So a reading is: list the sessions
//! on every active capture and render endpoint, keep the `Active` ones, and
//! hand them to [`crate::activity::activity_from_streams`], which drops
//! meet-ai's own pid. Names come from `sysinfo`, for the debug log only.
//!
//! The read is all COM calls on the caller's thread: no stream is opened, so
//! there is no permission prompt and no mic indicator.

use crate::Error;
use crate::activity::{AppStream, DeviceActivity, reading_without_our_own};

/// [`device_activity`] gives a real reading here.
#[cfg(test)]
pub(crate) const DEVICE_ACTIVITY: bool = true;

/// A CI runner may have no audio endpoints or no audio service.
#[cfg(test)]
pub(crate) const DEVICE_ACTIVITY_NEEDS_SERVER: bool = true;

/// COM for this thread, for as long as the read runs.
// Adapted from github.com/fastrepl/anarlog/crates/detect/src/list/windows.rs @ 93deb8642e75a0a2f8ece1bed186da4362213edd (MIT)
pub(super) struct ComGuard {
    /// We initialised COM here, so we uninitialise it. `false` when the
    /// thread already had COM in another apartment: still usable, not ours.
    owned: bool,
}

impl ComGuard {
    pub(super) fn initialize() -> Self {
        // S_OK and S_FALSE both need a matching CoUninitialize;
        // RPC_E_CHANGED_MODE (an STA thread) does not, and COM still works.
        Self {
            owned: wasapi::initialize_mta().is_ok(),
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.owned {
            wasapi::deinitialize();
        }
    }
}

/// Whether another app has an active session on any capture endpoint (a mic)
/// and on any render endpoint (speakers, headphones).
pub(crate) fn device_activity() -> Result<DeviceActivity, Error> {
    let _com = ComGuard::initialize();
    let enumerator = wasapi::DeviceEnumerator::new().map_err(read_error)?;
    let mut capture = sessions(&enumerator, &wasapi::Direction::Capture)?;
    let mut render = sessions(&enumerator, &wasapi::Direction::Render)?;
    if tracing::enabled!(tracing::Level::DEBUG) {
        name_processes(&mut capture);
        name_processes(&mut render);
    }
    Ok(reading_without_our_own(&capture, &render))
}

fn read_error(error: wasapi::WasapiError) -> Error {
    Error::DeviceRead(error.to_string())
}

/// Every session on every active endpoint in `direction`, with its pid and
/// whether it is `Active`. One endpoint or session failing to answer skips
/// just that one; the enumerator failing is an error.
// Adapted from github.com/fastrepl/anarlog/crates/detect/src/list/windows.rs @ 93deb8642e75a0a2f8ece1bed186da4362213edd (MIT)
fn sessions(
    enumerator: &wasapi::DeviceEnumerator,
    direction: &wasapi::Direction,
) -> Result<Vec<AppStream>, Error> {
    let devices = enumerator
        .get_device_collection(direction)
        .map_err(read_error)?;
    let count = devices.get_nbr_devices().map_err(read_error)?;
    let mut streams = Vec::new();
    for index in 0..count {
        let Ok(device) = devices.get_device_at_index(index) else {
            continue;
        };
        let Ok(manager) = device.get_iaudiosessionmanager() else {
            continue;
        };
        let Ok(list) = manager.get_audiosessionenumerator() else {
            continue;
        };
        let Ok(sessions) = list.get_count() else {
            continue;
        };
        for index in 0..sessions {
            let Ok(session) = list.get_session(index) else {
                continue;
            };
            let Ok(pid) = session.get_process_id() else {
                continue;
            };
            let active = matches!(session.get_state(), Ok(wasapi::SessionState::Active));
            streams.push(AppStream {
                pid: Some(pid),
                name: String::new(),
                active,
            });
        }
    }
    Ok(streams)
}

/// Fill in each active stream's process name from `sysinfo`.
// Adapted from github.com/fastrepl/anarlog/crates/detect/src/list/windows.rs @ 93deb8642e75a0a2f8ece1bed186da4362213edd (MIT)
fn name_processes(streams: &mut [AppStream]) {
    let pids: Vec<sysinfo::Pid> = streams
        .iter()
        .filter(|s| s.active)
        .filter_map(|s| s.pid)
        .map(sysinfo::Pid::from_u32)
        .collect();
    if pids.is_empty() {
        return;
    }
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&pids), true);
    for stream in streams.iter_mut() {
        if let Some(process) = stream
            .pid
            .and_then(|pid| system.process(sysinfo::Pid::from_u32(pid)))
        {
            stream.name = process.name().to_string_lossy().into_owned();
        }
    }
}
