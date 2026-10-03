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

/// The whisper engine for `model`, the way `stt::registry::select` builds it
/// for the whisper choice. For the live-transcript end-to-end tests, which then
/// compile on every OS; whisper itself is only built for macOS so far.
#[cfg(all(test, target_os = "macos"))]
pub(crate) fn load_whisper(model: &Path) -> Result<Box<dyn SttEngine>, String> {
    let engine =
        stt::whisper::WhisperEngine::load(model, Default::default()).map_err(|e| e.to_string())?;
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
