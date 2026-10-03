//! `cargo test -p calendar` for the Microsoft Graph provider (TUR-47), with
//! no network: a fake [`HttpClient`] answers from the fixture JSON in
//! `fixtures/microsoft/`. The fixture day is 2026-10-05 UTC.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use calendar::microsoft::{MicrosoftProvider, PREFER_UTC, SELECT, TokenSource};
use calendar::oauth::{
    CalendarAuth, HttpClient, HttpError, MemoryStore, OAuthClient, ProviderId, TokenStore as _,
};
use calendar::{CalendarProvider, Error};
use chrono::{DateTime, TimeZone, Utc};
use oauth2::url::Url;
use oauth2::{HttpRequest, HttpResponse};

const PAGE1: &str = include_str!("fixtures/microsoft/page1.json");
const PAGE2: &str = include_str!("fixtures/microsoft/page2.json");
const ERROR_401: &str = include_str!("fixtures/microsoft/error-401.json");
const ERROR_429: &str = include_str!("fixtures/microsoft/error-429.json");
const JOIN_LINKS: &str = include_str!("fixtures/microsoft/join-links.json");
const ORGANIZER: &str = include_str!("fixtures/microsoft/organizer.json");

const TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";

/// One request as the fake saw it.
#[derive(Debug, Clone)]
struct Seen {
    url: String,
    authorization: Option<String>,
    prefer: Option<String>,
}

/// Answers each request with the next queued reply, and records it.
#[derive(Default)]
struct FakeHttp {
    replies: Mutex<VecDeque<Result<(u16, String), String>>>,
    seen: Mutex<Vec<Seen>>,
}

impl FakeHttp {
    fn reply(&self, status: u16, body: &str) -> &Self {
        self.replies
            .lock()
            .unwrap()
            .push_back(Ok((status, body.to_owned())));
        self
    }

    fn fail(&self, detail: &str) -> &Self {
        self.replies.lock().unwrap().push_back(Err(detail.into()));
        self
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    fn graph_requests(&self) -> Vec<Seen> {
        self.seen()
            .into_iter()
            .filter(|seen| seen.url.starts_with("https://graph.microsoft.com/"))
            .collect()
    }
}

struct Shared(Arc<FakeHttp>);

impl HttpClient for Shared {
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
        let header = |name: &str| {
            request
                .headers()
                .get(name)
                .map(|value| value.to_str().unwrap().to_owned())
        };
        self.0.seen.lock().unwrap().push(Seen {
            url: request.uri().to_string(),
            authorization: header("authorization"),
            prefer: header("prefer"),
        });
        let reply = self
            .0
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| panic!("an unexpected request to {}", request.uri()));
        let (status, body) = reply.map_err(HttpError)?;
        Ok(oauth2::http::Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(body.into_bytes())
            .unwrap())
    }
}

/// Hands out `token-1`, then `token-2`, … on each renewal.
#[derive(Default)]
struct FakeTokens {
    renewals: Mutex<Vec<String>>,
    expired: bool,
}

impl TokenSource for FakeTokens {
    fn access_token(&self) -> Result<String, Error> {
        if self.expired {
            return Err(Error::SignInExpired {
                provider: "Microsoft",
            });
        }
        Ok(format!("token-{}", self.renewals.lock().unwrap().len() + 1))
    }

    fn renew_access_token(&self, rejected: &str) -> Result<String, Error> {
        self.renewals.lock().unwrap().push(rejected.to_owned());
        self.access_token()
    }
}

fn day(d: u32) -> (DateTime<Utc>, DateTime<Utc>) {
    let from = Utc.with_ymd_and_hms(2026, 10, d, 0, 0, 0).single().unwrap();
    (from, from + chrono::Duration::days(1))
}

fn at(rfc3339: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(rfc3339)
        .unwrap()
        .with_timezone(&Utc)
}

fn provider(http: &Arc<FakeHttp>, tokens: &Arc<FakeTokens>) -> MicrosoftProvider {
    MicrosoftProvider::new(Box::new(tokens.clone()), Box::new(Shared(http.clone())))
}

fn both_pages() -> Arc<FakeHttp> {
    let http = Arc::new(FakeHttp::default());
    http.reply(200, PAGE1).reply(200, PAGE2);
    http
}

