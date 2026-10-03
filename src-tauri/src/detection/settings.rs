//! Settings → Notifications (TUR-78): the commands behind the card.
//!
//! The card edits the `detection` section of `config.jsonc` through the same
//! comment-keeping writer as the agent and speech cards, and hands the saved
//! section to the running loops ([`super::live`]), so nothing restarts. It
//! also says when the OS is blocking meet-ai's notifications, opens the OS
//! page that allows them (through [`crate::platform`], rule R10), and sends
//! a test reminder.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager as _};
use tauri_plugin_opener::OpenerExt as _;

use super::Detection;
use crate::config::{self, DetectionConfig};
use crate::error::{UiError, on_blocking_pool};
use crate::folder_move::FolderGate;

/// The fewest attendees the card offers for "Only for meetings with at
/// least N people".
pub const MIN_ATTENDEES_FLOOR: u32 = 1;
/// The most.
pub const MIN_ATTENDEES_CEILING: u32 = 10;
/// The longest lead time, as `config.schema.json` has it.
pub const LONGEST_LEAD_MINUTES: u32 = 15;

/// What the Notifications card shows and saves: `detection` in
/// `config.jsonc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSettings {
    /// "Remind me before meetings": `detection.calendar`.
    pub remind: bool,
    /// The lead time: `detection.remind_before_minutes`, 0 to 15.
    pub remind_before_minutes: u32,
    /// "Ask when a meeting app is running": `detection.processes`.
    pub processes: bool,
    /// "Ask when my mic and speakers are both in use":
    /// `detection.audio_activity`.
    pub audio_activity: bool,
    /// "Only for meetings with at least N people": `detection.min_attendees`.
    pub min_attendees: u32,
}

impl From<DetectionConfig> for NotificationSettings {
    fn from(config: DetectionConfig) -> Self {
        Self {
            remind: config.calendar,
            remind_before_minutes: config.remind_before_minutes,
            processes: config.processes,
            audio_activity: config.audio_activity,
            min_attendees: config.min_attendees,
        }
    }
}

impl NotificationSettings {
    /// The `detection` section to save, or why these cannot be saved.
    pub fn to_config(self) -> Result<DetectionConfig, UiError> {
        if self.remind_before_minutes > LONGEST_LEAD_MINUTES {
            return Err(UiError::app(
                "invalid-setting",
                "The reminder can be at most 15 minutes before the meeting.",
            ));
        }
        if !(MIN_ATTENDEES_FLOOR..=MIN_ATTENDEES_CEILING).contains(&self.min_attendees) {
            return Err(UiError::app(
                "invalid-setting",
                "The number of people must be from 1 to 10.",
            ));
        }
        Ok(DetectionConfig {
            calendar: self.remind,
            processes: self.processes,
            audio_activity: self.audio_activity,
            min_attendees: self.min_attendees,
            remind_before_minutes: self.remind_before_minutes,
        })
    }
}

/// `detection` from `config.jsonc`, defaults when missing or not valid.
#[tauri::command]
#[specta::specta]
pub async fn notification_settings() -> Result<NotificationSettings, UiError> {
    on_blocking_pool(|| NotificationSettings::from(config::detection())).await
}

/// Save the card and return what was saved. The loops follow from their
/// next tick. Writes under the meetings root, so through the [`FolderGate`].
#[tauri::command]
#[specta::specta]
pub async fn set_notification_settings(
    app: AppHandle,
    settings: NotificationSettings,
) -> Result<NotificationSettings, UiError> {
    let wanted = settings.to_config()?;
    let handle = app.clone();
    let saved = on_blocking_pool(move || {
        handle
            .state::<FolderGate>()
            .writing(|| Ok(config::set_detection(&wanted)?))
    })
    .await??;
    if let Some(detection) = app.try_state::<Detection>() {
        detection.live.set(saved);
    }
    tracing::info!(?saved, "notification settings saved");
    Ok(NotificationSettings::from(saved))
}

/// Is the OS blocking meet-ai's notifications? The card then offers the OS
/// page that allows them.
#[tauri::command]
#[specta::specta]
pub async fn os_notifications_blocked(app: AppHandle) -> bool {
    crate::platform::notifications_blocked(&app)
}

/// Open the OS page where meet-ai's notifications are allowed.
#[tauri::command]
#[specta::specta]
pub async fn open_notification_settings(app: AppHandle) -> Result<(), UiError> {
    let url = crate::platform::notification_settings_url().ok_or_else(|| {
        UiError::app(
            "unsupported",
            "Open your desktop's notification settings to allow meet-ai.",
        )
    })?;
    app.opener().open_url(url, None::<&str>).map_err(|error| {
        UiError::app(
            "open-failed",
            format!("meet-ai could not open the notification settings: {error}"),
        )
    })
}

/// "Send a test reminder": a reminder for a fake "Test meeting" starting in
/// the configured lead time, through the real prompt path. `false` when
/// nothing was shown because a recording is running. Never records.
#[tauri::command]
#[specta::specta]
pub async fn send_test_reminder(app: AppHandle) -> bool {
    let minutes = app
        .try_state::<Detection>()
        .map_or(config::DEFAULT_REMIND_BEFORE_MINUTES, |detection| {
            detection.config().remind_before_minutes
        });
    super::notify::test_reminder(&app, minutes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> NotificationSettings {
        NotificationSettings::from(DetectionConfig::default())
    }

    #[test]
    fn the_card_shows_the_config_keys() {
        assert_eq!(
            settings(),
            NotificationSettings {
                remind: true,
                remind_before_minutes: 1,
                processes: true,
                audio_activity: true,
                min_attendees: 2,
            }
        );
    }

    #[test]
    fn each_card_control_is_its_config_key() {
        let changed = NotificationSettings {
            remind: false,
            remind_before_minutes: 10,
            processes: false,
            audio_activity: false,
            min_attendees: 5,
        };
        assert_eq!(
            changed.to_config().unwrap(),
            DetectionConfig {
                calendar: false,
                processes: false,
                audio_activity: false,
                min_attendees: 5,
                remind_before_minutes: 10,
            }
        );
        assert_eq!(
            NotificationSettings::from(changed.to_config().unwrap()),
            changed
        );
    }

    #[test]
    fn out_of_range_values_are_refused() {
        for bad in [
            NotificationSettings {
                remind_before_minutes: 16,
                ..settings()
            },
            NotificationSettings {
                min_attendees: 0,
                ..settings()
            },
            NotificationSettings {
                min_attendees: 11,
                ..settings()
            },
        ] {
            assert!(bad.to_config().is_err(), "{bad:?}");
        }
        for ok in [0, 15] {
            assert!(
                NotificationSettings {
                    remind_before_minutes: ok,
                    ..settings()
                }
                .to_config()
                .is_ok()
            );
        }
    }

    #[test]
    fn the_wire_shape_is_camel_case() {
        assert_eq!(
            serde_json::to_value(settings()).unwrap(),
            serde_json::json!({
                "remind": true,
                "remindBeforeMinutes": 1,
                "processes": true,
                "audioActivity": true,
                "minAttendees": 2
            })
        );
    }
}
