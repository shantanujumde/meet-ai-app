//! Meeting detection for meet-ai.
//!
//! L15: detection never starts a recording on its own. It notifies, and the user
//! confirms. SPEC §2.3 also rules browser-based meetings out of v1 — Google Meet
//! in a tab is not detectable without a browser extension, and calendar plus
//! audio activity covers it.
//!
//! - [`processes`]: which process names are meeting apps (`processes.json`).
//! - [`detector`]: when a running app is worth asking about (TUR-27's rules).
//! - [`poll`]: the loop that reads the process list and emits [`Signal`]s.
//! - [`activity`]: when the mic and speakers both in use count as a call
//!   (TUR-31), and the loop that reads them.
//! - [`call_start`]: when a call app or a browser holding the mic counts as
//!   a call starting (TUR-143), and [`mic_poll`], the loop that feeds it.
//!   Where the OS can list the apps using the mic, this replaces "a meeting
//!   app is open" ([`detector`]), which stays the fallback.
//! - [`call_end`]: when the call being recorded has ended, and the countdown
//!   that asks before stopping (TUR-144).

#![forbid(unsafe_op_in_unsafe_fn)]

pub mod activity;
pub mod call_end;
pub mod call_start;
pub mod detector;
pub mod mic_poll;
pub mod poll;
pub mod processes;
mod worker;

pub use activity::{AUDIO_POLL_INTERVAL, ActivityLoop, ActivitySource, AudioReading};
pub use call_start::{CallStarted, MicKind, MicReading, MicUser};
pub use detector::{CALL_SIGNAL_WINDOW, Detector, RunningProcess};
pub use mic_poll::{MicLoop, MicLoopHooks, MicSource};
pub use poll::{DetectionLoop, POLL_INTERVAL, ProcessSource, SysinfoProcesses, spawn};

/// Why meet-ai thinks a meeting is happening.
///
/// Carried through to the notification copy, so the user is told *why* they are
/// being asked rather than just being asked.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Signal {
    /// A calendar event with enough attendees starts in a minute.
    Calendar {
        title: String,
        #[specta(type = u32)]
        attendees: usize,
    },
    /// A known meeting application is running. `process` is its name as
    /// `processes.json` spells it, e.g. `zoom.us`.
    Process { process: String },
    /// The mic and the speakers have both been in use for a while
    /// (20 s, see [`activity`]), and no known meeting app explains it — a
    /// call in a browser tab, say.
    AudioActivity,
    /// A call app or a browser has been using the mic for a while
    /// ([`call_start`], TUR-143). `app` is its name as the user knows it
    /// ("WhatsApp", "Google Chrome"); `browser` says the call is in a tab.
    Call { app: String, browser: bool },
}

impl Signal {
    /// One sentence saying why the user is being asked, for the notification
    /// and the in-app prompt: "Zoom is open."
    pub fn reason(&self) -> String {
        match self {
            Self::Calendar { title, attendees } => {
                format!("“{title}” starts in a minute, with {attendees} people invited.")
            }
            Self::Process { process } => format!("{} is open.", processes::label(process)),
            Self::AudioActivity => {
                "Your microphone and speakers are both in use, like on a call.".to_string()
            }
            Self::Call {
                app,
                browser: false,
            } => format!("{app} is using your microphone."),
            Self::Call { app, browser: true } => {
                format!("{app} is using your microphone, like a call in a tab.")
            }
        }
    }
}

/// Everything that can go wrong while detecting.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The process list could not be read.
    #[error("could not read the list of running applications")]
    ProcessList(String),
    /// Whether the default mic and speakers are in use could not be read.
    #[error("could not read whether the microphone and speakers are in use")]
    AudioDevices(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processes::name_of;

    #[test]
    fn the_reason_names_the_app_by_its_label() {
        let zoom = Signal::Process {
            process: name_of("Zoom").to_string(),
        };
        assert_eq!(zoom.reason(), "Zoom is open.");
        let teams = Signal::Process {
            process: name_of("Microsoft Teams").to_string(),
        };
        assert_eq!(teams.reason(), "Microsoft Teams is open.");
    }

    #[test]
    fn every_signal_has_a_reason() {
        let calendar = Signal::Calendar {
            title: "Standup".to_string(),
            attendees: 4,
        };
        assert!(calendar.reason().contains("Standup"));
        assert!(calendar.reason().contains('4'));
        assert!(!Signal::AudioActivity.reason().is_empty());
    }

    #[test]
    fn a_call_names_its_app() {
        let whatsapp = Signal::Call {
            app: "WhatsApp".to_string(),
            browser: false,
        };
        assert_eq!(whatsapp.reason(), "WhatsApp is using your microphone.");
        let chrome = Signal::Call {
            app: "Google Chrome".to_string(),
            browser: true,
        };
        assert_eq!(
            chrome.reason(),
            "Google Chrome is using your microphone, like a call in a tab."
        );
        assert_eq!(
            serde_json::to_value(&chrome).expect("serialises"),
            serde_json::json!({ "kind": "call", "app": "Google Chrome", "browser": true })
        );
    }

    #[test]
    fn a_signal_serialises_with_its_kind() {
        let zoom = Signal::Process {
            process: "zoom.us".to_string(),
        };
        assert_eq!(
            serde_json::to_value(&zoom).expect("serialises"),
            serde_json::json!({ "kind": "process", "process": "zoom.us" })
        );
    }
}