#[test]
fn todays_meetings_skip_cancelled_declined_and_all_day() {
    let http = both_pages();
    let (from, to) = day(5);
    let events = provider(&http, &Arc::default())
        .list_events(from, to)
        .unwrap();
    let titles: Vec<&str> = events.iter().map(|e| e.title.as_str()).collect();
    assert_eq!(
        titles,
        vec![
            "Platform Standup",
            "Focus time",
            "Design review (Seattle)",
            // No subject: kept, with an empty title, like EventKit.
            "",
        ]
    );
}

#[test]
fn fields_are_mapped_and_rooms_are_not_counted() {
    let http = both_pages();
    let (from, to) = day(5);
    let events = provider(&http, &Arc::default())
        .list_events(from, to)
        .unwrap();
    let standup = &events[0];
    assert_eq!(standup.id, "AAMkAGStandup-occurrence-20261005");
    assert_eq!(standup.start, at("2026-10-05T09:00:00Z"));
    assert_eq!(standup.end, at("2026-10-05T09:15:00Z"));
    assert_eq!(standup.attendees, 2);
    assert_eq!(
        standup.attendee_names,
        vec!["Priya Raman", "dev@example.com"]
    );
    assert_eq!(
        standup.ical_uid.as_deref(),
        Some("040000008200E00074C5B7101A82E00807EA0A05STANDUP")
    );
    // TUR-77: `onlineMeeting.joinUrl` is the meeting link.
    assert_eq!(
        standup.join_url.as_deref(),
        Some("https://teams.microsoft.com/l/meetup-join/standup")
    );
    let focus = &events[1];
    assert_eq!(focus.attendees, 0);
    assert_eq!(focus.ical_uid, None);
    assert_eq!(focus.join_url, None);
}

#[test]
fn a_link_in_the_body_or_location_is_the_join_link() {
    // TUR-86: with no (safe) online meeting, a Zoom link in the body or a
    // Meet link in the location gets a Join; look-alikes never do.
    let http = Arc::new(FakeHttp::default());
    http.reply(200, JOIN_LINKS);
    let (from, to) = day(5);
    let events = provider(&http, &Arc::default())
        .list_events(from, to)
        .unwrap();
    let links: Vec<(&str, Option<&str>)> = events
        .iter()
        .map(|e| (e.id.as_str(), e.join_url.as_deref()))
        .collect();
    assert_eq!(
        links,
        vec![
            (
                "zoom-in-body",
                Some("https://acme.zoom.us/j/5551234567?pwd=abc")
            ),
            (
                "meet-in-location",
                Some("https://meet.google.com/abc-defg-hij")
            ),
            ("phishing-everywhere", None),
            (
                "bad-online-meeting-real-body",
                Some("https://teams.microsoft.com/l/meetup-join/real")
            ),
        ]
    );
}

#[test]
fn the_read_selects_location_and_body_preview() {
    assert!(SELECT.ends_with(",onlineMeeting,location,bodyPreview"));
}

#[test]
fn the_organizer_counts_once_so_a_one_on_one_is_not_solo() {
    // TUR-88: Graph sends the organizer apart from `attendees`. Both kinds
    // of Teams 1:1 are two people; a focus block of my own is still one.
    assert!(SELECT.split(',').any(|field| field == "organizer"));
    let http = Arc::new(FakeHttp::default());
    http.reply(200, ORGANIZER);
    let (from, to) = day(5);
    let events = provider(&http, &Arc::default())
        .list_events(from, to)
        .unwrap();
    let counts: Vec<(&str, usize)> = events
        .iter()
        .map(|e| (e.id.as_str(), e.attendees))
        .collect();
    assert_eq!(
        counts,
        vec![
            ("invited-one-on-one", 2),
            ("organized-one-on-one", 2),
            ("organizer-already-listed", 2),
            ("my-focus-block", 1),
        ]
    );
    assert_eq!(events[0].attendee_names, vec!["Ada Lovelace", "Bob Okafor"]);
}

#[test]
fn a_windows_time_zone_name_is_converted() {
    let http = both_pages();
    let (from, to) = day(5);
    let events = provider(&http, &Arc::default())
        .list_events(from, to)
        .unwrap();
    let review = events
        .iter()
        .find(|e| e.id == "AAMkAGPacificDesignReview")
        .unwrap();
    // 08:00 Pacific daylight time is 15:00 UTC.
    assert_eq!(review.start, at("2026-10-05T15:00:00Z"));
    assert_eq!(review.end, at("2026-10-05T16:00:00Z"));
    // Someone else declining does not drop the meeting or the count.
    assert_eq!(review.attendees, 2);
    assert_eq!(review.attendee_names, vec!["Dev", "sam@example.com"]);
}

