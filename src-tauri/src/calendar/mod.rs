//! Today's meetings, straight from the calendar (TUR-28, `docs/problem.md`
//! item 38): no login, no setup.
//!
//! [`CalendarState`] is the app's one way to read calendar events. It asks
//! every provider `config.jsonc` names (EventKit on macOS, Google and
//! Microsoft sign-in on every OS; see `config::calendar`) and merges the
//! answers. The window's Today pane reads
//! it through [`todays_meetings`]; the recorder's event matching (TUR-29)
//! reads it through [`CalendarState::events_between`].
//!
//! Reading can block: EventKit waits up to two minutes for the user to answer
//! the permission prompt (TUR-26). Never call [`CalendarState::events_between`]
//! on the main thread; the command below runs it on the blocking pool.
//!
//! The extern `calendar` crate is spelled `::calendar` here, so it is never
//! confused with this module of the same name.

use std::sync::Arc;

use ::calendar::oauth::ProviderId;
use ::calendar::{CalendarProvider, Error, Event};
use chrono::{DateTime, Duration, Local, NaiveDate, Offset as _, TimeZone, Utc};
use serde::Serialize;
use tauri::{AppHandle, Manager as _};

use self::readable::Prompt;
use crate::config::{self, Provider};
use crate::error::UiError;

// TUR-88: the sign-in's own loopback listener.
mod loopback;
pub mod readable;
pub mod signin;
// TUR-49: the Settings card's sources.
pub mod sources;
// TUR-47/48: Microsoft and Google, behind their sign-in.
pub(crate) mod cloud;
#[cfg(test)]
mod tests;

/// A provider that can be shared with the blocking pool.
type SharedProvider = Arc<dyn CalendarProvider + Send + Sync>;

/// Which providers a read asks, for a read that may or may not prompt.
type Picker = Box<dyn Fn(Prompt) -> Vec<SharedProvider> + Send + Sync>;

/// Managed state: where calendar events come from.
///
/// By default the providers are picked from `config.jsonc` on every read, so
/// editing `calendar.providers` needs no restart.
#[derive(Default)]
pub struct CalendarState {
    /// Picks the providers instead of the configured ones (tests).
    test_picker: Option<Picker>,
}

impl CalendarState {
    /// A state that reads only `providers`, whatever the config says.
    #[cfg(test)]
    pub fn with_providers(providers: Vec<SharedProvider>) -> Self {
        Self::with_picker(move |_| providers.clone())
    }

    /// A state whose providers come from `pick` (tests): say,
    /// [`readable::pick`] over fakes, with Calendar.app's answer made up.
    #[cfg(test)]
    pub fn with_picker(
        pick: impl Fn(Prompt) -> Vec<SharedProvider> + Send + Sync + 'static,
    ) -> Self {
        Self {
            test_picker: Some(Box::new(pick)),
        }
    }

    fn providers(&self, prompt: Prompt) -> Vec<SharedProvider> {
        match &self.test_picker {
            Some(pick) => pick(prompt),
            None => configured_providers(prompt),
        }
    }

