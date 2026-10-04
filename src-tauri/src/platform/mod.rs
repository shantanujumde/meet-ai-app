//! The OS seam for the app shell (SPEC §8.2).
//!
//! The one place in `src-tauri/src` where new OS-specific code goes, so the
//! rest of the shell carries no OS `cfg` (quality rule R10). Tauri's own
//! desktop-vs-mobile plugin gates in `lib.rs` and `notify.rs` are not OS
//! ports and stay where they are.
//!
//! Small enough today to be this one file. When a port needs more, it becomes
//! `macos.rs` / `windows.rs` / `linux.rs` beside it, the way the crates do it.
//!
//! TUR-76 added the app-lifecycle seams: the macOS Dock icon, the macOS app
//! menu's Quit, the Dock-click reopen event, and whether there is a tray to
//! hide to.

#[cfg(test)]
use std::path::Path;

#[cfg(test)]
use stt::SttEngine;

use tauri::{AppHandle, RunEvent};

#[cfg(windows)]
mod windows;

/// Which tray icon family this OS gets (TUR-58, `tray/icons.rs`).
#[cfg(target_os = "macos")]
pub const TRAY_OS: crate::tray::TrayOs = crate::tray::TrayOs::MacOs;

/// Which tray icon family this OS gets (TUR-58, `tray/icons.rs`).
#[cfg(windows)]
pub const TRAY_OS: crate::tray::TrayOs = crate::tray::TrayOs::Windows;

/// Which tray icon family this OS gets (TUR-58, `tray/icons.rs`).
#[cfg(all(unix, not(target_os = "macos")))]
pub const TRAY_OS: crate::tray::TrayOs = crate::tray::TrayOs::Linux;

