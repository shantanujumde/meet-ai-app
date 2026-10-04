//! Which render endpoint other apps are playing to, from WASAPI's audio
//! sessions (TUR-95).
//!
//! Every app that opens a render endpoint gets an audio session on it
//! (`IAudioSessionManager2` / `IAudioSessionEnumerator`), `Active` while one
//! of its streams runs. One read lists the sessions on every active render
//! endpoint and hands them to the pure
//! [`super::super::windows_render_choice::endpoint_to_follow`], which picks
//! the endpoint to record. No stream is opened: no prompt, no indicator.

use super::super::windows_render_choice::{RenderEndpoint, endpoint_to_follow};
use super::activity::ComGuard;
use crate::activity::AppStream;

// Adapted from github.com/fastrepl/anarlog/crates/audio-actual/src/speaker/windows.rs @ 93deb8642e75a0a2f8ece1bed186da4362213edd (MIT)
/// The id of the render endpoint the loopback should follow right now, or
/// `None` when the read failed or there is nothing to follow (no default and
/// no single endpoint playing).
pub(crate) fn render_endpoint_to_follow() -> Option<String> {
    let _com = ComGuard::initialize();
    let enumerator = match wasapi::DeviceEnumerator::new() {
        Ok(enumerator) => enumerator,
        Err(e) => {
            tracing::debug!("render endpoint read: {e}");
            return None;
        }
    };
    let default = enumerator
        .get_default_device(&wasapi::Direction::Render)
        .and_then(|device| device.get_id())
        .ok();
    let endpoints = render_endpoints(&enumerator);
    endpoint_to_follow(default.as_deref(), &endpoints, std::process::id())
}

/// Every active render endpoint with its sessions. An endpoint or session
/// that fails to answer is skipped (an endpoint without its sessions counts
/// as not playing); the collection failing gives an empty list, which
/// follows the default.
// Adapted from github.com/fastrepl/anarlog/crates/detect/src/list/windows.rs @ 93deb8642e75a0a2f8ece1bed186da4362213edd (MIT)
fn render_endpoints(enumerator: &wasapi::DeviceEnumerator) -> Vec<RenderEndpoint> {
    let Ok(devices) = enumerator.get_device_collection(&wasapi::Direction::Render) else {
        return Vec::new();
    };
    let count = devices.get_nbr_devices().unwrap_or(0);
    let mut endpoints = Vec::new();
    for index in 0..count {
        let Ok(device) = devices.get_device_at_index(index) else {
            continue;
        };
        let Ok(id) = device.get_id() else {
            continue;
        };
        endpoints.push(RenderEndpoint {
            id,
            sessions: sessions(&device),
        });
    }
    endpoints
}

/// The sessions on one endpoint, with pid and state; empty if the session
/// manager does not answer.
fn sessions(device: &wasapi::Device) -> Vec<AppStream> {
    let Ok(manager) = device.get_iaudiosessionmanager() else {
        return Vec::new();
    };
    let Ok(list) = manager.get_audiosessionenumerator() else {
        return Vec::new();
    };
    let count = list.get_count().unwrap_or(0);
    let mut streams = Vec::new();
    for index in 0..count {
        let Ok(session) = list.get_session(index) else {
            continue;
        };
        let Ok(pid) = session.get_process_id() else {
            continue;
        };
        streams.push(AppStream {
            pid: Some(pid),
            name: String::new(),
            active: matches!(session.get_state(), Ok(wasapi::SessionState::Active)),
        });
    }
    streams
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_read_answers_or_fails_without_panicking() {
        // A CI runner usually has no audio endpoint; either answer is fine.
        let followed = render_endpoint_to_follow();
        eprintln!("render endpoint to follow: {followed:?}");
    }
}
