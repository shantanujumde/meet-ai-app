//! Tier 3 (SPEC §2.7, A12): a Google account's primary calendar, on every OS
//! (TUR-48). The main calendar on Linux, and on a Mac whose Calendar app has
//! no Google account.
//!
//! One read is `GET calendars/primary/events` for the range, page by page
//! through `nextPageToken`. `singleEvents=true` makes Google expand repeating
//! meetings itself, so every occurrence comes back as its own event with its
//! own id: no RRULE work here. The scope is `calendar.events.readonly`
//! (read-only), asked for at sign-in (`oauth::ProviderId::Google`).
//!
//! - Times: the request says `timeZone=UTC`. A timed event has
//!   `start.dateTime`; an all-day one has only `start.date` and is skipped.
//! - Skipped, like EventKit: cancelled events, events the signed-in account
//!   (the attendee with `self: true`) declined, and all-day events. Rooms and
//!   other resources (`resource: true`) are not counted as attendees.
//! - Tokens: [`TokenSource`], which the app answers from
//!   [`CalendarAuth`](crate::oauth::CalendarAuth). A 401 renews the token
//!   once; a second 401 (or a refused refresh) is [`Error::SignInExpired`].
//! - Network: [`HttpClient`], the same one the sign-in uses, so this crate
//!   has no HTTP stack and the tests answer for Google from fixture JSON.

use chrono::{DateTime, SecondsFormat, Utc};
use oauth2::HttpResponse;
use oauth2::http::StatusCode;
use oauth2::url::Url;

use crate::cloud::{self, Api, IntoTokenSource};
use crate::join_url::{extract_join_url, is_safe_join_url};
use crate::oauth::{HttpClient, ProviderId};
use crate::raw::{RawAttendee, RawEvent, to_events};
use crate::{CalendarProvider, Error, Event};

pub mod types;

use types::{AttendeeResponseStatus, EventStatus};

pub use crate::cloud::TokenSource;

/// The name in errors and on the settings screen: "Google".
const PROVIDER: &str = "Google";

/// How [`cloud::read_pages`] calls Google Calendar.
const API: Api = Api {
    provider: PROVIDER,
    request_name: "Google Calendar",
    pages_name: "Google",
    headers: &[],
};

/// Every read: the signed-in account's primary calendar.
pub const EVENTS_URL: &str = "https://www.googleapis.com/calendar/v3/calendars/primary/events";

/// Only the fields [`types::Event`] reads, so a busy calendar stays small.
pub const FIELDS: &str = "nextPageToken,items(id,status,summary,start,end,recurringEventId,iCalUID,attendees(email,displayName,self,resource,responseStatus),hangoutLink,conferenceData(entryPoints(entryPointType,uri)),location,description)";

/// Events per page: Google's default is 250, its maximum 2500.
pub const PAGE_SIZE: u32 = 250;

/// The signed-in Google account's primary calendar.
pub struct GoogleProvider {
    tokens: Box<dyn TokenSource>,
    http: Box<dyn HttpClient>,
}

impl GoogleProvider {
    /// `tokens`: any [`TokenSource`], or the app's
    /// [`CalendarAuth`](crate::oauth::CalendarAuth), which answers for Google.
    pub fn new<T: IntoTokenSource + ?Sized>(tokens: Box<T>, http: Box<dyn HttpClient>) -> Self {
        Self {
            tokens: tokens.into_token_source(ProviderId::Google),
            http,
        }
    }
}

impl CalendarProvider for GoogleProvider {
    fn name(&self) -> &'static str {
        PROVIDER
    }

    fn list_events(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<Event>, Error> {
        let first = events_url(from, to, None)?;
        let found = cloud::read_pages(
            &API,
            self.tokens.as_ref(),
            self.http.as_ref(),
            first,
            |response| {
                let page = read_page(response)?;
                let next = match page.next_page_token.filter(|next| !next.is_empty()) {
                    Some(next) => Some(events_url(from, to, Some(&next))?),
                    None => None,
                };
                Ok((page.items, next))
            },
        )?;
        Ok(to_events(found.into_iter().filter_map(raw_event), from, to))
    }
}

