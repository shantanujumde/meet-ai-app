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

use tauri::image::Image;
use tauri::menu::{MenuBuilder, MenuEvent, MenuItem, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Listener as _, Manager as _};

use crate::recording::{self, Phase};

/// The menu-bar glyph: "m.", the brand symbol, drawn on the 16px pixel grid by
/// the brand build (`design-system/meet-ai/brand/tools/build.mjs`) and
/// rasterised by its `render.sh`. Never hand-exported.
///
/// The 2x raster is the one embedded. `tray-icon` sizes every status-item image
/// to 18pt tall whatever it is handed, so the larger art is the one with pixels
/// to spare when AppKit scales it.
///
/// Embedded rather than bundled as a resource: a resource that fails to copy
/// leaves a menu-bar item with no icon at all, and that is a far worse failure
/// than a slightly bigger binary.
const TEMPLATE_ICON: &[u8] = include_bytes!("../icons/meet-aiTemplate@2x.png");

const OPEN_ITEM: &str = "tray-open-window";
const TOGGLE_ITEM: &str = "tray-toggle-recording";
const QUIT_ITEM: &str = "tray-quit";

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
    let menu = MenuBuilder::new(app)
        .item(&MenuItemBuilder::with_id(OPEN_ITEM, "Open meet-ai").build(app)?)
        .separator()
        .item(&toggle)
        .separator()
        .item(&MenuItemBuilder::with_id(QUIT_ITEM, "Quit meet-ai").build(app)?)
        .build()?;

    TrayIconBuilder::with_id("meet-ai")
        .icon(Image::from_bytes(TEMPLATE_ICON)?)
        .icon_as_template(true)
        .tooltip("meet-ai")
        .menu(&menu)
        // Left-click opens the menu rather than silently toggling a recording.
        // A menu-bar item that starts capturing audio on a stray click is not a
        // thing to ship.
        .show_menu_on_left_click(true)
        .on_menu_event(on_menu_event)
        .build(app)?;

    watch_recording_state(app, toggle);
    Ok(())
}

/// Keep the recording item's wording honest.
///
/// `recording.rs` emits on every transition precisely so that the menu bar, the
/// titlebar and the sidebar cannot disagree about what is happening. The event
/// is used only as a nudge — the phase is then read back from the one recorder
/// everything else reads, so the label cannot drift from the real state even if
/// an event is missed.
fn watch_recording_state(app: &AppHandle, toggle: MenuItem<tauri::Wry>) {
    let handle = app.clone();
    app.listen(crate::events::RECORDING_STATE_EVENT, move |_event| {
        let Some(recorder) = handle.try_state::<recording::Recorder>() else {
            return;
        };
        if let Err(error) = toggle.set_text(label_for(recorder.status().phase)) {
            tracing::warn!(%error, "could not relabel the menu-bar recording item");
        }
    });
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
        OPEN_ITEM => show_window(app),
        // Off the main thread: this callback is delivered on it, and the
        // toggle blocks for as long as the chime and Core Audio take.
        TOGGLE_ITEM => crate::spawn_toggle(app, "menu-bar toggle"),
        QUIT_ITEM => app.exit(0),
        other => tracing::warn!(item = other, "unknown menu-bar item"),
    }
}

fn show_window(app: &AppHandle) {
    let Some(window) = app.webview_windows().values().next().cloned() else {
        tracing::warn!("no window to open from the menu bar");
        return;
    };
    // TUR-76: the Dock icon went away with the window; bring it back first.
    crate::platform::set_dock_visible(app, true);
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}
