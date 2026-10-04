//! The one place that picks, per OS, whether prompts use the popup window
//! (TUR-59). macOS keeps its notification and in-window banner; turning the
//! popup on there is changing its line below.

#[cfg(target_os = "macos")]
pub const USE_POPUP: bool = false;

#[cfg(not(target_os = "macos"))]
pub const USE_POPUP: bool = true;
