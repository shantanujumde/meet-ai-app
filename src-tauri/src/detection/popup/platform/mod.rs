//! The one place that picks, per OS, which prompts use the card window
//! (TUR-59, TUR-108). A calendar reminder uses it everywhere; whether the
//! detection prompts ("Zoom is open", the mic and speakers in use) do too is
//! the line below. macOS keeps its notification and in-window banner for
//! those.

/// Do detection prompts (not only reminders) show in the card?
#[cfg(target_os = "macos")]
pub const DETECTION_IN_POPUP: bool = false;

#[cfg(not(target_os = "macos"))]
pub const DETECTION_IN_POPUP: bool = true;

/// Does the card window keep the OS window shadow? macOS draws it around the
/// card's rounded shape. On Windows an undecorated window with a shadow gets
/// a 1px frame around the transparent corners, and Linux has none to give.
#[cfg(target_os = "macos")]
pub const SHADOW: bool = true;

#[cfg(not(target_os = "macos"))]
pub const SHADOW: bool = false;

#[cfg(test)]
mod tests {
    use detect::Signal;

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_shows_reminders_in_the_card_and_detection_as_before() {
        let reminder = Signal::Calendar {
            title: "Standup".to_string(),
            attendees: 3,
        };
        assert!(super::super::uses_popup(&reminder));
        assert!(!super::super::uses_popup(&Signal::AudioActivity));
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn windows_and_linux_show_every_prompt_in_the_card() {
        assert!(super::super::uses_popup(&Signal::AudioActivity));
    }
}
