//! The one place that picks, per OS, how the card window behaves (TUR-59,
//! TUR-108, TUR-147). Every prompt uses the card on every OS now; what
//! differs is the window itself:
//!
//! * macOS ([`macos`]): the window becomes a non-activating `NSPanel` that
//!   joins every Space and shows over full-screen apps, and is ordered in
//!   front without being made key, so it never takes focus from the call.
//! * Windows and Linux ([`other`]): a plain always-on-top window, as before.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{hide, make_panel, show};

#[cfg(not(target_os = "macos"))]
mod other;
#[cfg(not(target_os = "macos"))]
pub use other::{hide, make_panel, show};

/// Does the card window keep the OS window shadow? macOS draws it around the
/// card's rounded shape. On Windows an undecorated window with a shadow gets
/// a 1px frame around the transparent corners, and Linux has none to give.
#[cfg(target_os = "macos")]
pub const SHADOW: bool = true;

#[cfg(not(target_os = "macos"))]
pub const SHADOW: bool = false;

/// The room, in logical pixels, left around the narrow card for its own
/// soft shadow (TUR-147). None on macOS, where the OS draws the shadow; the
/// card's page drops its CSS shadow there (`data-os`).
#[cfg(target_os = "macos")]
pub const CARD_INSET: f64 = 0.0;

#[cfg(not(target_os = "macos"))]
pub const CARD_INSET: f64 = 8.0;

#[cfg(test)]
mod tests {
    use detect::Signal;

    #[test]
    fn every_os_shows_every_prompt_in_the_card() {
        let reminder = Signal::Calendar {
            title: "Standup".to_string(),
            attendees: 3,
        };
        let app = Signal::Process {
            process: detect::processes::name_of("Zoom").to_string(),
        };
        for signal in [reminder, app, Signal::AudioActivity] {
            assert!(super::super::uses_popup(&signal), "{signal:?}");
        }
    }
}