    /// Events overlapping `[from, to)` from every provider, sorted by start.
    ///
    /// Blocking (see the module comment). `Ok(vec![])` means the calendars
    /// were read and hold nothing then. A provider that fails is logged and
    /// skipped while another one answers; when none answers, the first
    /// error is returned, so a denied calendar is never an empty day.
    pub fn events_between(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<Event>, Error> {
        merge(&self.providers(Prompt::Allowed), from, to)
    }

    /// [`Self::events_between`] for a reader in the background (the menu
    /// bar, the reminders), which never shows the macOS prompt: every
    /// calendar that can be read now ([`readable`], TUR-88), so a Google or
    /// Microsoft sign-in is read while Calendar.app waits for its answer.
    ///
    /// Nothing left to read (no calendar set up, signed out, or only
    /// Calendar.app, unanswered) is [`Error::PermissionDenied`]: nothing is
    /// connected, which is not an empty day. Blocking, like the other read.
    pub fn events_between_unprompted(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<Event>, Error> {
        let providers = self.providers(Prompt::Never);
        if providers.is_empty() {
            return Err(Error::PermissionDenied);
        }
        merge(&providers, from, to)
    }
}

/// The providers `config.jsonc` names that this build can read, and this
/// read may ask ([`readable::pick`]).
fn configured_providers(prompt: Prompt) -> Vec<SharedProvider> {
    readable::pick(
        &config::calendar().available_providers(),
        ::calendar::eventkit::access_answered(),
        prompt,
        provider_for,
    )
}

fn provider_for(provider: Provider) -> Option<SharedProvider> {
    match provider {
        // On every OS: off macOS its reads are errors (SPEC §8.2 keeps the
        // `#[cfg]` inside `crates/calendar/src/eventkit.rs`).
        Provider::EventKit => Some(Arc::new(::calendar::eventkit::EventKitProvider::new())),
        // Only with a sign-in (`cloud`).
        Provider::Google => cloud::provider(ProviderId::Google).map(SharedProvider::from),
        Provider::Microsoft => cloud::provider(ProviderId::Microsoft).map(SharedProvider::from),
    }
}

/// Every provider's events in one sorted list; see
/// [`CalendarState::events_between`] for what an error does.
fn merge(
    providers: &[SharedProvider],
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<Vec<Event>, Error> {
    let mut sources = Vec::new();
    let mut answered = providers.is_empty();
    let mut first_error = None;
    for provider in providers {
        match provider.list_events(from, to) {
            Ok(found) => {
                answered = true;
                sources.push(found);
            }
            Err(error) => {
                tracing::warn!(provider = provider.name(), %error, "could not read a calendar");
                first_error.get_or_insert(error);
            }
        }
    }
    if let (false, Some(error)) = (answered, first_error) {
        return Err(error);
    }
    // TUR-49: one sorted list, a meeting in two calendars shown once.
    Ok(::calendar::merge::merge_events(sources))
}

/// Local midnight today and local midnight tomorrow, as UTC instants.
///
/// A midnight that does not exist (a DST jump at 00:00) is taken at the
/// offset `now` has, which is at most an hour off on a day that rare.
pub fn today_bounds<Tz: TimeZone>(now: &DateTime<Tz>) -> (DateTime<Utc>, DateTime<Utc>) {
    let today = now.date_naive();
    let start = midnight(now, today);
    let end = today
        .succ_opt()
        .map(|tomorrow| midnight(now, tomorrow))
        .unwrap_or_else(|| start + Duration::days(1));
    (start, end)
}

fn midnight<Tz: TimeZone>(now: &DateTime<Tz>, day: NaiveDate) -> DateTime<Utc> {
    let naive = day.and_time(chrono::NaiveTime::MIN);
    match now.timezone().from_local_datetime(&naive).earliest() {
        Some(local) => local.with_timezone(&Utc),
        None => {
            let offset = now.offset().fix();
            Utc.from_utc_datetime(&(naive - Duration::seconds(i64::from(offset.local_minus_utc()))))
        }
    }
}

/// One of today's events, as the Today pane shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TodayEvent {
    pub id: String,
    pub title: String,
    /// Start, in milliseconds since the Unix epoch.
    #[specta(type = specta_typescript::Number)]
    pub start_ms: i64,
    /// End, in milliseconds since the Unix epoch.
    #[specta(type = specta_typescript::Number)]
    pub end_ms: i64,
    /// People invited, rooms not counted.
    pub attendees: u32,
    /// Fewer attendees than `detection.min_attendees`: a focus block or a
    /// reminder, not a meeting. Shown greyed and never acted on.
    pub solo: bool,
}

/// What [`todays_meetings`] answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TodaysMeetings {
    pub events: Vec<TodayEvent>,
    /// `calendar.refresh_minutes`: how often the pane asks again.
    pub refresh_minutes: u32,
    /// `detection.min_attendees`: below it an event is `solo`.
    pub min_attendees: u32,
}

impl TodaysMeetings {
    pub fn new(events: Vec<Event>, refresh_minutes: u32, min_attendees: u32) -> Self {
        let events = events
            .into_iter()
            .map(|event| {
                let attendees = u32::try_from(event.attendees).unwrap_or(u32::MAX);
                TodayEvent {
                    id: event.id,
                    title: event.title,
                    start_ms: event.start.timestamp_millis(),
                    end_ms: event.end.timestamp_millis(),
                    attendees,
                    solo: attendees < min_attendees,
                }
            })
            .collect();
        Self {
            events,
            refresh_minutes,
            min_attendees,
        }
    }
}

impl From<Error> for UiError {
    fn from(error: Error) -> Self {
        // `app` domain like the other shell errors; the Today pane branches on
        // `calendar-denied` for its own screen.
        let kind = match &error {
            Error::PermissionDenied => "calendar-denied",
            Error::SignInExpired { .. } => "calendar-sign-in-expired",
            Error::Unreachable { .. } => "calendar-unreachable",
        };
        Self::app(kind, error.to_string())
    }
}

/// Today's events from every configured calendar, local midnight to
/// midnight. Denied access is the error kind `calendar-denied`, never an
/// empty list.
///
/// On the blocking pool: EventKit can wait minutes for the permission answer.
#[tauri::command]
#[specta::specta]
pub async fn todays_meetings(app: AppHandle) -> Result<TodaysMeetings, UiError> {
    tauri::async_runtime::spawn_blocking(move || {
        let refresh_minutes = config::calendar().refresh_minutes;
        let min_attendees = config::detection().min_attendees;
        let (from, to) = today_bounds(&Local::now());
        let events = app.state::<CalendarState>().events_between(from, to)?;
        // TUR-77: the menu bar's Today follows a fresh read (a grant, an edit),
        // and shows this one rather than reading again (TUR-90).
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        crate::tray::reread_soon(&app, &events);
        Ok(TodaysMeetings::new(events, refresh_minutes, min_attendees))
    })
    .await
    .map_err(|error| UiError::app("task-failed", error.to_string()))?
}

/// `calendar.refresh_minutes` alone: a config read, never the calendar. The
/// Today pane asks for it when [`todays_meetings`] fails, so the re-read
/// timer follows the config even while access is denied.
#[tauri::command]
#[specta::specta]
pub async fn calendar_refresh_minutes() -> u32 {
    config::calendar().refresh_minutes
}
