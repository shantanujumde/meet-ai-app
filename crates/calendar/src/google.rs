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
use oauth2::http::{Method, Request, StatusCode, header};
use oauth2::url::Url;
use oauth2::{HttpRequest, HttpResponse};

use crate::oauth::{CalendarAuth, HttpClient, ProviderId};
use crate::raw::{RawAttendee, RawEvent, to_events};
use crate::{CalendarProvider, Error, Event};

pub mod types;

use types::{AttendeeResponseStatus, EventStatus};

/// The name in errors and on the settings screen: "Google".
fn provider_name() -> &'static str {
    ProviderId::Google.display_name()
}

/// Every read: the signed-in account's primary calendar.
pub const EVENTS_URL: &str = "https://www.googleapis.com/calendar/v3/calendars/primary/events";

/// Only the fields [`types::Event`] reads, so a busy calendar stays small.
pub const FIELDS: &str = "nextPageToken,items(id,status,summary,start,end,recurringEventId,iCalUID,attendees(email,displayName,self,resource,responseStatus))";

/// Events per page: Google's default is 250, its maximum 2500.
pub const PAGE_SIZE: u32 = 250;

/// A cap on pages per read (10,000 events), so a server that keeps sending
/// `nextPageToken` cannot keep a read going forever.
const MAX_PAGES: usize = 40;

/// Where the Google provider gets its access token.
pub trait TokenSource: Send + Sync {
    /// A token that should work now. [`Error::SignInExpired`] when there is
    /// no sign-in or it was rejected.
    fn access_token(&self) -> Result<String, Error>;

    /// Google answered 401 with `rejected`: a fresh token, refreshed rather
    /// than cached.
    fn renew_access_token(&self, rejected: &str) -> Result<String, Error>;
}

impl TokenSource for CalendarAuth {
    fn access_token(&self) -> Result<String, Error> {
        CalendarAuth::access_token(self, ProviderId::Google)
    }

    fn renew_access_token(&self, rejected: &str) -> Result<String, Error> {
        CalendarAuth::renew_access_token(self, ProviderId::Google, rejected)
    }
}

/// The signed-in Google account's primary calendar.
pub struct GoogleProvider {
    tokens: Box<dyn TokenSource>,
    http: Box<dyn HttpClient>,
}

impl GoogleProvider {
    pub fn new(tokens: Box<dyn TokenSource>, http: Box<dyn HttpClient>) -> Self {
        Self { tokens, http }
    }

    /// One GET with the bearer token. Transport failures are
    /// [`Error::Unreachable`]; every HTTP status comes back as is.
    fn get(&self, url: &Url, token: &str) -> Result<HttpResponse, Error> {
        let request: HttpRequest = Request::builder()
            .method(Method::GET)
            .uri(url.as_str())
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::ACCEPT, "application/json")
            .body(Vec::new())
            // The detail never includes the request: it holds the token.
            .map_err(|_| unreachable("could not build the Google Calendar request"))?;
        self.http
            .execute(request)
            .map_err(|error| unreachable(error.to_string()))
    }
}

impl CalendarProvider for GoogleProvider {
    fn name(&self) -> &'static str {
        provider_name()
    }

    fn list_events(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<Event>, Error> {
        let mut token = self.tokens.access_token()?;
        let mut renewed = false;
        let mut page_token: Option<String> = None;
        let mut found = Vec::new();
        for _ in 0..MAX_PAGES {
            let url = events_url(from, to, page_token.as_deref())?;
            let mut response = self.get(&url, &token)?;
            if response.status() == StatusCode::UNAUTHORIZED && !renewed {
                // Revoked, or expired early: refresh once and try again.
                renewed = true;
                token = self.tokens.renew_access_token(&token)?;
                response = self.get(&url, &token)?;
            }
            let page = read_page(&response)?;
            found.extend(page.items);
            match page.next_page_token.filter(|next| !next.is_empty()) {
                Some(next) => page_token = Some(next),
                None => return Ok(to_events(found.into_iter().filter_map(raw_event), from, to)),
            }
        }
        Err(unreachable(format!(
            "Google sent more than {MAX_PAGES} pages of events"
        )))
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
        return Err(Error::SignInExpired {
            provider: provider_name(),
        });
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
        join_url: None,
    };
    Some(raw)
}

fn unreachable(detail: impl Into<String>) -> Error {
    Error::Unreachable {
        provider: provider_name(),
        detail: detail.into(),
    }
}
