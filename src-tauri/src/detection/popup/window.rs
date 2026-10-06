//! The popup's Tauri window: made once, then shown and hidden (TUR-59).
//!
//! The work area is the monitor less the taskbar, the Dock and, on macOS,
//! the menu bar (`NSScreen.visibleFrame` in tauri-runtime-wry), so the card
//! sits just under the menu bar there, as a reminder should (TUR-108).
//!
//! On Wayland a client cannot place its own window or keep it on top without
//! layer-shell, so there the compositor decides where it goes (see
//! docs/manual-checks/worktree-tur59.md). Nothing here needs a library the
//! .deb would have to depend on.

use tauri::{
    AppHandle, Manager as _, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

/// The popup window's label; `capabilities/prompt.json` names it.
pub const LABEL: &str = "prompt";
/// Its size, in logical pixels: the compact card (TUR-108).
pub const WIDTH: f64 = 380.0;
pub const HEIGHT: f64 = 72.0;
/// Its gap from the work area's top and right edges, in logical pixels.
pub const MARGIN: f64 = 16.0;

/// Where the popup's top-left corner goes: top-right of `area` (position and
/// size in physical pixels), at `scale`.
pub fn top_right(
    area_position: PhysicalPosition<i32>,
    area_size: PhysicalSize<u32>,
    scale: f64,
) -> PhysicalPosition<i32> {
    let width = (WIDTH * scale).round() as i64;
    let margin = (MARGIN * scale).round() as i64;
    let right = i64::from(area_position.x) + i64::from(area_size.width);
    let x = (right - width - margin).max(i64::from(area_position.x));
    let y = i64::from(area_position.y) + margin;
    PhysicalPosition::new(clamp_i32(x), clamp_i32(y))
}

fn clamp_i32(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}

/// Show the popup, making it the first time, top-right of the primary
/// monitor's work area, without taking focus.
pub fn show(app: &AppHandle) -> tauri::Result<()> {
    let window = match app.get_webview_window(LABEL) {
        Some(window) => window,
        None => build(app)?,
    };
    place(app, &window);
    window.show()?;
    window.set_always_on_top(true)?;
    Ok(())
}

/// Hide the popup, if it exists.
pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL)
        && let Err(error) = window.hide()
    {
        tracing::warn!(%error, "could not hide the prompt popup");
    }
}

fn build(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("meet-ai")
        .inner_size(WIDTH, HEIGHT)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        // TUR-108: only the rounded card paints; its corners show what is
        // behind the window. Whether each OS honours this is a manual check.
        .transparent(true)
        .shadow(super::platform::SHADOW)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .build()
}

/// Move the popup top-right of the primary monitor; left where it is when
/// no monitor is known.
fn place(app: &AppHandle, window: &WebviewWindow) {
    let monitor = match app.primary_monitor() {
        Ok(Some(monitor)) => monitor,
        Ok(None) => return,
        Err(error) => {
            tracing::debug!(%error, "no primary monitor; leaving the popup where it is");
            return;
        }
    };
    let area = monitor.work_area();
    let position = top_right(area.position, area.size, monitor.scale_factor());
    if let Err(error) = window.set_position(position) {
        tracing::debug!(%error, "could not place the prompt popup");
    }
}
