//! The one-off "meet-ai is still running" notice, the first time the main
//! window is closed (TUR-76), and the flag that keeps it one-off.
//!
//! The flag lives in `~/Meetings/.app/state.json`, inside the meetings root
//! because L10 says the app writes nothing outside it. Losing the file shows
//! the notice once more, which is the harmless way to fail.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::UiError;

const FILE: &str = "state.json";

/// The app's small state file. Unknown keys are kept on a write, so a later
/// flag can share the file without this code knowing about it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    /// The "still running in the menu bar" notice has been shown.
    #[serde(default)]
    pub still_running_notice_shown: bool,
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

/// Whether this close should post the notice: only if it never has.
pub fn should_notify(state: &AppState) -> bool {
    !state.still_running_notice_shown
}

fn path() -> Result<PathBuf, UiError> {
    Ok(meeting_format::layout::app_dir(&crate::meetings::root()?).join(FILE))
}

/// The state in `dir`. A missing or corrupt file is the defaults, logged.
pub fn read_in(dir: &Path) -> AppState {
    let path = dir.join(FILE);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return AppState::default();
    };
    serde_json::from_str(&raw).unwrap_or_else(|error| {
        tracing::warn!(%error, path = %path.display(), "unreadable app state; using defaults");
        AppState::default()
    })
}

/// Write `state` into `dir`, atomically.
pub fn write_in(dir: &Path, state: &AppState) -> Result<(), UiError> {
    std::fs::create_dir_all(dir)?;
    let body = serde_json::to_string_pretty(state)
        .map_err(|error| UiError::app("serialize", error.to_string()))?;
    meeting_format::write_atomic(&dir.join(FILE), body.as_bytes())?;
    Ok(())
}

/// Post the notice if it has never been posted, and remember that it was.
/// The flag is set before the notification, so a failing notification is
/// never retried on every close.
pub fn show_once(app: &tauri::AppHandle) {
    use tauri_plugin_notification::NotificationExt as _;

    let dir = match path() {
        Ok(path) => path.parent().map(Path::to_path_buf).unwrap_or_default(),
        Err(error) => {
            tracing::warn!(message = %error.message, "no meetings folder; not showing the still-running notice");
            return;
        }
    };
    let mut state = read_in(&dir);
    if !should_notify(&state) {
        return;
    }
    state.still_running_notice_shown = true;
    let saved = crate::folder_move::writing_in_root(app, |_root| write_in(&dir, &state));
    if let Err(error) = saved {
        // Not shown either: a notice that comes back on every close is worse
        // than none.
        tracing::warn!(message = %error.message, "could not save the still-running flag; not showing the notice");
        return;
    }
    if let Err(error) = app
        .notification()
        .builder()
        .title("meet-ai")
        .body(crate::platform::still_running_notice())
        .show()
    {
        tracing::warn!(%error, "could not show the still-running notice");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notice_is_shown_once_and_never_again() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("app-dir");

        let first = read_in(&dir);
        assert!(should_notify(&first), "a fresh install has never seen it");

        let shown = AppState {
            still_running_notice_shown: true,
            ..first
        };
        write_in(&dir, &shown).unwrap();
        assert!(
            !should_notify(&read_in(&dir)),
            "a later close, or a relaunch"
        );
    }

    #[test]
    fn other_keys_in_the_file_survive_a_write() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join(FILE),
            r#"{ "somethingElse": 3, "stillRunningNoticeShown": false }"#,
        )
        .unwrap();
        let mut state = read_in(temp.path());
        state.still_running_notice_shown = true;
        write_in(temp.path(), &state).unwrap();

        let raw = std::fs::read_to_string(temp.path().join(FILE)).unwrap();
        assert!(raw.contains(r#""somethingElse": 3"#), "{raw}");
        assert!(raw.contains(r#""stillRunningNoticeShown": true"#), "{raw}");
    }

    #[test]
    fn a_corrupt_file_reads_as_not_shown() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join(FILE), "{ not json").unwrap();
        assert!(should_notify(&read_in(temp.path())));
    }
}
