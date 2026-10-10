//! The defaults the window shows with no Rust side to ask (a plain browser,
//! a component test) or before Rust has answered (TUR-173).
//!
//! Each one is built from the same `Default` the config reader falls back on,
//! and reaches TypeScript as a `DEFAULT_*` constant in `bindings.ts` (see
//! [`crate::bindings`]), so a changed default is changed in one place. They
//! must not depend on this machine or this OS: `bindings.ts` is generated on
//! every OS and has to come out the same.

use crate::agent_setup::AgentChoice;
use crate::calendar::TodaysMeetings;
use crate::calendar::sources::CalendarSources;
use crate::config::{
    self, AgentConfig, AppConfig, AppearanceConfig, CalendarConfig, DetectionConfig,
    RetentionPolicy,
};
use crate::detection::settings::NotificationSettings;
use crate::engine::{DEFAULT_LOCALE, EnvironmentView};
use crate::lifecycle::AppSettings;
use crate::retention::AudioRetentionSetting;
use crate::sync::tracker::{self, TrackerSettings};

/// `detection`'s defaults as Settings, Notifications shows them.
pub fn notification_settings() -> NotificationSettings {
    DetectionConfig::default().into()
}

/// `audio.retention_days`'s default, as Settings' retention line reads it.
pub fn audio_retention() -> AudioRetentionSetting {
    RetentionPolicy::Run(store::retention::Retention::default()).into()
}

/// `agent`'s defaults as the Setup screen shows them.
pub fn agent_choice() -> AgentChoice {
    AgentChoice::from_config(&AgentConfig::default())
}

/// `agent.auto_run`'s default.
pub fn notes_auto_run() -> bool {
    AgentConfig::default().auto_run
}

/// `audio.use_builtin_mic_with_bluetooth`'s default.
pub fn builtin_mic_with_bluetooth() -> bool {
    config::DEFAULT_BUILTIN_MIC_WITH_BLUETOOTH
}

/// An empty day at `calendar.refresh_minutes` and `detection.min_attendees`'
/// defaults. The macOS calendar defaults, so the answer is the same on every
/// OS; `refresh_minutes` does not differ between them.
pub fn todays_meetings() -> TodaysMeetings {
    TodaysMeetings::new(
        Vec::new(),
        CalendarConfig::defaults_for(true).refresh_minutes,
        DetectionConfig::default().min_attendees,
    )
}

/// A fresh Mac's calendar sources: the Calendar app on, no sign-in set up.
pub fn calendar_sources() -> CalendarSources {
    CalendarSources::new(&CalendarConfig::defaults_for(true), true)
}

/// The `app` section's defaults as Settings shows them.
pub fn app_settings() -> AppSettings {
    AppConfig::default().into()
}

/// `app.menu_bar_countdown`'s default.
pub fn menu_bar_countdown() -> bool {
    AppConfig::default().menu_bar_countdown
}

/// `appearance`'s defaults.
pub fn appearance() -> AppearanceConfig {
    AppearanceConfig::default()
}

/// The tracker settings before any are saved: the shipped defaults, not
/// chosen.
pub fn tracker_settings() -> TrackerSettings {
    tracker::defaults()
}

/// What the engine screen says before the filesystem was looked at. The
/// model is the one `config.schema.json` documents as the default; a real
/// machine picks by hardware tier ([`crate::engine::default_model`]).
pub fn engine_environment() -> EnvironmentView {
    EnvironmentView {
        sidecar: None,
        whisper_model: None,
        locale: DEFAULT_LOCALE.to_owned(),
        model_id: stt::model::LARGE_MODEL.to_owned(),
        models_dir: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The values the window relied on before they came from here, so moving
    /// them changed nothing the user sees.
    #[test]
    fn the_defaults_are_the_shipped_ones() {
        let notifications = notification_settings();
        assert_eq!(notifications.min_attendees, 2);
        assert!(notifications.remind && notifications.processes);
        assert_eq!(
            audio_retention(),
            AudioRetentionSetting::Running { days: 7 }
        );
        assert_eq!(agent_choice().model, "");
        assert!(notes_auto_run());
        assert!(builtin_mic_with_bluetooth());
        assert_eq!(todays_meetings().refresh_minutes, 15);
        assert_eq!(todays_meetings().min_attendees, 2);
        assert!(calendar_sources().calendar_app);
        assert!(!app_settings().show_in_dock_when_closed);
        assert!(!menu_bar_countdown());
        assert!(appearance().glass);
        let tracker = tracker_settings();
        assert_eq!(tracker.tracker, tracker::Tracker::Linear);
        assert!(!tracker.chosen);
        assert_eq!(engine_environment().locale, "en-US");
    }
}