/// Whether the taskbar the tray sits on is light (TUR-58). Only Windows has
/// one setting to read; elsewhere the answer does not change the icon.
pub fn taskbar_is_light() -> bool {
    #[cfg(windows)]
    {
        windows::taskbar_is_light()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// The record shortcut this OS registers (TUR-58): ⌘⇧R on macOS, Ctrl+Alt+R
/// on Windows and Linux, where Ctrl+Shift+R is every browser's hard reload.
pub const RECORD_SHORTCUT: &str = if cfg!(target_os = "macos") {
    crate::shortcut::RECORD_SHORTCUT_MAC
} else {
    crate::shortcut::RECORD_SHORTCUT_OTHER
};

/// The whisper engine for `model`, the way `stt::registry::select` builds it
/// for the whisper choice. For the live-transcript end-to-end tests, which then
/// compile on every OS; whisper itself is only built for macOS so far.
#[cfg(all(test, target_os = "macos"))]
pub(crate) fn load_whisper(model: &Path) -> Result<Box<dyn SttEngine>, String> {
    let config = stt::whisper::WhisperConfig {
        language: stt::model::whisper_language_for_file(model).map(str::to_owned),
        ..Default::default()
    };
    let engine = stt::whisper::WhisperEngine::load(model, config).map_err(|e| e.to_string())?;
    Ok(Box::new(engine))
}

/// No whisper engine is built for this OS yet (`crates/stt/src/platform`).
#[cfg(all(test, not(target_os = "macos")))]
pub(crate) fn load_whisper(_model: &Path) -> Result<Box<dyn SttEngine>, String> {
    Err("the whisper engine is not built for this platform yet".into())
}

/// Show or hide the Dock icon (macOS). `Regular` while the main window is
/// open, `Accessory` (menu bar only, like Granola) while it is hidden.
/// Nothing to do elsewhere: Windows and Linux have no Dock, and their
/// taskbar entry goes with the window by itself.
pub fn set_dock_visible(app: &AppHandle, visible: bool) {
    #[cfg(target_os = "macos")]
    {
        let policy = if visible {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        };
        if let Err(error) = app.set_activation_policy(policy) {
            tracing::warn!(%error, visible, "could not change the Dock icon");
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, visible);
}

/// Whether `event` is the Dock icon being clicked, or the app being launched
/// again from Finder or Spotlight while it runs (macOS `Reopen`). Never on
/// Windows or Linux, where a second launch reaches the single-instance guard.
pub fn is_reopen(event: &RunEvent) -> bool {
    #[cfg(target_os = "macos")]
    {
        matches!(event, RunEvent::Reopen { .. })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = event;
        false
    }
}

/// Whether there is a menu-bar or tray item to bring the window back from.
///
/// `id` is the tray's id (`tray.rs`). The check is only as good as
/// `tray-icon`'s own answer: on Linux it builds the item whenever
/// libayatana-appindicator loads, and cannot tell that GNOME without the
/// AppIndicator extension will never draw it. Such a user closes the window
/// and has to relaunch to get it back, which the single-instance guard
/// handles. Linux without the library at all gets `false`, and closing quits.
pub fn has_tray(app: &AppHandle, id: &str) -> bool {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        app.tray_by_id(id).is_some()
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let _ = (app, id);
        false
    }
}

/// The text next to the menu-bar icon: the next meeting's countdown
/// (TUR-77), or `None` for the icon alone. macOS only: Windows trays have no
/// text, and a Linux AppIndicator label crowds the panel, so both skip it.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn set_tray_title(tray: &tauri::tray::TrayIcon, title: Option<&str>) {
    #[cfg(target_os = "macos")]
    {
        if let Err(error) = tray.set_title(title) {
            tracing::warn!(%error, "could not set the menu-bar title");
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (tray, title);
}

/// The one-off notice for the first time the window is closed.
pub fn still_running_notice() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "meet-ai is still running in the menu bar. Quit from the menu bar icon or with ⌘Q."
    }
    #[cfg(not(target_os = "macos"))]
    {
        "meet-ai is still running in the tray. Quit from the tray icon."
    }
}

/// Replace the macOS app menu's Quit (⌘Q) with an item of our own, with id
/// `quit_id`, so ⌘Q can ask first while recording.
///
/// The stock item sends AppKit's `terminate:` straight to the app, which ends
/// it without the `ExitRequested` event a quit could be stopped in. Ours is
/// only a menu event; `on_quit` decides. Logout and shutdown still reach
/// `terminate:` themselves, and those do not ask (the exit hook stops the
/// recording). Windows and Linux have no app menu: their quit is the tray's.
pub fn install_quit_menu(app: &AppHandle, quit_id: &str, on_quit: fn(&AppHandle)) {
    #[cfg(target_os = "macos")]
    {
        if let Err(error) = macos_quit_menu(app, quit_id, on_quit) {
            tracing::warn!(%error, "could not put the asking Quit in the app menu; ⌘Q quits at once");
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, quit_id, on_quit);
}

/// Tauri's own default menu, with the last item of the app submenu (the
/// stock Quit, see `tauri::menu::Menu::default`) swapped for ours.
#[cfg(target_os = "macos")]
fn macos_quit_menu(app: &AppHandle, quit_id: &str, on_quit: fn(&AppHandle)) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItemBuilder};

    let menu = Menu::default(app)?;
    let Some(app_menu) = menu
        .items()?
        .into_iter()
        .find_map(|item| item.as_submenu().cloned())
    else {
        return Ok(());
    };
    let last = app_menu.items()?.len().saturating_sub(1);
    app_menu.remove_at(last)?;
    let name = &app.package_info().name;
    app_menu.append(
        &MenuItemBuilder::with_id(quit_id, format!("Quit {name}"))
            .accelerator("CmdOrCtrl+Q")
            .build(app)?,
    )?;
    app.set_menu(menu)?;
    let quit_id = quit_id.to_owned();
    app.on_menu_event(move |app, event| {
        if event.id().as_ref() == quit_id {
            on_quit(app);
        }
    });
    Ok(())
}

/// Is this an Apple silicon Mac? Decides which whisper model Settings marks
/// "Recommended" (TUR-79).
pub(crate) fn is_apple_silicon() -> bool {
    cfg!(all(target_os = "macos", target_arch = "aarch64"))
}

