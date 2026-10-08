//! Is anything using the default mic or speakers right now? (TUR-31)
//!
//! The audio-activity meeting signal (`docs/problem.md` item 42) needs only
//! two yes/no answers: is some process running I/O on the default input
//! device, and on the default output device. Core Audio keeps exactly that as
//! `kAudioDevicePropertyDeviceIsRunningSomewhere`, a plain property of the
//! device. Reading it opens no stream, installs no tap and captures nothing,
//! so it needs no TCC permission and cannot raise a prompt — unlike
//! [`super::tap`] or [`crate::mic`].
//!
//! Each [`read`] is four property reads (two default-device ids, two
//! running flags), no allocation. The detection loop calls it every 2 s.
//!
//! The answer does not say *who* is running the device: while meet-ai itself
//! records, its own mic stream makes the input "running". The caller ignores
//! readings taken while recording (`detect::activity`).
//!
//! Callers outside this crate use [`crate::activity::device_activity`], which
//! works on every platform.
//!
//! [`mic_users`] answers *who* (TUR-142): macOS 14 added a Core Audio object
//! per audio client, listed as `kAudioHardwarePropertyProcessObjectList`,
//! each with its pid, bundle id and `IsRunningInput` / `IsRunningOutput`
//! flags. Reading them is property reads again, no permission prompt. Core
//! Audio sends no change notification for `IsRunningInput`, so this is
//! polled too. Before macOS 14 the list does not exist: the read fails and
//! the answer is [`MicUsers::NotSupported`]; [`read`] keeps working there.
//! The naming is OS-free, in [`crate::mic_users`]; this file only gathers
//! the facts: the pid, the bundle id, the executable's path
//! (`proc_pidpath`) and the process responsible for it
//! (`responsibility_get_pid_responsible_for_pid`, which is not public API,
//! so it is looked up at run time and its absence only costs the WebKit
//! naming).

// Adapted from github.com/fastrepl/anarlog/crates/detect/src/list/macos.rs @ 259a04ee2e1447dfed150ed08f0a1bb69909b836 (MIT)
// Adapted from github.com/island-io/mila/Mila/Audio/MeetingDetector.swift @ 605babbd5c2639841fc4ea366f9d3cf9f4a21118 (Apache-2.0)

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::OnceLock;

use objc2_core_audio::{self as ca, AudioObjectID, AudioObjectPropertyAddress};
use objc2_core_foundation::{CFBundle, CFRetained, CFString, CFURL};

use super::device_watch::{default_input_device, default_output_device};
use super::tap_rate::read as read_property;
use crate::Error;
use crate::activity::DeviceActivity;
use crate::mic_users::{MicUsers, ProcessFacts, name_apps, outermost_app_dir};

/// Read both flags for the current default devices. The default device ids
/// are re-read every time, so plugging in a headset is followed at once.
pub fn read() -> Result<DeviceActivity, Error> {
    Ok(DeviceActivity {
        input_running: is_running_somewhere(default_input_device()?)?,
        output_running: is_running_somewhere(default_output_device()?)?,
    })
}

/// `kAudioDevicePropertyDeviceIsRunningSomewhere` of `device`. A missing
/// device (`kAudioObjectUnknown`, e.g. a Mac mini with no mic) is not running.
fn is_running_somewhere(device: AudioObjectID) -> Result<bool, Error> {
    if device == ca::kAudioObjectUnknown {
        return Ok(false);
    }
    let mut address = AudioObjectPropertyAddress {
        mSelector: ca::kAudioDevicePropertyDeviceIsRunningSomewhere,
        mScope: ca::kAudioObjectPropertyScopeGlobal,
        mElement: ca::kAudioObjectPropertyElementMain,
    };
    let mut size = std::mem::size_of::<u32>() as u32;
    let mut value: u32 = 0;
    // SAFETY: the property is a scalar `UInt32` of an audio device; `size`
    // and `value` describe a `u32` that outlives the call, and no qualifier
    // is passed (size 0, null pointer).
    let status = unsafe {
        ca::AudioObjectGetPropertyData(
            device,
            std::ptr::NonNull::from(&mut address),
            0,
            std::ptr::null(),
            std::ptr::NonNull::from(&mut size),
            std::ptr::NonNull::from(&mut value).cast(),
        )
    };
    if status != 0 {
        return Err(Error::DeviceRead(format!(
            "AudioObjectGetPropertyData(DeviceIsRunningSomewhere) on device {device} failed: \
             OSStatus {status}"
        )));
    }
    Ok(value != 0)
}

/// One entry of Core Audio's process list.
struct AudioProcess {
    object: AudioObjectID,
    pid: u32,
    input: bool,
    output: bool,
}

