//! Windows and Linux: the card is a plain always-on-top window (TUR-59).
//! Nothing turns it into anything else, and showing it is Tauri's own show.
//! On Wayland the compositor decides where it goes and whether it stays on
//! top.

use tauri::WebviewWindow;

/// Nothing to change on these OSes.
pub fn make_panel(_window: &WebviewWindow) {}

/// Show the window, on top. The builder already asked for no focus.
pub fn show(window: &WebviewWindow) -> tauri::Result<()> {
    window.show()?;
    window.set_always_on_top(true)
}
