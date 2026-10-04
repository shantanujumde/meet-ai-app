//! The permission checks on Windows (TUR-51).
//!
//! * Microphone: the consent registry keys, decided by
//!   [`super::windows_consent::mic_consent`] (the `NonPackaged` key is the one
//!   a desktop app is held to). A stored `Deny` is trusted without opening
//!   anything. Otherwise the `cpal` microphone is opened briefly, and an
//!   `E_ACCESSDENIED` (0x80070005) from stream setup is a denial too.
//! * System audio: WASAPI loopback needs no permission, so it is
//!   [`ChannelState::NotApplicable`].

use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

use super::windows_consent::{Access, is_access_denied, mic_consent, parse_value};
use crate::permission_check::{self, ChannelResult, ChannelState};

const MICROPHONE_PATH: &str =
    r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";
const DESKTOP_APPS_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\NonPackaged";

fn read(root: winreg::HKEY, path: &str) -> Access {
    let value = RegKey::predef(root)
        .open_subkey(path)
        .and_then(|key| key.get_value::<String, _>("Value"))
        .ok();
    parse_value(value.as_deref())
}

fn stored_access() -> Access {
    mic_consent(
        read(HKEY_LOCAL_MACHINE, MICROPHONE_PATH),
        read(HKEY_CURRENT_USER, MICROPHONE_PATH),
        read(HKEY_CURRENT_USER, DESKTOP_APPS_PATH),
    )
}

/// A stored `Deny` in the consent registry, if there is one.
pub(crate) fn stored_mic_denial() -> Option<ChannelResult> {
    (stored_access() == Access::Denied).then(|| ChannelResult {
        state: ChannelState::Denied,
        detail: "Windows privacy settings block microphone access for desktop apps".into(),
    })
}

/// Open the `cpal` microphone briefly; `E_ACCESSDENIED` on setup is a denial.
pub(crate) fn check_mic() -> ChannelResult {
    let result = permission_check::check_mic_with(super::windows_devices::mic_source());
    if result.state == ChannelState::Unmeasurable && is_access_denied(&result.detail) {
        return ChannelResult {
            state: ChannelState::Denied,
            detail: format!(
                "Windows refused the microphone (E_ACCESSDENIED): {}",
                result.detail
            ),
        };
    }
    result
}

/// Loopback capture needs no permission on Windows.
pub(crate) fn check_system() -> ChannelResult {
    ChannelResult {
        state: ChannelState::NotApplicable,
        detail: "Windows needs no permission to record system audio".into(),
    }
}
