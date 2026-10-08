//! The popup's Tauri window: made once, then shown and hidden (TUR-59).
//!
//! The work area is the monitor less the taskbar, the Dock and, on macOS,
//! the menu bar (`NSScreen.visibleFrame` in tauri-runtime-wry), so the card
//! sits just under the menu bar there, as a reminder should (TUR-108).
//!
//! The window takes the size of the card it shows ([`Layout`]): the wide
//! reminder card (TUR-108), or the narrow detection and countdown card
//! (TUR-147). Where the OS draws no window shadow (Windows, Linux), the
//! narrow card's window is [`super::platform::CARD_INSET`] bigger on every
//! side, so the card's own soft shadow has room.
//!
//! On Wayland a client cannot place its own window or keep it on top without
//! layer-shell, so there the compositor decides where it goes (see
//! docs/manual-checks/worktree-tur59.md). Nothing here needs a library the
//! .deb would have to depend on.

use tauri::{
    AppHandle, LogicalSize, Manager as _, PhysicalPosition, PhysicalSize, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
};

use super::Card;
use super::platform;
use detect::Signal;

/// The popup window's label; `capabilities/prompt.json` names it.
pub const LABEL: &str = "prompt";
/// The reminder card's size, in logical pixels (TUR-108).
pub const WIDTH: f64 = 380.0;
pub const HEIGHT: f64 = 72.0;
/// The narrow detection and countdown card's size, in logical pixels
/// (TUR-147): about Granola's.
pub const NARROW_WIDTH: f64 = 220.0;
pub const NARROW_HEIGHT: f64 = 120.0;
/// The card's gap from the work area's top and right edges, in logical
/// pixels.
pub const MARGIN: f64 = 16.0;

/// Which card the window holds, for its size. The window's own page picks
/// the same card from the same rule (`src/ui/PromptPopup.tsx`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// A calendar reminder: the wide card.
    Reminder,
    /// A detection prompt or a countdown: the narrow card.
    Narrow,
}

impl Layout {
    pub fn of(card: &Card) -> Self {
        match card {
            Card::Prompt { prompt } if matches!(prompt.signal, Signal::Calendar { .. }) => {
                Self::Reminder
            }
            Card::Prompt { .. } | Card::Countdown { .. } => Self::Narrow,
        }
    }

    /// The room around the card, in logical pixels, given the OS's `inset`
    /// for a card shadow. The reminder card fills its window, as in TUR-108.
    pub fn inset(self, inset: f64) -> f64 {
        match self {
            Self::Reminder => 0.0,
            Self::Narrow => inset,
        }
    }

    /// The window's size, in logical pixels: the card's, plus the inset on
    /// every side.
    pub fn window_size(self, inset: f64) -> (f64, f64) {
        let (width, height) = match self {
            Self::Reminder => (WIDTH, HEIGHT),
            Self::Narrow => (NARROW_WIDTH, NARROW_HEIGHT),
        };
        let room = 2.0 * self.inset(inset);
        (width + room, height + room)
    }
}

/// Where the popup's top-left corner goes: a window `width` wide (logical
/// px) whose card sits `inset` inside it, so the card is [`MARGIN`] from the
/// top-right of `area` (position and size in physical pixels), at `scale`.
pub fn top_right(
    area_position: PhysicalPosition<i32>,
    area_size: PhysicalSize<u32>,
    scale: f64,
    width: f64,
    inset: f64,
) -> PhysicalPosition<i32> {
    let width = (width * scale).round() as i64;
    let margin = ((MARGIN - inset).max(0.0) * scale).round() as i64;
    let right = i64::from(area_position.x) + i64::from(area_size.width);
    let x = (right - width - margin).max(i64::from(area_position.x));
    let y = i64::from(area_position.y) + margin;
    PhysicalPosition::new(clamp_i32(x), clamp_i32(y))
}

fn clamp_i32(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}

/// Show the popup sized for `layout`, making it the first time, top-right
/// of the primary monitor's work area, without taking focus.
pub fn show(app: &AppHandle, layout: Layout) -> tauri::Result<()> {
    let window = match app.get_webview_window(LABEL) {
        Some(window) => window,
        None => {
            let window = build(app)?;
            platform::make_panel(&window);
            window
        }
    };
    let inset = layout.inset(platform::CARD_INSET);
    let (width, height) = layout.window_size(platform::CARD_INSET);
    window.set_size(LogicalSize::new(width, height))?;
    place(app, &window, width, inset);
    platform::show(&window)
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
        .shadow(platform::SHADOW)
        .always_on_top(true)
        // TUR-147: on every desktop (Space, workspace), and a click on a
        // card in the background presses the button rather than only
        // bringing the window forward (macOS).
        .visible_on_all_workspaces(true)
        .accept_first_mouse(true)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .build()
}

/// Move the popup top-right of the primary monitor; left where it is when
/// no monitor is known.
fn place(app: &AppHandle, window: &WebviewWindow, width: f64, inset: f64) {
    let monitor = match app.primary_monitor() {
        Ok(Some(monitor)) => monitor,
        Ok(None) => return,
        Err(error) => {
            tracing::debug!(%error, "no primary monitor; leaving the popup where it is");
            return;
        }
    };
    let area = monitor.work_area();
    let position = top_right(
        area.position,
        area.size,
        monitor.scale_factor(),
        width,
        inset,
    );
    if let Err(error) = window.set_position(position) {
        tracing::debug!(%error, "could not place the prompt popup");
    }
}
