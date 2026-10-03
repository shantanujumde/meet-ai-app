//! `cargo test -p calendar` for the Google provider (TUR-48): fixture JSON
//! from `tests/fixtures/google/`, a fake HTTP layer that answers for both
//! Google's token endpoint and the Calendar API, and the real
//! `CalendarAuth` over an in-memory token store. No network, no keystore.
//! The fixture day is 2026-10-05 UTC.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use calendar::google::{FIELDS, GoogleProvider, PAGE_SIZE, TokenSource, events_url};
use calendar::oauth::{
    CalendarAuth, HttpClient, HttpError, MemoryStore, OAuthClient, ProviderId, TokenStore as _,
};
use calendar::{CalendarProvider, Error};
use chrono::{DateTime, TimeZone, Utc};
use oauth2::url::Url;
use oauth2::{HttpRequest, HttpResponse};

const DAY_PAGE_1: &str = include_str!("fixtures/google/day-page1.json");
const DAY_PAGE_2: &str = include_str!("fixtures/google/day-page2.json");
const RECURRING: &str = include_str!("fixtures/google/recurring.json");
const ERROR_401: &str = include_str!("fixtures/google/error-401.json");
const ERROR_403: &str = include_str!("fixtures/google/error-403.json");

const TOKEN_HOST: &str = "oauth2.googleapis.com";

fn at(d: u32, h: u32, m: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, d, h, m, 0).single().unwrap()
}

fn day(d: u32) -> (DateTime<Utc>, DateTime<Utc>) {
    (at(d, 0, 0), at(d, 0, 0) + chrono::Duration::days(1))
}

/// One request the fake saw.
#[derive(Debug, Clone)]
struct Seen {
    url: Url,
    authorization: Option<String>,
}

/// Answers for Google: token requests from one queue, Calendar API requests
/// from another. An empty queue is a transport error.
#[derive(Default)]
struct FakeGoogle {
    tokens: Mutex<VecDeque<(u16, String)>>,
    calendar: Mutex<VecDeque<Result<(u16, String), String>>>,
    token_calls: Mutex<usize>,
    seen: Mutex<Vec<Seen>>,
}

impl FakeGoogle {
    fn token(&self, access: &str) -> &Self {
        let body = serde_json::json!({
            "access_token": access,
            "token_type": "Bearer",
            "expires_in": 3600,
        });
        self.tokens
            .lock()
            .unwrap()
            .push_back((200, body.to_string()));
        self
    }

    fn token_refused(&self) -> &Self {
        let body = serde_json::json!({
            "error": "invalid_grant",
            "error_description": "Token has been expired or revoked.",
        });
        self.tokens
            .lock()
            .unwrap()
            .push_back((400, body.to_string()));
        self
    }

    fn page(&self, status: u16, body: &str) -> &Self {
        self.calendar
            .lock()
            .unwrap()
            .push_back(Ok((status, body.to_owned())));
        self
    }

    fn offline(&self) -> &Self {
        self.calendar
            .lock()
            .unwrap()
            .push_back(Err("connection refused".into()));
        self
    }

    fn token_calls(&self) -> usize {
        *self.token_calls.lock().unwrap()
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

struct Http(Arc<FakeGoogle>);

impl HttpClient for Http {
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
        let url = Url::parse(&request.uri().to_string()).unwrap();
        let answer = if url.host_str() == Some(TOKEN_HOST) {
            *self.0.token_calls.lock().unwrap() += 1;
            self.0
                .tokens
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| "no token answer queued".to_owned())
        } else {
            self.0.seen.lock().unwrap().push(Seen {
                url,
                authorization: request
                    .headers()
                    .get("authorization")
                    .map(|value| value.to_str().unwrap().to_owned()),
            });
            self.0
                .calendar
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err("no calendar answer queued".into()))
        };
        let (status, body) = answer.map_err(HttpError)?;
        Ok(oauth2::http::Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(body.into_bytes())
            .unwrap())
    }
}

/// The real `CalendarAuth`, signed in to Google when `refresh` is a token.
fn auth(fake: &Arc<FakeGoogle>, refresh: Option<&str>) -> CalendarAuth {
    let store = MemoryStore::default();
    if let Some(refresh) = refresh {
        store.save(ProviderId::Google, refresh).unwrap();
    }
    CalendarAuth::new(
        Box::new(|_| {
            Some(OAuthClient {
                client_id: "test-client.apps.googleusercontent.com".into(),
                client_secret: Some("not-secret".into()),
            })
        }),
        Box::new(store),
        Box::new(Http(fake.clone())),
    )
}

fn provider(fake: &Arc<FakeGoogle>) -> GoogleProvider {
    provider_with(fake, Some("refresh-1"))
}

fn provider_with(fake: &Arc<FakeGoogle>, refresh: Option<&str>) -> GoogleProvider {
    GoogleProvider::new(Box::new(auth(fake, refresh)), Box::new(Http(fake.clone())))
}

