//! Everything the Settings screen reads from `config.jsonc`, in one command
//! (TUR-171).
//!
//! Settings used to ask a dozen commands on every visit, and each one found
//! the meetings root, read `config.jsonc` and parsed it again. This asks the
//! same section readers inside [`config::read_once`], so the root pointer and
//! the file are read once. Each card still saves through its own command and
//! shows what that save returns.
//!
//! Not in here: the engine card (its probe is slow and it paints without
//! waiting on it), the OS answers (start at login, notification permission)
//! and the calendar accounts (the keystore, maybe the network).

use serde::Serialize;

use crate::agent_setup::AgentChoice;
use crate::calendar::sources::CalendarSources;
use crate::config;
use crate::config_problem::ConfigSection;
use crate::detection::settings::NotificationSettings;
use crate::error::{UiError, on_blocking_pool};
use crate::retention::AudioRetentionSetting;
use crate::sync::tracker::TrackerSettings;

/// The saved settings, as each Settings card's own read would answer.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    /// `app.show_in_dock_when_closed` (TUR-76).
    pub show_in_dock_when_closed: bool,
    /// `app.menu_bar_countdown` (TUR-77).
    pub menu_bar_countdown: bool,
    /// What in `app` was not valid (TUR-155), as `config_problem` says.
    pub app_problem: Option<String>,
    /// What in `appearance` was not valid.
    pub appearance_problem: Option<String>,
    /// What in `detection` was not valid.
    pub detection_problem: Option<String>,
    /// `audio.retention_days`, as the retention job reads it.
    pub audio_retention: AudioRetentionSetting,
    /// `audio.use_builtin_mic_with_bluetooth` (TUR-91).
    pub builtin_mic_with_bluetooth: bool,
    /// `audio.show_recording_overlay` (TUR-146).
    pub show_recording_overlay: bool,
    /// The agent pick; `None` when `agent` could not be read (`agent_error`).
    pub agent_choice: Option<AgentChoice>,
    /// `agent.auto_run` (TUR-101); `None` with `agent_error`.
    pub notes_auto_run: Option<bool>,
    /// Why `agent` could not be read, such as an unknown `agent.harness`.
    pub agent_error: Option<UiError>,
    /// The Notifications card (TUR-78).
    pub notifications: NotificationSettings,
    /// The Calendars card's sources (TUR-49); the accounts are read apart.
    pub calendar_sources: CalendarSources,
    /// The Tracker card (TUR-113); `None` with `tracker_error`.
    pub tracker: Option<TrackerSettings>,
    pub tracker_error: Option<UiError>,
}

/// Read every section the snapshot holds. Call inside [`config::read_once`].
fn read() -> SettingsSnapshot {
    let app = config::app_checked();
    let detection = config::detection_checked();
    let (agent_choice, notes_auto_run, agent_error) = match config::agent() {
        Ok(agent) => (
            Some(AgentChoice::from_config(&agent)),
            Some(agent.auto_run),
            None,
        ),
        Err(error) => (None, None, Some(UiError::from(error))),
    };
    let (tracker, tracker_error) = split(crate::sync::tracker::current());
    SettingsSnapshot {
        show_in_dock_when_closed: app.value.show_in_dock_when_closed,
        menu_bar_countdown: app.value.menu_bar_countdown,
        app_problem: app.problem,
        appearance_problem: crate::config_problem::problem(ConfigSection::Appearance),
        detection_problem: detection.problem,
        audio_retention: config::audio().into(),
        builtin_mic_with_bluetooth: config::use_builtin_mic_with_bluetooth(),
        show_recording_overlay: config::show_recording_overlay(),
        agent_choice,
        notes_auto_run,
        agent_error,
        notifications: NotificationSettings::from(detection.value),
        calendar_sources: crate::calendar::sources::current(),
        tracker,
        tracker_error,
    }
}

fn split<T>(result: Result<T, UiError>) -> (Option<T>, Option<UiError>) {
    match result {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error)),
    }
}

/// Every saved setting the Settings screen shows, from one read of the
/// meetings root and `config.jsonc`. Disk, so the blocking pool.
#[tauri::command]
#[specta::specta]
pub async fn settings_snapshot() -> Result<SettingsSnapshot, UiError> {
    on_blocking_pool(|| config::read_once(read)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_read_is_its_error_and_no_value() {
        let error = UiError::app("unknown-harness", "agent.harness \"x\" is not known");
        let (value, kept) = split::<bool>(Err(error));
        assert!(value.is_none());
        assert_eq!(kept.unwrap().kind, "unknown-harness");
        let (value, kept) = split::<bool>(Ok(true));
        assert_eq!(value, Some(true));
        assert!(kept.is_none());
    }

    #[test]
    fn every_section_comes_from_the_one_read() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.jsonc");
        std::fs::write(
            &path,
            r#"{
                "app": { "show_in_dock_when_closed": true, "menu_bar_countdown": true },
                "appearance": { "theme": 3 },
                "audio": { "retention_days": 30, "use_builtin_mic_with_bluetooth": true },
                "agent": { "harness": "codex", "auto_run": false },
                "detection": { "min_attendees": 3 }
            }"#,
        )
        .unwrap();
        let snapshot = config::read_once_in(temp.path(), || {
            // Gone from disk after the one read: every answer below is from it.
            std::fs::remove_file(&path).unwrap();
            read()
        });
        assert!(snapshot.show_in_dock_when_closed);
        assert!(snapshot.menu_bar_countdown);
        assert_eq!(snapshot.app_problem, None);
        assert!(snapshot.appearance_problem.is_some());
        assert_eq!(snapshot.detection_problem, None);
        assert_eq!(
            snapshot.audio_retention,
            AudioRetentionSetting::Running { days: 30 }
        );
        assert!(snapshot.builtin_mic_with_bluetooth);
        assert!(snapshot.show_recording_overlay);
        assert_eq!(snapshot.notes_auto_run, Some(false));
        assert!(snapshot.agent_error.is_none());
        assert_eq!(snapshot.notifications.min_attendees, 3);
        assert!(snapshot.tracker.is_some(), "{:?}", snapshot.tracker_error);

        let wire = serde_json::to_value(&snapshot).unwrap();
        for key in [
            "showInDockWhenClosed",
            "menuBarCountdown",
            "appProblem",
            "appearanceProblem",
            "detectionProblem",
            "audioRetention",
            "builtinMicWithBluetooth",
            "showRecordingOverlay",
            "agentChoice",
            "notesAutoRun",
            "agentError",
            "notifications",
            "calendarSources",
            "tracker",
            "trackerError",
        ] {
            assert!(wire.get(key).is_some(), "{key} missing from {wire}");
        }
    }

    #[test]
    fn an_unknown_agent_is_the_agent_error_and_the_rest_still_reads() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("config.jsonc"),
            r#"{ "agent": { "harness": "made-up" }, "app": { "menu_bar_countdown": true } }"#,
        )
        .unwrap();
        let snapshot = config::read_once_in(temp.path(), read);
        assert!(snapshot.agent_choice.is_none());
        assert!(snapshot.notes_auto_run.is_none());
        assert!(snapshot.agent_error.is_some());
        assert!(snapshot.menu_bar_countdown);
    }
}
