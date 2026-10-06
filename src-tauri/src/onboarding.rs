//! Remembering whether onboarding has been done.
//!
//! SPEC §8.1 makes `/onboarding` its own route in v1 rather than a first-launch
//! special case, so this is one flag, not a wizard framework. It lives at
//! `~/Meetings/.app/onboarding.json` — inside the meetings root, because L10
//! says the app writes nothing outside it.
//!
//! Losing this file re-shows onboarding. That is the right failure: a user who
//! sees the permission steps twice is mildly annoyed, and a user who never sees
//! them records silence.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::UiError;

const FILE: &str = "onboarding.json";

/// Whether onboarding is done, and when.
///
/// One nullable timestamp rather than a boolean plus a date: two fields can
/// disagree, and "completed but with no date" is a state nobody wants to have
/// to reason about. The frontend reads `completedAt !== null`.
#[derive(Debug, Clone, Default, Serialize, specta::Type, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    /// When the user finished onboarding, as an RFC 3339 string. `None` means
    /// they have not.
    pub completed_at: Option<String>,
}

fn path() -> Result<PathBuf, UiError> {
    Ok(meeting_format::layout::app_dir(&crate::meetings::root()?).join(FILE))
}

/// Read the flag. An unreadable or corrupt file means "not onboarded" rather
/// than an error screen — there is nothing the user could do about it, and
/// re-running onboarding fixes it by rewriting the file.
pub fn state() -> Result<State, UiError> {
    let path = path()?;
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Ok(State::default());
    };
    match serde_json::from_str(&raw) {
        Ok(state) => Ok(state),
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "unreadable onboarding flag; treating it as not onboarded");
            Ok(State::default())
        }
    }
}

/// Mark onboarding done.
pub fn complete() -> Result<State, UiError> {
    let state = State {
        completed_at: Some(chrono::Local::now().to_rfc3339()),
    };
    write(&state)?;
    Ok(state)
}

/// Forget it, so the flow can be walked again from Settings. Useful to a user
/// whose permissions changed, and to anyone reviewing the screens.
pub fn reset() -> Result<State, UiError> {
    let state = State::default();
    write(&state)?;
    Ok(state)
}

fn write(state: &State) -> Result<(), UiError> {
    let path = path()?;
    if let Some(parent) = path.parent() {
        store::create_app_dir(parent)?;
    }
    let body = serde_json::to_string_pretty(state)
        .map_err(|error| UiError::app("serialize", error.to_string()))?;
    std::fs::write(&path, body)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_state_has_not_been_onboarded() {
        assert!(State::default().completed_at.is_none());
    }

    #[test]
    fn the_flag_round_trips_through_the_file_format() {
        let written = State {
            completed_at: Some("2026-09-27T13:00:00+05:30".into()),
        };
        let json = serde_json::to_string(&written).expect("serializes");
        // camelCase on the wire, because that is what the webview reads.
        assert!(json.contains("completedAt"), "{json}");
        let read: State = serde_json::from_str(&json).expect("round-trips");
        assert_eq!(read.completed_at, written.completed_at);
    }

    /// The other tests exercise (de)serialization only. This is the one that
    /// goes through `path()` and the real filesystem, which is what a genuine
    /// app restart actually depends on to see a finished setup stay finished.
    #[test]
    fn completing_onboarding_survives_a_fresh_read_through_the_real_root() {
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: this test does not run other tests concurrently that read
        // `MEET_AI_MEETINGS_ROOT`, and it is restored before returning.
        unsafe { std::env::set_var("MEET_AI_MEETINGS_ROOT", dir.path()) };

        assert!(state().unwrap().completed_at.is_none());
        let written = complete().unwrap();
        assert!(written.completed_at.is_some());
        // A fresh `state()` call, as a new app launch would make it, must see
        // exactly what `complete()` just wrote rather than a cached value.
        assert_eq!(state().unwrap().completed_at, written.completed_at);

        unsafe { std::env::remove_var("MEET_AI_MEETINGS_ROOT") };
    }

    #[test]
    fn a_corrupt_flag_reads_as_not_onboarded_rather_than_failing() {
        // The deserialize half of the rule in the module docs: garbage in the
        // file must not become an error the user cannot act on.
        assert!(serde_json::from_str::<State>("{ not json").is_err());
    }
}
