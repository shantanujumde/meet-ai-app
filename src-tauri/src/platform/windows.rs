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

/// The user's 12/24-hour clock (TUR-129): the `sShortTime` pattern under
/// `HKCU\\Control Panel\\International`, like `HH:mm` or `h:mm tt`. A missing
/// value means 24-hour. Verify on a real Windows machine (manual checks).
pub fn clock() -> crate::tray::Clock {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Control Panel\\International")
        .and_then(|key| key.get_value::<String, _>("sShortTime"))
        .map(|pattern| crate::tray::Clock::from_windows_short_time(&pattern))
        .unwrap_or(crate::tray::Clock::H24)
}