#[test]
fn each_occurrence_of_a_recurring_meeting_is_its_own_event() {
    let http = both_pages();
    let (from, _) = day(5);
    let (_, to) = day(6);
    let events = provider(&http, &Arc::default())
        .list_events(from, to)
        .unwrap();
    let standups: Vec<(&str, DateTime<Utc>)> = events
        .iter()
        .filter(|e| e.title == "Platform Standup")
        .map(|e| (e.id.as_str(), e.start))
        .collect();
    assert_eq!(
        standups,
        vec![
            (
                "AAMkAGStandup-occurrence-20261005",
                at("2026-10-05T09:00:00Z")
            ),
            (
                "AAMkAGStandup-occurrence-20261006",
                at("2026-10-06T09:00:00Z")
            ),
        ]
    );
}

#[test]
fn the_request_asks_for_utc_with_the_token_and_follows_next_link() {
    let http = both_pages();
    let (from, to) = day(5);
    provider(&http, &Arc::default())
        .list_events(from, to)
        .unwrap();
    let seen = http.graph_requests();
    assert_eq!(seen.len(), 2);
    for request in &seen {
        assert_eq!(request.authorization.as_deref(), Some("Bearer token-1"));
        assert_eq!(request.prefer.as_deref(), Some(PREFER_UTC));
    }
    let first = Url::parse(&seen[0].url).unwrap();
    assert_eq!(first.path(), "/v1.0/me/calendarView");
    let pairs: Vec<(String, String)> = first.query_pairs().into_owned().collect();
    assert!(pairs.contains(&("startDateTime".into(), "2026-10-05T00:00:00Z".into())));
    assert!(pairs.contains(&("endDateTime".into(), "2026-10-06T00:00:00Z".into())));
    assert!(pairs.contains(&("$select".into(), SELECT.into())));
    assert!(pairs.contains(&("$top".into(), "50".into())));
    assert!(seen[1].url.contains("%24skip=50"), "{}", seen[1].url);
}

#[test]
fn a_401_renews_the_token_once_and_retries() {
    let http = Arc::new(FakeHttp::default());
    http.reply(401, ERROR_401)
        .reply(200, PAGE1)
        .reply(200, PAGE2);
    let tokens = Arc::new(FakeTokens::default());
    let (from, to) = day(5);
    let events = provider(&http, &tokens).list_events(from, to).unwrap();
    assert_eq!(events.len(), 4);
    assert_eq!(*tokens.renewals.lock().unwrap(), vec!["token-1"]);
    let auth: Vec<_> = http
        .graph_requests()
        .into_iter()
        .map(|seen| seen.authorization.unwrap())
        .collect();
    assert_eq!(
        auth,
        vec!["Bearer token-1", "Bearer token-2", "Bearer token-2"]
    );
}

#[test]
fn a_second_401_is_sign_in_expired() {
    let http = Arc::new(FakeHttp::default());
    http.reply(401, ERROR_401).reply(401, ERROR_401);
    let (from, to) = day(5);
    let error = provider(&http, &Arc::default())
        .list_events(from, to)
        .unwrap_err();
    assert!(
        matches!(
            error,
            Error::SignInExpired {
                provider: "Microsoft"
            }
        ),
        "{error:?}"
    );
}

#[test]
fn no_sign_in_is_sign_in_expired_without_a_request() {
    let http = Arc::new(FakeHttp::default());
    let tokens = Arc::new(FakeTokens {
        expired: true,
        ..FakeTokens::default()
    });
    let (from, to) = day(5);
    let error = provider(&http, &tokens).list_events(from, to).unwrap_err();
    assert!(matches!(error, Error::SignInExpired { .. }), "{error:?}");
    assert!(http.seen().is_empty());
}

