//! Dev/QA tool, not part of `meet-rec` itself: flips the system's real
//! default output device to a given UID (`cargo run -p audio --example
//! switch_output -- list` to find one), so `meet-rec`'s device-change
//! segment reopening (`reopen_segment` in `src/bin/meet-rec.rs`) can be
//! exercised against a genuine Core Audio device swap without physical
//! AirPods on hand. Used to verify TUR-4's AirPods-survival exit-gate
//! condition on real hardware; kept for Tess's TUR-7 fixture/QA work, which
//! needs the same repeatable trigger.
// Core Audio only exists on macOS. Off macOS (CI builds every target on Windows
// and Linux, TUR-36) the example is a one-line `main` that says so; it lives
// under `examples/macos/` so these `cfg`s sit in an OS folder (rule R10).
#[cfg(target_os = "macos")]
use objc2_core_audio::{self as ca, AudioObjectID, AudioObjectPropertyAddress};
#[cfg(target_os = "macos")]
use objc2_core_foundation::CFString;

#[cfg(target_os = "macos")]
fn device_uid_to_id(uid: &str) -> AudioObjectID {
    unsafe {
        let mut address = AudioObjectPropertyAddress {
            mSelector: ca::kAudioHardwarePropertyDevices,
            mScope: ca::kAudioObjectPropertyScopeGlobal,
            mElement: ca::kAudioObjectPropertyElementMain,
        };
        let mut size: u32 = 0;
        ca::AudioObjectGetPropertyDataSize(
            ca::kAudioObjectSystemObject as AudioObjectID,
            std::ptr::NonNull::from(&mut address),
            0,
            std::ptr::null(),
            std::ptr::NonNull::from(&mut size),
        );
        let count = size as usize / std::mem::size_of::<AudioObjectID>();
        let mut ids = vec![0 as AudioObjectID; count];
        ca::AudioObjectGetPropertyData(
            ca::kAudioObjectSystemObject as AudioObjectID,
            std::ptr::NonNull::from(&mut address),
            0,
            std::ptr::null(),
            std::ptr::NonNull::from(&mut size),
            std::ptr::NonNull::new(ids.as_mut_ptr().cast()).unwrap(),
        );
        for id in ids {
            let mut uid_addr = AudioObjectPropertyAddress {
                mSelector: ca::kAudioDevicePropertyDeviceUID,
                mScope: ca::kAudioObjectPropertyScopeGlobal,
                mElement: ca::kAudioObjectPropertyElementMain,
            };
            let mut uid_size = std::mem::size_of::<*mut CFString>() as u32;
            let mut raw: *mut CFString = std::ptr::null_mut();
            let status = ca::AudioObjectGetPropertyData(
                id,
                std::ptr::NonNull::from(&mut uid_addr),
                0,
                std::ptr::null(),
                std::ptr::NonNull::from(&mut uid_size),
                std::ptr::NonNull::new((&mut raw as *mut *mut CFString).cast()).unwrap(),
            );
            if status == 0 && !raw.is_null() {
                let s = objc2_core_foundation::CFRetained::from_raw(
                    std::ptr::NonNull::new(raw).unwrap(),
                );
                if s.to_string() == uid {
                    return id;
                }
            }
        }
        0
    }
}

#[cfg(target_os = "macos")]
fn list_devices() {
    unsafe {
        let mut address = AudioObjectPropertyAddress {
            mSelector: ca::kAudioHardwarePropertyDevices,
            mScope: ca::kAudioObjectPropertyScopeGlobal,
            mElement: ca::kAudioObjectPropertyElementMain,
        };
        let mut size: u32 = 0;
        ca::AudioObjectGetPropertyDataSize(
            ca::kAudioObjectSystemObject as AudioObjectID,
            std::ptr::NonNull::from(&mut address),
            0,
            std::ptr::null(),
            std::ptr::NonNull::from(&mut size),
        );
        let count = size as usize / std::mem::size_of::<AudioObjectID>();
        let mut ids = vec![0 as AudioObjectID; count];
        ca::AudioObjectGetPropertyData(
            ca::kAudioObjectSystemObject as AudioObjectID,
            std::ptr::NonNull::from(&mut address),
            0,
            std::ptr::null(),
            std::ptr::NonNull::from(&mut size),
            std::ptr::NonNull::new(ids.as_mut_ptr().cast()).unwrap(),
        );
        for id in ids {
            let mut uid_addr = AudioObjectPropertyAddress {
                mSelector: ca::kAudioDevicePropertyDeviceUID,
                mScope: ca::kAudioObjectPropertyScopeGlobal,
                mElement: ca::kAudioObjectPropertyElementMain,
            };
            let mut uid_size = std::mem::size_of::<*mut CFString>() as u32;
            let mut raw: *mut CFString = std::ptr::null_mut();
            let status = ca::AudioObjectGetPropertyData(
                id,
                std::ptr::NonNull::from(&mut uid_addr),
                0,
                std::ptr::null(),
                std::ptr::NonNull::from(&mut uid_size),
                std::ptr::NonNull::new((&mut raw as *mut *mut CFString).cast()).unwrap(),
            );
            if status == 0 && !raw.is_null() {
                let s = objc2_core_foundation::CFRetained::from_raw(
                    std::ptr::NonNull::new(raw).unwrap(),
                );
                println!("id={id} uid={}", s);
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("list") {
        list_devices();
        return;
    }
    let target_uid = &args[1];
    let id = device_uid_to_id(target_uid);
    if id == 0 {
        eprintln!("device UID {target_uid} not found");
        std::process::exit(1);
    }
    unsafe {
        let mut address = AudioObjectPropertyAddress {
            mSelector: ca::kAudioHardwarePropertyDefaultOutputDevice,
            mScope: ca::kAudioObjectPropertyScopeGlobal,
            mElement: ca::kAudioObjectPropertyElementMain,
        };
        let mut value = id;
        let status = ca::AudioObjectSetPropertyData(
            ca::kAudioObjectSystemObject as AudioObjectID,
            std::ptr::NonNull::from(&mut address),
            0,
            std::ptr::null(),
            std::mem::size_of::<AudioObjectID>() as u32,
            std::ptr::NonNull::from(&mut value).cast(),
        );
        println!("set default output to {target_uid} (id {id}): status {status}");
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("switch_output drives Core Audio and only runs on macOS");
    std::process::exit(1);
}