fn param(url: &Url, name: &str) -> Option<String> {
    url.query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

#[test]
fn todays_meetings_skip_cancelled_declined_and_all_day_across_pages() {
    let fake = Arc::new(FakeGoogle::default());
    fake.token("access-1")
        .page(200, DAY_PAGE_1)
        .page(200, DAY_PAGE_2);
    let (from, to) = day(5);
    let events = provider(&fake).list_events(from, to).unwrap();

    let titles: Vec<&str> = events.iter().map(|e| e.title.as_str()).collect();
    // Gone: "Company holiday" (all-day), "Vendor pitch" (I declined),
    // "Team offsite planning" (cancelled).
    assert_eq!(
        titles,
        vec!["Platform Standup", "Design review", "Focus time"]
    );

    let standup = &events[0];
    assert_eq!(standup.id, "standup_20261005T043000Z");
    assert_eq!(standup.start, at(5, 4, 30));
    assert_eq!(standup.end, at(5, 4, 45));
    // Room 4B is a resource: neither counted nor named.
    assert_eq!(standup.attendees, 3);
    assert_eq!(
        standup.attendee_names,
        vec!["me@example.com", "Priya", "dev@example.com"]
    );
    assert_eq!(standup.ical_uid.as_deref(), Some("standup@google.com"));

    // Someone else declining, or me being tentative, keeps the meeting.
    assert_eq!(events[1].attendees, 2);
    // No attendees and no iCalUID: a block of my own.
    assert_eq!(events[2].attendees, 0);
    assert_eq!(events[2].ical_uid, None);
    assert_eq!(fake.token_calls(), 1);
}

#[test]
fn the_request_reads_the_primary_calendar_expanded_in_utc_and_follows_pages() {
    let fake = Arc::new(FakeGoogle::default());
    fake.token("access-1")
        .page(200, DAY_PAGE_1)
        .page(200, DAY_PAGE_2);
    let (from, to) = day(5);
    provider(&fake).list_events(from, to).unwrap();

    let seen = fake.seen();
    assert_eq!(seen.len(), 2);
    for request in &seen {
        assert_eq!(
            request.url.as_str().split('?').next(),
            Some("https://www.googleapis.com/calendar/v3/calendars/primary/events")
        );
        assert_eq!(request.authorization.as_deref(), Some("Bearer access-1"));
        let url = &request.url;
        assert_eq!(
            param(url, "timeMin").as_deref(),
            Some("2026-10-05T00:00:00Z")
        );
        assert_eq!(
            param(url, "timeMax").as_deref(),
            Some("2026-10-06T00:00:00Z")
        );
        assert_eq!(param(url, "singleEvents").as_deref(), Some("true"));
        assert_eq!(param(url, "orderBy").as_deref(), Some("startTime"));
        assert_eq!(param(url, "timeZone").as_deref(), Some("UTC"));
        assert_eq!(param(url, "maxResults"), Some(PAGE_SIZE.to_string()));
        assert_eq!(param(url, "fields").as_deref(), Some(FIELDS));
    }
    assert_eq!(param(&seen[0].url, "pageToken"), None);
    assert_eq!(param(&seen[1].url, "pageToken").as_deref(), Some("page-2"));
}

#[test]
fn recurring_occurrences_each_have_their_own_id() {
    let fake = Arc::new(FakeGoogle::default());
    fake.token("access-1").page(200, RECURRING);
    let events = provider(&fake)
        .list_events(at(5, 0, 0), at(9, 0, 0))
        .unwrap();

    let ids: Vec<&str> = events.iter().map(|e| e.id.as_str()).collect();
    // The 6th was cancelled, the 8th declined; the 7th was moved and keeps
    // its own id and new time.
    assert_eq!(
        ids,
        vec!["oneonone_20261005T093000Z", "oneonone_20261007T093000Z"]
    );
    assert_eq!(events[1].start, at(7, 11, 0));
    assert!(
        events
            .iter()
            .all(|e| e.ical_uid.as_deref() == Some("oneonone@google.com"))
    );
}

#[test]
fn an_empty_day_is_an_empty_list() {
    let fake = Arc::new(FakeGoogle::default());
    fake.token("access-1").page(200, "{}");
    let (from, to) = day(5);
    assert_eq!(provider(&fake).list_events(from, to).unwrap(), vec![]);
}

#[test]
fn the_access_token_is_reused_between_reads() {
    let fake = Arc::new(FakeGoogle::default());
    fake.token("access-1").page(200, "{}").page(200, "{}");
    let google = provider(&fake);
    let (from, to) = day(5);
    google.list_events(from, to).unwrap();
    google.list_events(from, to).unwrap();
    assert_eq!(fake.token_calls(), 1);
}

#[test]
fn a_401_refreshes_once_and_retries() {
    let fake = Arc::new(FakeGoogle::default());
    fake.token("access-1")
        .token("access-2")
        .page(401, ERROR_401)
        .page(200, DAY_PAGE_2);
    let (from, to) = day(5);
    let events = provider(&fake).list_events(from, to).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(fake.token_calls(), 2);
    let auth: Vec<_> = fake
        .seen()
        .into_iter()
        .map(|s| s.authorization.unwrap())
        .collect();
    assert_eq!(auth, vec!["Bearer access-1", "Bearer access-2"]);
}

#[test]
fn a_second_401_is_sign_in_expired() {
    let fake = Arc::new(FakeGoogle::default());
    fake.token("access-1")
        .token("access-2")
        .page(401, ERROR_401)
        .page(401, ERROR_401);
    let (from, to) = day(5);
    let error = provider(&fake).list_events(from, to).unwrap_err();
    assert!(
        matches!(error, Error::SignInExpired { provider: "Google" }),
        "{error:?}"
    );
}

#[test]
fn a_revoked_refresh_token_is_sign_in_expired() {
    let fake = Arc::new(FakeGoogle::default());
    fake.token("access-1").token_refused().page(401, ERROR_401);
    let (from, to) = day(5);
    let error = provider(&fake).list_events(from, to).unwrap_err();
    assert!(
        matches!(error, Error::SignInExpired { provider: "Google" }),
        "{error:?}"
    );
    assert_eq!(fake.seen().len(), 1, "no retry with a refused refresh");
}

#[test]
fn a_refresh_refused_at_the_first_read_is_sign_in_expired_without_a_calendar_call() {
    let fake = Arc::new(FakeGoogle::default());
    fake.token_refused();
    let (from, to) = day(5);
    let error = provider(&fake).list_events(from, to).unwrap_err();
    assert!(
        matches!(error, Error::SignInExpired { provider: "Google" }),
        "{error:?}"
    );
    assert!(fake.seen().is_empty());
}

#[test]
fn not_signed_in_is_sign_in_expired() {
    let fake = Arc::new(FakeGoogle::default());
    let (from, to) = day(5);
    let error = provider_with(&fake, None)
        .list_events(from, to)
        .unwrap_err();
    assert!(
        matches!(error, Error::SignInExpired { provider: "Google" }),
        "{error:?}"
    );
    assert_eq!(fake.token_calls(), 0);
}

#[test]
fn an_api_error_names_its_status_and_reason_but_not_the_message() {
    let fake = Arc::new(FakeGoogle::default());
    fake.token("access-1").page(403, ERROR_403);
    let (from, to) = day(5);
    let error = provider(&fake).list_events(from, to).unwrap_err();
    let Error::Unreachable { provider, detail } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(*provider, "Google");
    assert!(detail.contains("403"), "{detail}");
    assert!(detail.contains("PERMISSION_DENIED"), "{detail}");
    assert!(detail.contains("accessNotConfigured"), "{detail}");
    assert!(!detail.contains("me@example.com"), "{detail}");
}

#[test]
fn no_network_is_unreachable_never_an_empty_day() {
    let fake = Arc::new(FakeGoogle::default());
    fake.token("access-1").offline();
    let (from, to) = day(5);
    let error = provider(&fake).list_events(from, to).unwrap_err();
    assert!(
        matches!(&error, Error::Unreachable { provider: "Google", detail } if detail.contains("connection refused")),
        "{error:?}"
    );
}

#[test]
fn an_unreadable_page_is_unreachable() {
    let fake = Arc::new(FakeGoogle::default());
    fake.token("access-1")
        .page(200, "<html>captive portal</html>");
    let (from, to) = day(5);
    let error = provider(&fake).list_events(from, to).unwrap_err();
    assert!(
        matches!(
            error,
            Error::Unreachable {
                provider: "Google",
                ..
            }
        ),
        "{error:?}"
    );
}

#[test]
fn the_page_token_is_query_encoded() {
    let (from, to) = day(5);
    let url = events_url(from, to, Some("a&b=c d")).unwrap();
    assert_eq!(param(&url, "pageToken").as_deref(), Some("a&b=c d"));
    assert!(url.as_str().contains("pageToken=a%26b%3Dc+d"), "{url}");
}

/// A token source that is not `CalendarAuth`, to pin the trait's contract:
/// a 401 hands back the rejected token, and a renewal is asked for once.
#[test]
fn renewal_is_asked_for_with_the_rejected_token() {
    struct Tokens(Mutex<Vec<String>>);
    impl TokenSource for Tokens {
        fn access_token(&self) -> Result<String, Error> {
            Ok("old".into())
        }
        fn renew_access_token(&self, rejected: &str) -> Result<String, Error> {
            self.0.lock().unwrap().push(rejected.to_owned());
            Ok("new".into())
        }
    }
    let renewals = Arc::new(Tokens(Mutex::new(Vec::new())));
    struct Shared(Arc<Tokens>);
    impl TokenSource for Shared {
        fn access_token(&self) -> Result<String, Error> {
            self.0.access_token()
        }
        fn renew_access_token(&self, rejected: &str) -> Result<String, Error> {
            self.0.renew_access_token(rejected)
        }
    }
    let fake = Arc::new(FakeGoogle::default());
    fake.page(401, ERROR_401).page(200, "{}");
    let google = GoogleProvider::new(
        Box::new(Shared(renewals.clone())),
        Box::new(Http(fake.clone())),
    );
    let (from, to) = day(5);
    assert_eq!(google.list_events(from, to).unwrap(), vec![]);
    assert_eq!(*renewals.0.lock().unwrap(), vec!["old"]);
    assert_eq!(google.name(), "Google");
}
