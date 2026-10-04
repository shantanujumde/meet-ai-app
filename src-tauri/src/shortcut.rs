//! The global record shortcut (TUR-58): one value per OS family.
//!
//! These two strings are the only place the shortcut is spelled. Rust
//! registers the one for this OS (`platform::RECORD_SHORTCUT`), and both
//! reach the window through `bindings.ts`, where `src/lib/` turns the right
//! one into the OS's own key names. No copy of it is kept in TypeScript.

/// macOS: ⌘⇧R, as SPEC §5 Phase 2 names it.
pub const RECORD_SHORTCUT_MAC: &str = "CmdOrCtrl+Shift+R";

/// Windows and Linux: Ctrl+Alt+R. Ctrl+Shift+R is the browsers' hard reload,
/// which meet-ai would take over everywhere while it runs (decided 2026-10-03).
pub const RECORD_SHORTCUT_OTHER: &str = "Ctrl+Alt+R";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_shortcuts_parse_as_accelerators() {
        use tauri_plugin_global_shortcut::Shortcut;
        for value in [RECORD_SHORTCUT_MAC, RECORD_SHORTCUT_OTHER] {
            assert!(value.parse::<Shortcut>().is_ok(), "{value}");
        }
    }

    #[test]
    fn this_os_registers_one_of_the_pair() {
        assert!(
            [RECORD_SHORTCUT_MAC, RECORD_SHORTCUT_OTHER]
                .contains(&crate::platform::RECORD_SHORTCUT)
        );
    }

    #[test]
    fn windows_and_linux_never_take_the_browser_reload() {
        assert_ne!(RECORD_SHORTCUT_OTHER, "Ctrl+Shift+R");
    }
}