/// The apps using a mic, named, without meet-ai. [`MicUsers::NotSupported`]
/// when the process list cannot be read (macOS older than 14).
pub(crate) fn mic_users() -> MicUsers {
    let Some(objects) = process_objects() else {
        return MicUsers::NotSupported;
    };
    let processes: Vec<AudioProcess> = objects.into_iter().filter_map(audio_process).collect();
    let users = processes
        .iter()
        .filter(|process| process.input)
        .map(|process| facts(process.pid, Some(process), true))
        .collect();
    MicUsers::Supported(name_apps(users, std::process::id(), |pid| {
        let listed = processes.iter().find(|process| process.pid == pid);
        let found = facts(pid, listed, false);
        (listed.is_some() || found.path.is_some()).then_some(found)
    }))
}

/// What is known about `pid`: its bundle id (from Core Audio, or else from
/// the app bundle it runs from), its path, and (for a mic user) the process
/// responsible for it.
fn facts(pid: u32, listed: Option<&AudioProcess>, with_responsible: bool) -> ProcessFacts {
    let path = executable_path(pid);
    ProcessFacts {
        pid,
        bundle_id: listed
            .and_then(|process| bundle_id(process.object))
            .or_else(|| path.as_deref().and_then(app_bundle_id)),
        exe: path
            .as_deref()
            .and_then(|path| path.rsplit('/').next())
            .unwrap_or_default()
            .to_string(),
        path,
        responsible: if with_responsible {
            responsible_pid(pid)
        } else {
            None
        },
        output: listed.is_some_and(|process| process.output),
        ..ProcessFacts::default()
    }
}

/// `kAudioHardwarePropertyProcessObjectList`, or `None` when the system
/// object has no such property (macOS older than 14) or the read fails.
fn process_objects() -> Option<Vec<AudioObjectID>> {
    let system = ca::kAudioObjectSystemObject as AudioObjectID;
    let mut address = AudioObjectPropertyAddress {
        mSelector: ca::kAudioHardwarePropertyProcessObjectList,
        mScope: ca::kAudioObjectPropertyScopeGlobal,
        mElement: ca::kAudioObjectPropertyElementMain,
    };
    let mut size = 0u32;
    // SAFETY: `address` and `size` are valid for the call; no qualifier is
    // passed (size 0, null pointer).
    let status = unsafe {
        ca::AudioObjectGetPropertyDataSize(
            system,
            NonNull::from(&mut address),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
        )
    };
    if status != 0 {
        tracing::debug!(status, "no Core Audio process list (macOS older than 14?)");
        return None;
    }
    let count = size as usize / std::mem::size_of::<AudioObjectID>();
    let mut ids = vec![ca::kAudioObjectUnknown; count];
    if count == 0 {
        return Some(ids);
    }
    // SAFETY: `ids` holds `size` bytes of `AudioObjectID`s and outlives the
    // call; Core Audio writes at most `size` bytes and updates `size`.
    let status = unsafe {
        ca::AudioObjectGetPropertyData(
            system,
            NonNull::from(&mut address),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut ids[0]).cast(),
        )
    };
    if status != 0 {
        tracing::debug!(status, "reading the Core Audio process list failed");
        return None;
    }
    // The list can shrink between the two calls.
    ids.truncate(size as usize / std::mem::size_of::<AudioObjectID>());
    Some(ids)
}

/// One process object's pid and running flags. `None` when it has gone (the
/// process quit between the list and this read) or has no real pid.
fn audio_process(object: AudioObjectID) -> Option<AudioProcess> {
    let global = ca::kAudioObjectPropertyScopeGlobal;
    // SAFETY: `kAudioProcessPropertyPID` is a `pid_t` (`i32`).
    let pid: i32 = unsafe { read_property(object, ca::kAudioProcessPropertyPID, global) }?;
    // SAFETY: both running flags are `UInt32`s.
    let input: u32 =
        unsafe { read_property(object, ca::kAudioProcessPropertyIsRunningInput, global) }?;
    // SAFETY: as above.
    let output: u32 =
        unsafe { read_property(object, ca::kAudioProcessPropertyIsRunningOutput, global) }
            .unwrap_or(0);
    Some(AudioProcess {
        object,
        pid: u32::try_from(pid).ok().filter(|&pid| pid > 0)?,
        input: input != 0,
        output: output != 0,
    })
}

/// `kAudioProcessPropertyBundleID`: a `CFStringRef` the caller owns (+1),
/// released when the [`CFRetained`] drops. Empty for a process without one.
fn bundle_id(object: AudioObjectID) -> Option<String> {
    // SAFETY: the property's value is a `CFStringRef`, a pointer.
    let raw: *const CFString = unsafe {
        read_property(
            object,
            ca::kAudioProcessPropertyBundleID,
            ca::kAudioObjectPropertyScopeGlobal,
        )
    }?;
    // SAFETY: non-null, a `CFString`, and handed over with a +1 retain that
    // this `CFRetained` now owns and releases.
    let owned: CFRetained<CFString> =
        unsafe { CFRetained::from_raw(NonNull::new(raw.cast_mut())?) };
    Some(owned.to_string()).filter(|id| !id.is_empty())
}

