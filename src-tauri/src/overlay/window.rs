//! The overlay's Tauri window (TUR-146): made when a recording starts and
//! closed when it ends, never hidden and kept. Placed where the user last
//! dragged it, or top-right ([`super::placement`]).

use tauri::{AppHandle, Manager as _, PhysicalPosition, WebviewUrl, WebviewWindow};

use super::placement::{self, Area, HEIGHT, Point, WIDTH};
use super::{platform, position};

/// The overlay window's label; `capabilities/overlay.json` names it, and
/// `src/main.tsx` renders the overlay for it.
pub const LABEL: &str = "overlay";

/// Whether the overlay is up.
pub fn is_open(app: &AppHandle) -> bool {
    app.get_webview_window(LABEL).is_some()
}

/// Make the overlay and show it, without taking focus from the call.
pub fn open(app: &AppHandle) -> tauri::Result<()> {
    if is_open(app) {
        return Ok(());
    }
    let window = build(app)?;
    platform::after_build(&window);
    place(app, &window);
    window.show()?;
    window.set_always_on_top(true)?;
    Ok(())
}

/// Remember where the overlay is, then close it. Nothing when it is not up.
pub fn close(app: &AppHandle) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    match window.outer_position() {
        Ok(at) => {
            let point = Point { x: at.x, y: at.y };
            let saved = crate::folder_move::writing_in_root(app, |root| position::save(root, point));
            if let Err(error) = saved {
                tracing::warn!(message = %error.message, "could not save where the overlay was");
            }
        }
        Err(error) => tracing::debug!(%error, "could not read where the overlay was"),
    }
    if let Err(error) = window.close() {
        tracing::warn!(%error, "could not close the recording overlay");
    }
}

fn build(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let builder = tauri::WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("meet-ai recording")
        .inner_size(WIDTH, HEIGHT)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        // Only the rounded card paints; what is behind the window shows
        // through its corners, and through the card itself where the OS
        // blurs it ([`platform::effects`]).
        .transparent(true)
        .shadow(platform::SHADOW)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .focused(false)
        .focusable(platform::FOCUSABLE)
        // A click on Pause works first time, with meet-ai in the background.
        .accept_first_mouse(true)
        .visible(false);
    match platform::effects() {
        Some(effects) => builder.effects(effects),
        None => builder,
    }
    .build()
}

/// Move the overlay to its saved spot when that is still on a screen, else
/// top-right of the primary monitor. Left where the OS put it when no
/// monitor is known (and on Wayland, where the compositor decides).
fn place(app: &AppHandle, window: &WebviewWindow) {
    let saved = crate::meetings::root()
        .ok()
        .and_then(|root| position::load(&root));
    let monitors: Vec<Area> = app
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|monitor| area(*monitor.position(), *monitor.size()))
        .collect();
    let spot = match placement::restore(saved, &monitors) {
        Some(spot) => spot,
        None => match app.primary_monitor() {
            Ok(Some(monitor)) => {
                let work = monitor.work_area();
                placement::default_spot(area(work.position, work.size), monitor.scale_factor())
            }
            Ok(None) | Err(_) => {
                tracing::debug!("no primary monitor; leaving the overlay where it is");
                return;
            }
        },
    };
    if let Err(error) = window.set_position(PhysicalPosition::new(spot.x, spot.y)) {
        tracing::debug!(%error, "could not place the recording overlay");
    }
}

fn area(position: PhysicalPosition<i32>, size: tauri::PhysicalSize<u32>) -> Area {
    Area {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    }
}
