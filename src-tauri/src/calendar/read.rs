//! One read of every provider (TUR-174): all at once, and each failure kept.
//!
//! Providers are read in parallel, one scoped thread each, so a Calendar.app
//! waiting minutes on its permission prompt does not hold up a Google or
//! Microsoft read. Each provider that fails is kept next to the events the
//! others gave, so the Today pane can say "your Google sign-in has expired"
//! while still showing the Calendar.app meetings, instead of a partial day
//! with no warning.

use ::calendar::{Error, Event};
use chrono::{DateTime, Utc};
use serde::Serialize;

use super::SharedProvider;
use crate::error::UiError;

/// What one read found.
#[derive(Debug)]
pub struct Read {
    /// Every answering provider's events, merged and sorted (TUR-49).
    pub events: Vec<Event>,
    /// The providers that did not answer, in the order they were asked.
    pub failed: Vec<Failed>,
}

/// One provider that could not be read.
#[derive(Debug)]
pub struct Failed {
    /// [`::calendar::CalendarProvider::name`].
    pub provider: &'static str,
    pub error: Error,
}

/// Read every provider in parallel and merge the answers.
///
/// `Ok` when at least one provider answered (or there were none to ask), with
/// the others in [`Read::failed`]. When every provider failed, the first
/// one's error, so a denied calendar is never an empty day.
pub fn read_all(
    providers: &[SharedProvider],
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<Read, Error> {
    let answers = list_all(providers, from, to);
    let mut sources = Vec::new();
    let mut failed = Vec::new();
    for (provider, answer) in providers.iter().zip(answers) {
        match answer {
            Ok(found) => sources.push(found),
            Err(error) => {
                tracing::warn!(provider = provider.name(), %error, "could not read a calendar");
                failed.push(Failed {
                    provider: provider.name(),
                    error,
                });
            }
        }
    }
    if sources.is_empty() && !failed.is_empty() {
        return Err(failed.remove(0).error);
    }
    // TUR-49: one sorted list, a meeting in two calendars shown once. The
    // answers keep the providers' order, so EventKit's ids still win.
    Ok(Read {
        events: ::calendar::merge::merge_events(sources),
        failed,
    })
}

/// Each provider's answer, in the providers' order.
fn list_all(
    providers: &[SharedProvider],
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Vec<Result<Vec<Event>, Error>> {
    if let [only] = providers {
        // Nothing to wait on in parallel.
        return vec![only.list_events(from, to)];
    }
    std::thread::scope(|scope| {
        let pending: Vec<_> = providers
            .iter()
            .map(|provider| {
                let spawned = std::thread::Builder::new()
                    .name("meet-ai-calendar-read".to_string())
                    .spawn_scoped(scope, move || provider.list_events(from, to));
                (provider, spawned)
            })
            .collect();
        pending
            .into_iter()
            .map(|(provider, spawned)| match spawned {
                Ok(handle) => handle.join().unwrap_or_else(|_| {
                    Err(Error::Unreachable {
                        provider: provider.name(),
                        detail: "the calendar read stopped unexpectedly".to_string(),
                    })
                }),
                // No thread to spare: read it here instead.
                Err(error) => {
                    tracing::warn!(%error, "could not start a calendar read thread");
                    provider.list_events(from, to)
                }
            })
            .collect()
    })
}

/// A calendar the Today pane could not read, while others answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UnreadableCalendar {
    /// The provider's display name, like "Google".
    pub provider: String,
    /// The error kind, as in [`UiError::kind`]: `calendar-sign-in-expired`,
    /// `calendar-denied` or `calendar-unreachable`.
    pub kind: String,
    /// The error's own sentence. Display it; do not parse it.
    pub message: String,
}

impl From<Failed> for UnreadableCalendar {
    fn from(failed: Failed) -> Self {
        let ui = UiError::from(failed.error);
        Self {
            provider: failed.provider.to_string(),
            kind: ui.kind.to_string(),
            message: ui.message,
        }
    }
}
