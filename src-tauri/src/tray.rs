//! The menu-bar item.
//!
//! The icon is a **template image**: pure black with an alpha channel and no
//! colour of its own. macOS recolours a template itself — dark glyph on a light
//! menu bar, light glyph on a dark one — but only once the image is flagged as
//! a template. An unflagged PNG stays solid black on a dark menu bar, which
//! every user reads as a rendering bug rather than as a design choice.
//!
//! The flag is set twice here on purpose. The files are named with AppKit's
//! `…Template` suffix so the intent travels with the artwork, and
//! [`icon_as_template`](tauri::tray::TrayIconBuilder::icon_as_template) states
//! it in code so it does not quietly depend on how the bytes were loaded.
//!
//! TUR-77: the menu opens with today's meetings (`today.rs` keeps them
//! current, `menu_model.rs` decides what they say), and can show the next
//! one's countdown next to the icon.

use tauri::image::Image;
use tauri::menu::{MenuEvent, MenuItem, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Listener as _, Manager as _};

use crate::recording::{self, Phase};

mod icons;
mod menu_model;
mod today;

pub use today::{redraw_soon, reread_soon};

pub use icons::TrayOs;
use icons::{TrayArt, pick};

/// Every menu-bar item's id starts with this, `today.rs`'s too. Menu events
/// reach every handler, so the app menu's own items (`app-quit`, Edit's
/// copy and paste) arrive here as well, and are not ours to warn about.
const TRAY_PREFIX: &str = "tray-";
const OPEN_ITEM: &str = "tray-open-window";
const TOGGLE_ITEM: &str = "tray-toggle-recording";
const QUIT_ITEM: &str = "tray-quit";
/// The tray's id; `lifecycle.rs` looks it up by the same name.
const TRAY_ID: &str = "meet-ai";

/// Put meet-ai in the menu bar.
///
/// A failure is logged and swallowed. The window, its buttons and ⌘⇧R all still
/// work without a menu-bar item, so refusing to start the app over one would be
/// out of all proportion.
pub fn init(app: &AppHandle) {
    if let Err(error) = build(app) {
        tracing::warn!(%error, "could not create the menu-bar item");
    }
}

fn build(app: &AppHandle) -> tauri::Result<()> {
    let toggle = MenuItemBuilder::with_id(TOGGLE_ITEM, label_for(Phase::Idle)).build(app)?;
    let fixed = today::Fixed {
        open: MenuItemBuilder::with_id(OPEN_ITEM, "Open meet-ai").build(app)?,
        toggle: toggle.clone(),
        quit: MenuItemBuilder::with_id(QUIT_ITEM, "Quit meet-ai").build(app)?,
    };
    // "Today" reads "Reading your calendar…" until the worker's first read.
    let first = menu_model::build_menu_model(
        &menu_model::CalendarRead::Pending,
        &chrono::Local::now(),
        0,
        false,
    );
    let menu = today::menu(app, &first, &fixed)?;

    let art = current_art(false);
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(art.png)?)
        .icon_as_template(art.template)
        .tooltip("meet-ai")
        .menu(&menu)
        // Left-click opens the menu rather than silently toggling a recording.
        // TUR-58: and the menu is the only way in on Linux, whose tray sends
        // no click events at all, so every action lives in it.
        // A menu-bar item that starts capturing audio on a stray click is not a
        // thing to ship.
        .show_menu_on_left_click(true)
        .on_menu_event(on_menu_event)
        .build(app)?;

    watch_recording_state(app, toggle, art);
    today::start(app, TRAY_ID, fixed);
    Ok(())
}

