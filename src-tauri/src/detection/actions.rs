//! The reminder banner's **Join and record**, **Join** and **Record**
//! (TUR-78).
//!
//! A reminder remembers its event ([`Reminded`]), and the banner sends back
//! only that event's id, never a link: the link a click opens is the one the
//! calendar gave, and only a web link ([`join_link`], the same rule as the
//! menu bar's Join). Record pins the event, so the recording is named after
//! this meeting even when the reminder came ten minutes early, and goes
//! through the normal start path (`folder_move::start_recording`, as the menu
//! bar's Record does). Every one of these runs from a click (L15).

use std::collections::VecDeque;

use tauri::{AppHandle, Manager as _};
use tauri_plugin_opener::OpenerExt as _;

use super::Detection;
use crate::error::UiError;
use crate::recording::Status;
use crate::recording::auto_title::PinnedEvent;

/// How many reminded events are kept for the banner: more than can be on
/// screen at once, and few enough to never matter.
const KEEP: usize = 8;

/// The events reminded about lately, newest last.
#[derive(Debug, Default)]
pub struct Reminded(VecDeque<::calendar::Event>);

impl Reminded {
    /// Remember `event`, replacing an older copy of it.
    pub fn remember(&mut self, event: ::calendar::Event) {
        self.0.retain(|known| known.id != event.id);
        self.0.push_back(event);
        while self.0.len() > KEEP {
            self.0.pop_front();
        }
    }

    /// The reminded event called `id`.
    pub fn get(&self, id: &str) -> Option<::calendar::Event> {
        self.0.iter().find(|event| event.id == id).cloned()
    }
}

impl Detection {
    /// A reminder for `event` is being shown.
    pub fn remember_reminded(&self, event: ::calendar::Event) {
        crate::lock::lock_or_recover(&self.reminded).remember(event);
    }

    fn reminded(&self, id: &str) -> Option<::calendar::Event> {
        crate::lock::lock_or_recover(&self.reminded).get(id)
    }
}

/// `event`'s meeting link, if it has one that is a web link. The providers'
/// fields are someone else's data, so nothing else is opened.
pub fn join_link(event: &::calendar::Event) -> Option<&str> {
    let url = event.join_url.as_deref()?.trim();
    let lower = url.to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://")).then_some(url)
}

fn reminded_event(app: &AppHandle, event_id: &str) -> Result<::calendar::Event, UiError> {
    app.try_state::<Detection>()
        .and_then(|detection| detection.reminded(event_id))
        .ok_or_else(|| {
            UiError::app(
                "reminder-gone",
                "That meeting is no longer in meet-ai's reminders.",
            )
        })
}

fn open_link(app: &AppHandle, event: &::calendar::Event) -> Result<(), UiError> {
    let url = join_link(event)
        .ok_or_else(|| UiError::app("no-meeting-link", "This meeting has no link to join."))?;
    app.opener().open_url(url, None::<&str>).map_err(|error| {
        UiError::app(
            "open-failed",
            format!("Could not open the meeting link: {error}"),
        )
    })
}

/// **Join**: open the reminded meeting's link in the browser or the app that
/// owns it.
#[tauri::command]
#[specta::specta]
pub async fn join_reminded_meeting(app: AppHandle, event_id: String) -> Result<(), UiError> {
    let event = reminded_event(&app, &event_id)?;
    open_link(&app, &event)
}

/// **Record**, or with `join` **Join and record**: open the link first (a
/// link that will not open is logged, and the recording still starts), then
/// start a recording named after this meeting.
#[tauri::command]
#[specta::specta]
pub async fn record_reminded_meeting(
    app: AppHandle,
    event_id: String,
    join: bool,
) -> Result<Status, UiError> {
    let event = reminded_event(&app, &event_id)?;
    if join && let Err(error) = open_link(&app, &event) {
        tracing::warn!(message = %error.message, "could not join from the reminder; recording anyway");
    }
    tauri::async_runtime::spawn_blocking(move || {
        let pin = app.state::<PinnedEvent>();
        pin.pin(event);
        let started = crate::folder_move::start_recording(&app);
        // Taken by the start if it got that far; never left for a later one.
        pin.clear();
        started
    })
    .await
    .map_err(|error| UiError::app("record-failed", error.to_string()))?
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone as _, Utc};

    use super::*;

    fn event(id: &str, join_url: Option<&str>) -> ::calendar::Event {
        let start = Utc.with_ymd_and_hms(2026, 10, 5, 10, 0, 0).unwrap();
        ::calendar::Event {
            id: id.to_string(),
            title: format!("{id} title"),
            start,
            end: start + chrono::Duration::minutes(30),
            attendees: 3,
            attendee_names: Vec::new(),
            ical_uid: None,
            join_url: join_url.map(str::to_string),
        }
    }

    #[test]
    fn only_a_web_link_is_joined() {
        assert_eq!(
            join_link(&event("a", Some("https://zoom.us/j/1"))),
            Some("https://zoom.us/j/1")
        );
        assert_eq!(
            join_link(&event("a", Some(" HTTP://meet.example/x "))),
            Some("HTTP://meet.example/x")
        );
        for odd in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "zoommtg://x",
            "",
        ] {
            assert_eq!(join_link(&event("a", Some(odd))), None, "{odd}");
        }
        assert_eq!(join_link(&event("a", None)), None);
    }

    #[test]
    fn reminded_events_are_found_by_id_newest_copy_and_bounded() {
        let mut reminded = Reminded::default();
        reminded.remember(event("a", None));
        reminded.remember(event("a", Some("https://zoom.us/j/1")));
        assert_eq!(
            reminded.get("a").and_then(|event| event.join_url),
            Some("https://zoom.us/j/1".to_string())
        );
        for n in 0..KEEP {
            reminded.remember(event(&format!("e{n}"), None));
        }
        assert_eq!(reminded.get("a"), None, "the oldest goes first");
        assert!(reminded.get(&format!("e{}", KEEP - 1)).is_some());
        assert_eq!(reminded.0.len(), KEEP);
    }
}