#[test]
fn other_failures_are_unreachable_never_an_empty_day() {
    let (from, to) = day(5);

    let http = Arc::new(FakeHttp::default());
    http.reply(429, ERROR_429);
    match provider(&http, &Arc::default()).list_events(from, to) {
        Err(Error::Unreachable { provider, detail }) => {
            assert_eq!(provider, "Microsoft");
            assert_eq!(detail, "Graph answered 429 (TooManyRequests)");
        }
        other => panic!("{other:?}"),
    }

    let http = Arc::new(FakeHttp::default());
    http.fail("connection refused");
    assert!(matches!(
        provider(&http, &Arc::default()).list_events(from, to),
        Err(Error::Unreachable { .. })
    ));

    let http = Arc::new(FakeHttp::default());
    http.reply(200, "<html>captive portal</html>");
    assert!(matches!(
        provider(&http, &Arc::default()).list_events(from, to),
        Err(Error::Unreachable { .. })
    ));

    // A failing second page fails the read: half a day is not the day.
    let http = Arc::new(FakeHttp::default());
    http.reply(200, PAGE1).reply(503, "{}");
    assert!(matches!(
        provider(&http, &Arc::default()).list_events(from, to),
        Err(Error::Unreachable { .. })
    ));
}

#[test]
fn a_next_link_off_graph_never_gets_the_token() {
    let page = PAGE1.replace(
        "https://graph.microsoft.com/v1.0/me/calendarView?",
        "https://attacker.example.com/v1.0/me/calendarView?",
    );
    let http = Arc::new(FakeHttp::default());
    http.reply(200, &page);
    let (from, to) = day(5);
    assert!(matches!(
        provider(&http, &Arc::default()).list_events(from, to),
        Err(Error::Unreachable { .. })
    ));
    assert_eq!(http.seen().len(), 1);
}

/// The real [`CalendarAuth`] behind the provider: a 401 makes it refresh
/// (instead of handing back its cached token), and a rejected refresh is
/// `SignInExpired`.
mod with_calendar_auth {
    use super::*;

    fn auth(http: &Arc<FakeHttp>) -> Arc<CalendarAuth> {
        let store = MemoryStore::default();
        store.save(ProviderId::Microsoft, "refresh-1").unwrap();
        Arc::new(CalendarAuth::new(
            Box::new(|_| {
                Some(OAuthClient {
                    client_id: "ms-client".into(),
                    client_secret: None,
                })
            }),
            Box::new(store),
            Box::new(Shared(http.clone())),
        ))
    }

    fn token_reply(access: &str) -> String {
        serde_json::json!({
            "access_token": access,
            "token_type": "Bearer",
            "expires_in": 3600,
            "refresh_token": "refresh-2",
        })
        .to_string()
    }

    #[test]
    fn a_401_refreshes_through_calendar_auth() {
        let http = Arc::new(FakeHttp::default());
        http.reply(200, &token_reply("access-1"))
            .reply(401, ERROR_401)
            .reply(200, &token_reply("access-2"))
            .reply(200, PAGE1)
            .reply(200, PAGE2);
        let auth = auth(&http);
        let provider =
            MicrosoftProvider::new(Box::new(auth.clone()), Box::new(Shared(http.clone())));
        let (from, to) = day(5);
        assert_eq!(provider.list_events(from, to).unwrap().len(), 4);
        let urls: Vec<String> = http.seen().into_iter().map(|seen| seen.url).collect();
        assert_eq!(urls[0], TOKEN_URL);
        assert_eq!(urls[2], TOKEN_URL);
        let auth_headers: Vec<String> = http
            .graph_requests()
            .into_iter()
            .map(|seen| seen.authorization.unwrap())
            .collect();
        assert_eq!(
            auth_headers,
            vec!["Bearer access-1", "Bearer access-2", "Bearer access-2"]
        );
    }

    #[test]
    fn a_rejected_refresh_after_a_401_is_sign_in_expired() {
        let http = Arc::new(FakeHttp::default());
        let invalid_grant = serde_json::json!({ "error": "invalid_grant" }).to_string();
        http.reply(200, &token_reply("access-1"))
            .reply(401, ERROR_401)
            .reply(400, &invalid_grant);
        let auth = auth(&http);
        let provider =
            MicrosoftProvider::new(Box::new(auth.clone()), Box::new(Shared(http.clone())));
        let (from, to) = day(5);
        let error = provider.list_events(from, to).unwrap_err();
        assert!(
            matches!(
                error,
                Error::SignInExpired {
                    provider: "Microsoft"
                }
            ),
            "{error:?}"
        );
        assert!(auth.has_sign_in(ProviderId::Microsoft));
    }

    #[test]
    fn has_sign_in_needs_a_stored_token() {
        let http = Arc::new(FakeHttp::default());
        assert!(auth(&http).has_sign_in(ProviderId::Microsoft));
        assert!(!auth(&http).has_sign_in(ProviderId::Google));
        assert!(http.seen().is_empty(), "no network");
    }
}
