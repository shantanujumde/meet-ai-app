//! The Windows microphone decision, as pure functions (TUR-51).
//!
//! Compiled on every OS so the rules are unit-tested on the Mac that builds
//! this crate; only `windows_permission.rs` reads the real registry and
//! calls them.
//!
//! Windows keeps microphone consent under
//! `…\CapabilityAccessManager\ConsentStore\microphone`, each key holding a
//! `Value` string of `Allow` or `Deny`:
//!
//! * HKLM `microphone`: the device-wide switch ("Microphone access").
//! * HKCU `microphone`: the per-user master switch, which governs Store
//!   (packaged) apps.
//! * HKCU `microphone\NonPackaged`: "Let desktop apps access your
//!   microphone". meet-ai is a desktop app, so this is the one that counts.

/// One consent key's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    Allowed,
    Denied,
    /// The key or its `Value` is missing or not `Allow`/`Deny`.
    Unknown,
}

/// A registry `Value` string, or `None` when the key or value is missing.
pub(crate) fn parse_value(value: Option<&str>) -> Access {
    match value.map(str::to_ascii_lowercase).as_deref() {
        Some("allow") => Access::Allowed,
        Some("deny") => Access::Denied,
        _ => Access::Unknown,
    }
}

// Adapted from github.com/cjpais/Handy/src-tauri/src/commands/audio.rs @ 73ab851c2b6242283759a4c101b60f0ece132f08 (MIT)
/// The decision for a desktop app from the three keys.
///
/// The device switch off blocks everyone. Otherwise `NonPackaged` decides
/// whenever it has an answer. The UWP master key only counts when
/// `NonPackaged` is silent: debloater tools (O&O ShutUp10 and similar) set
/// it to `Deny` without blocking desktop apps, so trusting it first would
/// call a working microphone denied.
pub(crate) fn mic_consent(hklm: Access, hkcu: Access, nonpackaged: Access) -> Access {
    if hklm == Access::Denied || nonpackaged == Access::Denied {
        Access::Denied
    } else if nonpackaged == Access::Allowed {
        Access::Allowed
    } else if hkcu == Access::Denied {
        Access::Denied
    } else if hklm == Access::Allowed && hkcu == Access::Allowed {
        Access::Allowed
    } else {
        Access::Unknown
    }
}

/// `E_ACCESSDENIED` as WASAPI reports it when the privacy switch blocks the
/// stream, in the spellings `cpal`'s error text can carry it.
const ACCESS_DENIED_MARKERS: [&str; 4] = [
    "0x80070005",
    "e_accessdenied",
    "-2147024891",
    "access is denied",
];

/// Whether a failed stream setup's text names `E_ACCESSDENIED` (0x80070005),
/// which the microphone privacy switch produces on stream setup.
pub(crate) fn is_access_denied(error_text: &str) -> bool {
    let lower = error_text.to_ascii_lowercase();
    ACCESS_DENIED_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::Access::{Allowed, Denied, Unknown};
    use super::*;

    #[test]
    fn values_parse_case_insensitively() {
        assert_eq!(parse_value(Some("Allow")), Allowed);
        assert_eq!(parse_value(Some("deny")), Denied);
        assert_eq!(parse_value(Some("DENY")), Denied);
        assert_eq!(parse_value(Some("Prompt")), Unknown);
        assert_eq!(parse_value(None), Unknown);
    }

    #[test]
    fn the_device_switch_off_blocks_everything() {
        assert_eq!(mic_consent(Denied, Allowed, Allowed), Denied);
        assert_eq!(mic_consent(Denied, Unknown, Unknown), Denied);
    }

    #[test]
    fn nonpackaged_wins_over_the_uwp_master_key() {
        // The debloater case: master key Deny, desktop apps still allowed.
        assert_eq!(mic_consent(Allowed, Denied, Allowed), Allowed);
        assert_eq!(mic_consent(Unknown, Denied, Allowed), Allowed);
        assert_eq!(mic_consent(Allowed, Allowed, Denied), Denied);
    }

    #[test]
    fn the_master_key_counts_only_when_nonpackaged_is_silent() {
        assert_eq!(mic_consent(Allowed, Denied, Unknown), Denied);
        assert_eq!(mic_consent(Allowed, Allowed, Unknown), Allowed);
        assert_eq!(mic_consent(Unknown, Allowed, Unknown), Unknown);
        assert_eq!(mic_consent(Unknown, Unknown, Unknown), Unknown);
    }

    #[test]
    fn access_denied_is_recognised_in_its_spellings() {
        assert!(is_access_denied(
            "A backend-specific error has occurred: 0x80070005"
        ));
        assert!(is_access_denied("HRESULT E_ACCESSDENIED"));
        assert!(is_access_denied("os error -2147024891"));
        assert!(is_access_denied("Access is denied. (0x80070005)"));
        assert!(!is_access_denied("The device is no longer available"));
        assert!(!is_access_denied("0x88890004"));
    }
}