/// `GET calendars/primary/events` for `[from, to)`: the first page, or the
/// one `page_token` names.
pub fn events_url(
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    page_token: Option<&str>,
) -> Result<Url, Error> {
    let mut url = Url::parse(EVENTS_URL).map_err(|error| unreachable(error.to_string()))?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("timeMin", &from.to_rfc3339_opts(SecondsFormat::Secs, true))
            .append_pair("timeMax", &to.to_rfc3339_opts(SecondsFormat::Secs, true))
            .append_pair("singleEvents", "true")
            .append_pair("orderBy", "startTime")
            .append_pair("timeZone", "UTC")
            .append_pair("maxResults", &PAGE_SIZE.to_string())
            .append_pair("fields", FIELDS);
        if let Some(page_token) = page_token {
            query.append_pair("pageToken", page_token);
        }
    }
    Ok(url)
}

/// One page, or the error its status means.
fn read_page(response: &HttpResponse) -> Result<types::ListEventsResponse, Error> {
    let status = response.status();
    if status == StatusCode::UNAUTHORIZED {
        tracing::warn!("Google Calendar rejected a fresh access token; sign in again");
        return Err(Error::SignInExpired { provider: PROVIDER });
    }
    if !status.is_success() {
        // Google's status and reason ("PERMISSION_DENIED",
        // "accessNotConfigured"), never the message: it can name the account.
        let body = serde_json::from_slice::<types::ErrorResponse>(response.body())
            .map(|body| body.error)
            .unwrap_or_default();
        let named: Vec<String> = body
            .status
            .into_iter()
            .chain(body.errors.into_iter().filter_map(|item| item.reason))
            .collect();
        let detail = if named.is_empty() {
            String::new()
        } else {
            format!(" ({})", named.join(", "))
        };
        return Err(unreachable(format!(
            "Google Calendar answered {}{detail}",
            status.as_u16()
        )));
    }
    serde_json::from_slice(response.body())
        .map_err(|error| unreachable(format!("unreadable reply from Google Calendar: {error}")))
}

/// One Google event as plain data. `None` for an event
/// meet-ai never shows (cancelled, all-day) or one whose times cannot be
/// read. A declined event comes back with its "me" attendee marked, and
/// [`to_events`] drops it.
fn raw_event(event: types::Event) -> Option<RawEvent> {
    if event.status == Some(EventStatus::Cancelled) {
        return None;
    }
    let start = event.start.as_ref()?;
    let end = event.end.as_ref()?;
    let (Some(start), Some(end)) = (start.date_time, end.date_time) else {
        // `date` alone: an all-day event. Never a call.
        return None;
    };
    let attendees = event
        .attendees
        .unwrap_or_default()
        .into_iter()
        .map(|attendee| RawAttendee {
            name: attendee.display_name,
            email: attendee.email,
            is_me: attendee.is_self.unwrap_or(false),
            declined: attendee.response_status == Some(AttendeeResponseStatus::Declined),
            resource: attendee.resource.unwrap_or(false),
        })
        .collect();
    let raw = RawEvent {
        id: event.id,
        title: event.summary.unwrap_or_default(),
        start: start.with_timezone(&Utc),
        end: end.with_timezone(&Utc),
        all_day: false,
        attendees,
        ical_uid: event.ical_uid,
        join_url: join_url(
            event.hangout_link,
            event.conference_data,
            event.location.as_deref(),
            event.description.as_deref(),
        ),
    };
    Some(raw)
}

/// The link that joins the call (TUR-77): `hangoutLink` for Meet, else the
/// conference's `video` entry point (a Zoom or other add-on), else a link
/// pasted into the location or description (TUR-86). Blank links, and links
/// that fail [`is_safe_join_url`] (an organiser sets the conference too),
/// are skipped.
fn join_url(
    hangout_link: Option<String>,
    conference: Option<types::ConferenceData>,
    location: Option<&str>,
    description: Option<&str>,
) -> Option<String> {
    let video = conference
        .into_iter()
        .flat_map(|c| c.entry_points)
        .filter(|entry| entry.entry_point_type.as_deref() == Some("video"))
        .filter_map(|entry| entry.uri);
    hangout_link
        .into_iter()
        .chain(video)
        .map(|url| url.trim().to_owned())
        .find(|url| is_safe_join_url(url))
        .or_else(|| extract_join_url(None, location, description))
}

fn unreachable(detail: impl Into<String>) -> Error {
    cloud::unreachable(PROVIDER, detail)
}
