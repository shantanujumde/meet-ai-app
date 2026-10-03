//! Tier 2 (SPEC §2.7, A12): a Microsoft account's calendar, personal or
//! work, through Microsoft Graph, on every OS (TUR-47). The main calendar for
//! Windows users.
//!
//! One read is `GET /me/calendarView` for the range, page by page through
//! `@odata.nextLink`. Graph expands repeating meetings itself, so every
//! occurrence comes back as its own event with its own id: no RRULE work here.
//! The scope is `Calendars.Read` (delegated, read-only), asked for at sign-in
//! (`oauth::ProviderId::Microsoft`).
//!
//! - Times: the request says `Prefer: outlook.timezone="UTC"`, so Graph
//!   answers in UTC. A time that still names a zone, often a Windows name
//!   like "Pacific Standard Time", goes through [`crate::windows_tz`] and
//!   chrono-tz.
//! - Skipped, like EventKit: cancelled, declined (the event's own
//!   `responseStatus`, which is the signed-in user's) and all-day events.
//!   Rooms and other resources are not counted as attendees.
//! - Tokens: [`TokenSource`], which the app answers from
//!   [`CalendarAuth`](crate::oauth::CalendarAuth). A 401 renews the token
//!   once; a second 401 is [`Error::SignInExpired`].
//! - Network: [`HttpClient`], the same one the sign-in uses, so this crate
//!   has no HTTP stack and the tests answer for Graph from fixture JSON.

use std::collections::HashMap;

use chrono::{DateTime, LocalResult, NaiveDateTime, Offset as _, SecondsFormat, TimeZone, Utc};
use oauth2::http::{Method, Request, StatusCode, header};
use oauth2::url::Url;
use oauth2::{HttpRequest, HttpResponse};

use crate::oauth::{CalendarAuth, HttpClient, ProviderId};
use crate::raw::{RawAttendee, RawEvent, to_events};
use crate::windows_tz::windows_tz_to_iana;
use crate::{CalendarProvider, Error, Event};

pub mod types;

use types::{AttendeeType, DateTimeTimeZone, ResponseType};

/// The name in errors and on the settings screen.
const PROVIDER: &str = "Microsoft";

/// The first page of a read. The range and `$select` go in the query.
pub const CALENDAR_VIEW_URL: &str = "https://graph.microsoft.com/v1.0/me/calendarView";

/// The only host a bearer token is ever sent to, `@odata.nextLink` included.
const GRAPH_HOST: &str = "graph.microsoft.com";

/// Every field [`types::Event`] reads, and nothing else.
pub const SELECT: &str =
    "id,subject,start,end,isAllDay,isCancelled,responseStatus,attendees,iCalUId,onlineMeeting";

/// Events per page.
pub const PAGE_SIZE: &str = "50";

/// Asks Graph for every time in UTC instead of each event's own zone.
pub const PREFER_UTC: &str = "outlook.timezone=\"UTC\"";

/// A cap on pages per read (2,000 events), so a server that keeps sending
/// `@odata.nextLink` cannot keep a read going forever.
const MAX_PAGES: usize = 40;

/// Where the Microsoft provider gets its access token.
pub trait TokenSource: Send + Sync {
    /// A token that should work now. [`Error::SignInExpired`] when there is
    /// no sign-in or it was rejected.
    fn access_token(&self) -> Result<String, Error>;

    /// Graph answered 401 with `rejected`: a fresh token, refreshed rather
    /// than cached.
    fn renew_access_token(&self, rejected: &str) -> Result<String, Error>;
}

impl TokenSource for CalendarAuth {
    fn access_token(&self) -> Result<String, Error> {
        CalendarAuth::access_token(self, ProviderId::Microsoft)
    }

    fn renew_access_token(&self, rejected: &str) -> Result<String, Error> {
        self.reject_access_token(ProviderId::Microsoft, rejected);
        CalendarAuth::access_token(self, ProviderId::Microsoft)
    }
}

impl<T: TokenSource + ?Sized> TokenSource for std::sync::Arc<T> {
    fn access_token(&self) -> Result<String, Error> {
        (**self).access_token()
    }

    fn renew_access_token(&self, rejected: &str) -> Result<String, Error> {
        (**self).renew_access_token(rejected)
    }
}

/// The signed-in Microsoft account's calendar.
pub struct MicrosoftProvider {
    tokens: Box<dyn TokenSource>,
    http: Box<dyn HttpClient>,
}

impl MicrosoftProvider {
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
            .header("Prefer", PREFER_UTC)
            .body(Vec::new())
            // The detail never includes the request: it holds the token.
            .map_err(|_| unreachable("could not build the Graph request"))?;
        self.http
            .execute(request)
            .map_err(|error| unreachable(error.to_string()))
    }
}

