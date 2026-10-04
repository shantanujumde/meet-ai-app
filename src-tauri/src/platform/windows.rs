//! Windows-only pieces of the app shell (SPEC §8.2, rule R10).

// Adapted from github.com/cjpais/Handy/src-tauri/src/tray.rs @ 73ab851c2b6242283759a4c101b60f0ece132f08 (MIT)
/// Whether the Windows taskbar uses the light theme (TUR-58): the
/// `SystemUsesLightTheme` value under the user's `Personalize` key. That is
/// the *system* (taskbar) theme, which can differ from the apps' theme. A
/// missing key or value means the Windows default, a dark taskbar.
pub fn taskbar_is_light() -> bool {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")
        .and_then(|key| key.get_value::<u32, _>("SystemUsesLightTheme"))
        .map(|light| light != 0)
        .unwrap_or(false)
}
