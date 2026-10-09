//! The one place that picks, per OS, how the overlay window looks and
//! behaves (TUR-146, quality rule R10). Everything else about it is shared.
//!
//! - **macOS**: the popover material behind the card, which follows the
//!   app's light or dark appearance (`appearance.rs` sets it app-wide), kept
//!   on even while meet-ai is in the background (the overlay's normal
//!   state); never the key window,
//!   so showing it or clicking it never takes the keyboard from the call;
//!   and TUR-147's non-activating panel ([`after_build`]), the one kind of
//!   window that shows over another app's full-screen Space.
//! - **Windows**: acrylic blur behind it (Windows 10 1903 and later). No OS
//!   shadow: on an undecorated window it draws a 1px frame around the
//!   transparent corners (TUR-108).
//! - **Linux**: no native blur exists, so the card is the CSS fill alone.
//!   On Wayland the compositor may ignore always-on-top and the position.

use tauri::WebviewWindow;
use tauri::utils::config::WindowEffectsConfig;

/// Does the overlay keep the OS window shadow?
#[cfg(target_os = "macos")]
pub const SHADOW: bool = true;
#[cfg(not(target_os = "macos"))]
pub const SHADOW: bool = false;

/// Can the overlay become the focused (key) window? Not on macOS, where
/// showing a window that can makes it key. Elsewhere it is shown without
/// focus (`focused(false)`) and stays focusable, so a click and a drag
/// behave as the OS expects.
#[cfg(target_os = "macos")]
pub const FOCUSABLE: bool = false;
#[cfg(not(target_os = "macos"))]
pub const FOCUSABLE: bool = true;

/// The card's corner radius, which the macOS material is cut to as well:
/// `--radius-panel` in `design-system/meet-ai/tokens.css`.
#[cfg(target_os = "macos")]
const RADIUS: f64 = 16.0;

/// The blur behind the card, where the OS has one.
#[cfg(target_os = "macos")]
pub fn effects() -> Option<WindowEffectsConfig> {
    use tauri::window::{Effect, EffectState};
    Some(WindowEffectsConfig {
        effects: vec![Effect::Popover],
        state: Some(EffectState::Active),
        radius: Some(RADIUS),
        ..WindowEffectsConfig::default()
    })
}

/// The blur behind the card, where the OS has one.
#[cfg(windows)]
pub fn effects() -> Option<WindowEffectsConfig> {
    Some(WindowEffectsConfig {
        effects: vec![tauri::window::Effect::Acrylic],
        ..WindowEffectsConfig::default()
    })
}

/// The blur behind the card, where the OS has one: none on Linux.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn effects() -> Option<WindowEffectsConfig> {
    None
}

/// What the builder cannot say, once the window exists: on macOS the
/// prompt card's panel (`detection/popup/platform/macos.rs`), on every Space
/// and over full-screen apps; nothing elsewhere.
pub fn after_build(window: &WebviewWindow) {
    crate::detection::popup::make_panel(window);
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(target_os = "macos")]
    fn macos_blurs_with_the_popover_material_even_in_the_background() {
        let effects = super::effects().expect("macOS has a material");
        assert_eq!(effects.effects, [tauri::window::Effect::Popover]);
        assert_eq!(effects.state, Some(tauri::window::EffectState::Active));
        const { assert!(!super::FOCUSABLE) };
    }

    #[test]
    #[cfg(windows)]
    fn windows_blurs_with_acrylic() {
        assert_eq!(
            super::effects().unwrap().effects,
            [tauri::window::Effect::Acrylic]
        );
    }

    #[test]
    #[cfg(all(unix, not(target_os = "macos")))]
    fn linux_has_no_native_blur() {
        assert!(super::effects().is_none());
    }
}
