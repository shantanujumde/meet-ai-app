//! The one place that picks, per OS, how the overlay window looks and
//! behaves (TUR-146, quality rule R10). Everything else about it is shared.
//!
//! - **macOS**: the popover material behind the card, which follows the
//!   app's light or dark appearance (`appearance.rs` sets it app-wide), kept
//!   on even while meet-ai is in the background (the overlay's normal
//!   state); never the key window,
//!   so showing it or clicking it never takes the keyboard from the call;
//!   and full-screen auxiliary, so it can join another app's full-screen
//!   Space ([`macos::float_over_full_screen`]).
//! - **Windows**: acrylic blur behind it (Windows 10 1903 and later). No OS
//!   shadow: on an undecorated window it draws a 1px frame around the
//!   transparent corners (TUR-108).
//! - **Linux**: no native blur exists, so the card is the CSS fill alone.
//!   On Wayland the compositor may ignore always-on-top and the position.

use tauri::WebviewWindow;
use tauri::utils::config::WindowEffectsConfig;

#[cfg(target_os = "macos")]
mod macos;

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

/// The blur behind the card, where the OS has one.
#[cfg(target_os = "macos")]
pub fn effects() -> Option<WindowEffectsConfig> {
    use tauri::window::{Effect, EffectState};
    Some(WindowEffectsConfig {
        effects: vec![Effect::Popover],
        state: Some(EffectState::Active),
        radius: Some(super::placement::RADIUS),
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

/// What the builder cannot say, once the window exists.
pub fn after_build(window: &WebviewWindow) {
    #[cfg(target_os = "macos")]
    macos::float_over_full_screen(window);
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(target_os = "macos")]
    fn macos_blurs_with_the_popover_material_even_in_the_background() {
        let effects = super::effects().expect("macOS has a material");
        assert_eq!(effects.effects, [tauri::window::Effect::Popover]);
        assert_eq!(effects.state, Some(tauri::window::EffectState::Active));
        assert!(!super::FOCUSABLE);
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