/// Keep the recording item's wording honest.
///
/// `recording.rs` emits on every transition precisely so that the menu bar, the
/// titlebar and the sidebar cannot disagree about what is happening. The event
/// is used only as a nudge — the phase is then read back from the one recorder
/// everything else reads, so the label cannot drift from the real state even if
/// an event is missed.
///
/// TUR-58: the icon follows too, off macOS (idle or recording, and the
/// Windows taskbar theme as it is now). On macOS [`current_art`] never
/// changes, so the icon is never touched there.
fn watch_recording_state(app: &AppHandle, toggle: MenuItem<tauri::Wry>, first: TrayArt) {
    app.manage(ShownArt(std::sync::Mutex::new(first)));
    let handle = app.clone();
    app.listen(crate::events::RECORDING_STATE_EVENT, move |_event| {
        let Some(recorder) = handle.try_state::<recording::Recorder>() else {
            return;
        };
        if let Err(error) = toggle.set_text(label_for(recorder.status().phase)) {
            tracing::warn!(%error, "could not relabel the menu-bar recording item");
        }
        refresh_icon(&handle);
        // Record in Today's submenus is greyed while a recording is under way.
        today::redraw_soon(&handle);
    });
}

/// The icon the tray shows now, so a redraw that changes nothing is skipped.
struct ShownArt(std::sync::Mutex<TrayArt>);

/// Pick the tray icon again from the recording phase and the taskbar theme
/// as they are now (TUR-58). Called on every recording transition and when
/// the OS theme changes (`lifecycle::on_window_event`), so a Windows taskbar
/// switched to light or dark while idle gets the matching glyph at once.
pub fn refresh_icon(app: &AppHandle) {
    let (Some(recorder), Some(shown)) = (
        app.try_state::<recording::Recorder>(),
        app.try_state::<ShownArt>(),
    ) else {
        return;
    };
    let art = current_art(recorder.status().phase != Phase::Idle);
    let Ok(mut shown) = shown.0.lock() else {
        return;
    };
    if *shown != art {
        if let Err(error) = set_art(app, art) {
            tracing::warn!(%error, "could not change the tray icon");
        }
        *shown = art;
    }
}

/// The picture for this OS and taskbar right now.
fn current_art(recording: bool) -> TrayArt {
    pick(
        crate::platform::TRAY_OS,
        crate::platform::taskbar_is_light(),
        recording,
    )
}

fn set_art(app: &AppHandle, art: TrayArt) -> tauri::Result<()> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };
    tray.set_icon(Some(Image::from_bytes(art.png)?))?;
    tray.set_icon_as_template(art.template)
}

/// What the recording item says right now.
///
/// `Starting` and `Stopping` get their own wording rather than borrowing a
/// neighbouring state's. Clicking during a transition does nothing, and an item
/// reading "Stop recording" that ignores the click is a worse lie than one that
/// says it is still starting.
fn label_for(phase: Phase) -> &'static str {
    match phase {
        Phase::Idle => "Start recording",
        Phase::Starting => "Starting…",
        Phase::Recording => "Stop recording",
        Phase::Stopping => "Stopping…",
    }
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        OPEN_ITEM => crate::lifecycle::show_main_window(app),
        // Off the main thread: this callback is delivered on it, and the
        // toggle blocks for as long as the chime and Core Audio take.
        TOGGLE_ITEM => crate::spawn_toggle(app, "menu-bar toggle"),
        QUIT_ITEM => app.exit(0),
        other if !is_tray_item(other) => {}
        other if today::on_click(app, other) => {}
        other => tracing::warn!(item = other, "unknown menu-bar item"),
    }
}

/// Whether `id` names one of the menu-bar item's own entries.
fn is_tray_item(id: &str) -> bool {
    id.starts_with(TRAY_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_tray_ids_are_the_menu_bar_s_own() {
        for id in [
            OPEN_ITEM,
            TOGGLE_ITEM,
            QUIT_ITEM,
            "tray-join:abc",
            "tray-today-connect",
        ] {
            assert!(is_tray_item(id), "{id}");
        }
        // The app menu's items reach the same handler and are ignored quietly.
        for id in ["app-quit", "copy", "paste", ""] {
            assert!(!is_tray_item(id), "{id}");
        }
    }
}