/// Whether this OS has a Calendar app for EventKit to read (macOS), the one
/// calendar source that needs no sign-in (TUR-49, SPEC A12). Windows and
/// Linux read calendars only through the Google and Microsoft sign-ins.
pub const HAS_CALENDAR_APP: bool = cfg!(target_os = "macos");

/// Where the OS lets the user allow meet-ai's notifications (TUR-78):
/// System Settings → Notifications on macOS, Settings → Notifications on
/// Windows. `None` on Linux, where each desktop has its own place.
pub fn notification_settings_url() -> Option<&'static str> {
    #[cfg(target_os = "macos")]
    {
        Some("x-apple.systempreferences:com.apple.Notifications-Settings.extension")
    }
    #[cfg(target_os = "windows")]
    {
        Some("ms-settings:notifications")
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

/// Is the OS blocking meet-ai's notifications? Through the notification
/// plugin's `permission_state`. On desktop, tauri-plugin-notification 2.4.0
/// answers `Granted` without asking the OS, so this is `false` there until
/// the plugin (or a native check here) can tell.
pub fn notifications_blocked(app: &AppHandle) -> bool {
    use tauri_plugin_notification::{NotificationExt as _, PermissionState};

    match app.notification().permission_state() {
        Ok(PermissionState::Denied) => true,
        Ok(_) => false,
        Err(error) => {
            tracing::debug!(%error, "could not read the notification permission");
            false
        }
    }
}

/// Marks a test's fake CLI script executable (`sync/tests.rs`). Off Unix
/// there is no mode bit to set, and no `/bin/sh` to run such a script with
/// either; making those tests portable is TUR-54.
#[cfg(all(test, unix))]
pub(crate) fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

/// Whether a test's fake agent CLI can run here: `agent::fake` and the
/// `sync/tests.rs` scripts both need `/bin/sh`. Tests that start one return
/// early where it is false, until TUR-54 makes the fakes portable.
#[cfg(test)]
pub(crate) const FAKE_CLI_RUNS: bool = cfg!(unix);

/// First line of a test that starts a fake agent CLI: where
/// [`FAKE_CLI_RUNS`] is false it says why on stderr and returns, so the CI
/// log shows the skip instead of a silent `ok`. It writes to stderr directly
/// because libtest hides `eprintln!` output of passing tests.
#[cfg(test)]
macro_rules! skip_without_fake_cli {
    () => {
        if !$crate::platform::FAKE_CLI_RUNS {
            use std::io::Write as _;
            let _ = writeln!(
                std::io::stderr(),
                "skipped {}: the fake agent CLI needs /bin/sh, which this OS lacks (TUR-54)",
                module_path!()
            );
            return;
        }
    };
}
#[cfg(test)]
pub(crate) use skip_without_fake_cli;

/// See the Unix version: nothing to set here.
#[cfg(all(test, not(unix)))]
pub(crate) fn make_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// How the Setup screen writes the sign-in command for this OS's terminal
/// (TUR-53). Plain data, so every style is tested on every OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "each OS build uses only its own style")
)]
pub(crate) enum SignInShell {
    /// macOS Terminal: the bare name, unless the CLI is a copy inside an
    /// `.app` bundle, which is never on the shell's `PATH`.
    PosixAppBundles,
    /// A Linux terminal: always the bare name. An installer that puts the
    /// CLI in `~/.local/bin` or `~/.npm-global/bin` also puts that folder on
    /// the shell's `PATH`, even when the app's own `PATH` lacks it.
    Posix,
    /// PowerShell: the bare name when its folder is on `PATH` (GUI apps get
    /// the user's full `PATH` on Windows), else `& 'C:\...\claude.exe'`.
    PowerShell,
}

/// This OS's sign-in command style.
#[cfg(target_os = "macos")]
pub(crate) const SIGN_IN_SHELL: SignInShell = SignInShell::PosixAppBundles;

/// This OS's sign-in command style.
#[cfg(windows)]
pub(crate) const SIGN_IN_SHELL: SignInShell = SignInShell::PowerShell;

/// This OS's sign-in command style.
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) const SIGN_IN_SHELL: SignInShell = SignInShell::Posix;
