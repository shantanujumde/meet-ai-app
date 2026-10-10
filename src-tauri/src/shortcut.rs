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

/// Managed state: did registering the shortcut work (TUR-169)? Another app
/// may own it already, and then pressing it does nothing in meet-ai, so the
/// window says it is unavailable rather than advertising it. Available until
/// a registration fails: the window never loads before the app's setup has
/// tried.
#[derive(Debug)]
pub struct RecordShortcut(std::sync::atomic::AtomicBool);

impl Default for RecordShortcut {
    fn default() -> Self {
        Self(std::sync::atomic::AtomicBool::new(true))
    }
}

impl RecordShortcut {
    /// How registering the shortcut went.
    pub fn registered(&self, worked: bool) {
        self.0.store(worked, std::sync::atomic::Ordering::SeqCst);
    }

    /// Does pressing the shortcut reach meet-ai?
    pub fn available(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Is the record shortcut meet-ai's (TUR-169)? `false` when registering it
/// failed, most likely because another app owns it.
#[tauri::command]
#[specta::specta]
pub async fn record_shortcut_available(app: tauri::AppHandle) -> bool {
    use tauri::Manager as _;
    app.try_state::<RecordShortcut>()
        .is_none_or(|state| state.available())
}

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
    fn the_shortcut_is_available_until_its_registration_fails() {
        let state = RecordShortcut::default();
        assert!(state.available());
        state.registered(false);
        assert!(!state.available(), "another app owns it");
        state.registered(true);
        assert!(state.available());
    }

    #[test]
    fn windows_and_linux_never_take_the_browser_reload() {
        assert_ne!(RECORD_SHORTCUT_OTHER, "Ctrl+Shift+R");
    }
}