/// The identifier in the `Info.plist` of the outermost app bundle `path` is
/// in: `com.apple.Safari` for Safari, which Core Audio may not list.
fn app_bundle_id(path: &str) -> Option<String> {
    let url = CFURL::from_directory_path(outermost_app_dir(path)?)?;
    let bundle = CFBundle::new(None, Some(&url))?;
    Some(bundle.identifier()?.to_string()).filter(|id| !id.is_empty())
}

/// The executable's full path, from `proc_pidpath`.
fn executable_path(pid: u32) -> Option<String> {
    let pid = libc::pid_t::try_from(pid).ok()?;
    let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: `buffer` is writable for its whole length, which is the size
    // passed; `proc_pidpath` writes at most that many bytes and returns how
    // many it wrote, or 0 or less on failure.
    let written =
        unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    let written = usize::try_from(written).ok().filter(|&n| n > 0)?;
    buffer.truncate(written);
    Some(String::from_utf8_lossy(&buffer).into_owned())
}

/// `pid_t responsibility_get_pid_responsible_for_pid(pid_t)`, in libsystem
/// on every macOS this app runs on but not in the SDK headers.
type ResponsibleFn = unsafe extern "C" fn(libc::pid_t) -> libc::pid_t;

/// The responsibility lookup, found once with `dlsym`. `None` when this
/// macOS does not have it; WebKit processes are then named Safari.
fn responsible_fn() -> Option<ResponsibleFn> {
    static FOUND: OnceLock<Option<ResponsibleFn>> = OnceLock::new();
    *FOUND.get_or_init(|| {
        // SAFETY: `dlsym` with `RTLD_DEFAULT` and a NUL-terminated name only
        // searches the loaded images; it returns null when the name is absent.
        let symbol: *mut c_void = unsafe {
            libc::dlsym(
                libc::RTLD_DEFAULT,
                c"responsibility_get_pid_responsible_for_pid".as_ptr(),
            )
        };
        if symbol.is_null() {
            tracing::debug!("responsibility_get_pid_responsible_for_pid is missing");
            return None;
        }
        // SAFETY: the symbol is that function, whose C signature is
        // `ResponsibleFn`; a non-null data pointer from `dlsym` is the
        // function's address on Apple platforms.
        Some(unsafe { std::mem::transmute::<*mut c_void, ResponsibleFn>(symbol) })
    })
}

/// The process responsible for `pid` (Safari for its WebKit GPU process),
/// when it is another process.
fn responsible_pid(pid: u32) -> Option<u32> {
    let lookup = responsible_fn()?;
    let raw = libc::pid_t::try_from(pid).ok()?;
    // SAFETY: the function takes any pid and returns a pid, or -1 when it
    // has no answer; it reads kernel state and touches no memory of ours.
    let responsible = unsafe { lookup(raw) };
    u32::try_from(responsible)
        .ok()
        .filter(|&responsible| responsible > 0 && responsible != pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_device_is_not_running() {
        assert_eq!(
            is_running_somewhere(ca::kAudioObjectUnknown).ok(),
            Some(false)
        );
    }

    /// Like `device_watch`'s smoke test: whatever this Mac is doing, the read
    /// itself succeeds. It reads properties only — no stream, no permission
    /// prompt.
    #[test]
    fn the_default_devices_activity_is_readable_on_this_machine() {
        let reading = read();
        assert!(reading.is_ok(), "{reading:?}");
    }

    /// The process list reads on this Mac (macOS 14 or later here) without a
    /// prompt, and never lists this test binary's own pid.
    #[test]
    fn the_mic_users_list_is_readable_on_this_machine() {
        match mic_users() {
            MicUsers::Supported(apps) => {
                assert!(apps.iter().all(|app| app.pid != std::process::id()));
            }
            MicUsers::NotSupported => {
                assert!(process_objects().is_none());
            }
        }
    }

    #[test]
    fn our_own_path_is_readable() {
        let ours = executable_path(std::process::id());
        assert!(ours.is_some_and(|path| path.starts_with('/')));
    }

    #[test]
    fn an_app_bundles_id_comes_from_its_info_plist() {
        // Present on every Mac this runs on; reading it opens nothing.
        let finder = "/System/Library/CoreServices/Finder.app/Contents/MacOS/Finder";
        assert_eq!(app_bundle_id(finder).as_deref(), Some("com.apple.finder"));
        assert_eq!(app_bundle_id("/usr/libexec/avconferenced"), None);
    }

    #[test]
    fn responsibility_answers_without_crashing() {
        // Loaded or not, a lookup on our own pid never names ourselves.
        assert_ne!(
            responsible_pid(std::process::id()),
            Some(std::process::id())
        );
    }
}
