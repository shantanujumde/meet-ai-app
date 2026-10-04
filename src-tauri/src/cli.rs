//! Command-line flags (TUR-58): `meet-ai --toggle-recording`.
//!
//! Global shortcuts never fire under Wayland (`global-hotkey` has no Wayland
//! support), so a Linux user binds this command to a key in GNOME or KDE's
//! own custom shortcuts instead (README). The second launch reaches the
//! running app through the single-instance guard, which toggles there.
//!
//! When no meet-ai is running, the flag starts the app normally and does
//! **not** record. SPEC L15: nothing records without a click. A typed command
//! counts as that click only when it reaches an app that is already running,
//! the same as pressing ⌘⇧R in it; a login item, a script or a stale shortcut
//! that launches the app with the flag must not open the microphone.

/// The flag. Parsed only by [`wants_toggle`].
pub const TOGGLE_RECORDING_FLAG: &str = "--toggle-recording";

/// Whether `argv` (program name first, as the OS passes it) asks to toggle
/// the recording.
pub fn wants_toggle<S: AsRef<str>>(argv: &[S]) -> bool {
    argv.iter()
        .skip(1)
        .any(|arg| arg.as_ref() == TOGGLE_RECORDING_FLAG)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flag_asks_for_a_toggle() {
        assert!(wants_toggle(&["meet-ai", "--toggle-recording"]));
        assert!(wants_toggle(&[
            "/usr/bin/meet-ai",
            "--other",
            "--toggle-recording"
        ]));
    }

    #[test]
    fn a_plain_launch_does_not() {
        assert!(!wants_toggle(&["meet-ai"]));
        assert!(!wants_toggle::<&str>(&[]));
        assert!(!wants_toggle(&["meet-ai", "--toggle"]));
        assert!(!wants_toggle(&["meet-ai", "--toggle-recording=1"]));
    }

    #[test]
    fn the_program_name_is_never_read_as_the_flag() {
        assert!(!wants_toggle(&["--toggle-recording"]));
    }
}