impl CalendarProvider for MicrosoftProvider {
    fn name(&self) -> &'static str {
        PROVIDER
    }

    fn list_events(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<Event>, Error> {
        let mut url = calendar_view_url(from, to)?;
        let mut token = self.tokens.access_token()?;
        let mut renewed = false;
        let mut found = Vec::new();
        for _ in 0..MAX_PAGES {
            let mut response = self.get(&url, &token)?;
            if response.status() == StatusCode::UNAUTHORIZED && !renewed {
                // Revoked, or expired early: refresh once and try again.
                renewed = true;
                token = self.tokens.renew_access_token(&token)?;
                response = self.get(&url, &token)?;
            }
            let page = read_page(&response)?;
            found.extend(page.value);
            match page.odata_next_link {
                Some(next) => url = next_page_url(&next)?,
                None => return Ok(convert(found, from, to)),
            }
        }
        Err(unreachable(format!(
            "Graph sent more than {MAX_PAGES} pages of events"
        )))
    }
}

/// `GET /me/calendarView` for `[from, to)`, first page.
pub fn calendar_view_url(from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Url, Error> {
    let mut url = Url::parse(CALENDAR_VIEW_URL).map_err(|error| unreachable(error.to_string()))?;
    url.query_pairs_mut()
        .append_pair(
            "startDateTime",
            &from.to_rfc3339_opts(SecondsFormat::Secs, true),
        )
        .append_pair(
            "endDateTime",
            &to.to_rfc3339_opts(SecondsFormat::Secs, true),
        )
        .append_pair("$select", SELECT)
        .append_pair("$top", PAGE_SIZE);
    Ok(url)
}

/// `@odata.nextLink`, only if it is Graph over https: the token goes with it.
fn next_page_url(next: &str) -> Result<Url, Error> {
    match Url::parse(next) {
        Ok(url) if url.scheme() == "https" && url.host_str() == Some(GRAPH_HOST) => Ok(url),
        _ => Err(unreachable(
            "Graph sent a next page that is not on graph.microsoft.com",
        )),
    }
}

/// One page, or the error its status means.
fn read_page(response: &HttpResponse) -> Result<types::ListEventsResponse, Error> {
    let status = response.status();
    if status == StatusCode::UNAUTHORIZED {
        tracing::warn!("Microsoft Graph rejected a fresh access token; sign in again");
        return Err(Error::SignInExpired { provider: PROVIDER });
    }
    if !status.is_success() {
        // Graph's error code ("ErrorAccessDenied", "TooManyRequests"), never
        // the message: it can name the account.
        let code = serde_json::from_slice::<types::ErrorResponse>(response.body())
            .ok()
            .and_then(|body| body.error.code)
            .map(|code| format!(" ({code})"))
            .unwrap_or_default();
        return Err(unreachable(format!(
            "Graph answered {}{code}",
            status.as_u16()
        )));
    }
    serde_json::from_slice(response.body())
        .map_err(|error| unreachable(format!("unreadable reply from Graph: {error}")))
}

/// Graph events to [`Event`]s: the skip rules, then [`to_events`] for the
/// range, all-day and sorting, then each event's iCalUID back on it.
fn convert(found: Vec<types::Event>, from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<Event> {
    let mut ical_uids = HashMap::new();
    let raw: Vec<RawEvent> = found
        .into_iter()
        .filter_map(raw_event)
        .map(|(raw, ical_uid)| {
            if let Some(uid) = ical_uid {
                ical_uids.insert(raw.id.clone(), uid);
            }
            raw
        })
        .collect();
    let mut events = to_events(raw, from, to);
    for event in &mut events {
        event.ical_uid = ical_uids.remove(&event.id);
    }
    events
}

/// One Graph event as plain data, and its iCalUID. `None` for an event
/// meet-ai never shows (cancelled, declined, all-day) or one whose times
/// cannot be read.
fn raw_event(event: types::Event) -> Option<(RawEvent, Option<String>)> {
    let declined = event
        .response_status
        .as_ref()
        .is_some_and(|status| status.response == Some(ResponseType::Declined));
    let all_day = event.is_all_day.unwrap_or(false);
    if event.is_cancelled.unwrap_or(false) || declined || all_day {
        return None;
    }
    let (Some(start), Some(end)) = (
        event.start.as_ref().and_then(graph_time),
        event.end.as_ref().and_then(graph_time),
    ) else {
        tracing::debug!("skipping a Microsoft event whose times cannot be read");
        return None;
    };
    let attendees = event
        .attendees
        .unwrap_or_default()
        .into_iter()
        .map(|attendee| {
            let (name, email) = attendee
                .email_address
                .map(|address| (address.name, address.address))
                .unwrap_or_default();
            RawAttendee {
                name,
                email,
                // Graph does not mark the user among the attendees; the
                // event's own `responseStatus` (checked above) is theirs.
                is_me: false,
                declined: attendee
                    .status
                    .is_some_and(|status| status.response == Some(ResponseType::Declined)),
                resource: attendee.type_ == Some(AttendeeType::Resource),
            }
        })
        .collect();
    let raw = RawEvent {
        id: event.id,
        title: event.subject.unwrap_or_default(),
        start,
        end,
        all_day,
        attendees,
    };
    Some((raw, event.ical_uid.filter(|uid| !uid.trim().is_empty())))
}

/// A Graph `dateTimeTimeZone` as a UTC instant.
///
/// `dateTime` has no offset (`2026-10-05T09:00:00.0000000`); `timeZone` says
/// how to read it: UTC (what [`PREFER_UTC`] asks for), a Windows name, or an
/// IANA name. `None` for an unknown zone or an unreadable time.
pub fn graph_time(value: &DateTimeTimeZone) -> Option<DateTime<Utc>> {
    let naive =
        NaiveDateTime::parse_from_str(value.date_time.trim(), "%Y-%m-%dT%H:%M:%S%.f").ok()?;
    let zone = value.time_zone.as_deref().map(str::trim).unwrap_or("");
    // Graph's own spelling of UTC in some replies.
    let zone = zone.strip_prefix("tzone://Microsoft/").unwrap_or(zone);
    if zone.is_empty() || zone.eq_ignore_ascii_case("UTC") {
        return Some(Utc.from_utc_datetime(&naive));
    }
    let iana = windows_tz_to_iana(zone).unwrap_or(zone);
    let Ok(tz) = iana.parse::<chrono_tz::Tz>() else {
        tracing::debug!(zone, "unknown time zone on a Microsoft event");
        return None;
    };
    match tz.from_local_datetime(&naive) {
        LocalResult::Single(at) | LocalResult::Ambiguous(at, _) => Some(at.with_timezone(&Utc)),
        // A wall time inside a DST jump: read it at the offset just after.
        LocalResult::None => {
            let offset = tz.offset_from_utc_datetime(&naive).fix();
            Some(Utc.from_utc_datetime(
                &(naive - chrono::Duration::seconds(i64::from(offset.local_minus_utc()))),
            ))
        }
    }
}

fn unreachable(detail: impl Into<String>) -> Error {
    Error::Unreachable {
        provider: PROVIDER,
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn time(date_time: &str, zone: Option<&str>) -> Option<DateTime<Utc>> {
        graph_time(&DateTimeTimeZone {
            date_time: date_time.into(),
            time_zone: zone.map(Into::into),
        })
    }

    #[test]
    fn utc_times_with_seven_fraction_digits() {
        assert_eq!(
            time("2026-10-05T09:00:00.0000000", Some("UTC")),
            Some(at("2026-10-05T09:00:00Z"))
        );
        assert_eq!(
            time("2026-10-05T09:00:00", None),
            Some(at("2026-10-05T09:00:00Z"))
        );
        assert_eq!(
            time("2026-10-05T09:00:00.0000000", Some("tzone://Microsoft/Utc")),
            Some(at("2026-10-05T09:00:00Z"))
        );
    }

    #[test]
    fn a_windows_zone_name_is_read_at_its_offset() {
        // 2026-10-05 is daylight time in Los Angeles: UTC-7.
        assert_eq!(
            time("2026-10-05T09:00:00.0000000", Some("Pacific Standard Time")),
            Some(at("2026-10-05T16:00:00Z"))
        );
        // And standard time in December: UTC-8.
        assert_eq!(
            time("2026-12-07T09:00:00.0000000", Some("Pacific Standard Time")),
            Some(at("2026-12-07T17:00:00Z"))
        );
        assert_eq!(
            time("2026-10-05T09:00:00.0000000", Some("India Standard Time")),
            Some(at("2026-10-05T03:30:00Z"))
        );
    }

    #[test]
    fn an_iana_zone_name_works_too_and_an_unknown_one_is_none() {
        assert_eq!(
            time("2026-10-05T09:00:00", Some("Europe/Berlin")),
            Some(at("2026-10-05T07:00:00Z"))
        );
        assert_eq!(
            time("2026-10-05T09:00:00", Some("Mars Standard Time")),
            None
        );
        assert_eq!(time("not a time", Some("UTC")), None);
    }

    #[test]
    fn a_wall_time_in_a_dst_gap_is_not_dropped() {
        // 02:30 does not exist in Los Angeles on 2026-03-08.
        let read = time("2026-03-08T02:30:00", Some("Pacific Standard Time"));
        assert!(read.is_some());
    }

    #[test]
    fn the_first_page_asks_for_the_range_fields_and_page_size() {
        let url =
            calendar_view_url(at("2026-10-05T00:00:00Z"), at("2026-10-06T00:00:00Z")).unwrap();
        let pairs: HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(url.host_str(), Some(GRAPH_HOST));
        assert_eq!(url.path(), "/v1.0/me/calendarView");
        assert_eq!(pairs["startDateTime"], "2026-10-05T00:00:00Z");
        assert_eq!(pairs["endDateTime"], "2026-10-06T00:00:00Z");
        assert_eq!(pairs["$select"], SELECT);
        assert_eq!(pairs["$top"], "50");
    }

    #[test]
    fn a_next_link_off_graph_is_refused() {
        assert!(next_page_url("https://graph.microsoft.com/v1.0/me/calendarView?$skip=50").is_ok());
        for bad in [
            "http://graph.microsoft.com/v1.0/me/calendarView",
            "https://evil.example.com/v1.0/me/calendarView",
            "https://graph.microsoft.com.evil.example.com/x",
            "not a url",
        ] {
            assert!(
                matches!(next_page_url(bad), Err(Error::Unreachable { .. })),
                "{bad}"
            );
        }
    }
}
