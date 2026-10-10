//! Today's meetings in the menu bar (TUR-77): the worker that keeps the
//! "Today" section and the countdown title current, and what its items do.
//!
//! One thread, one timer. It wakes at the top of every minute (so "Now" and
//! the countdown flip on time), re-reads the calendar every
//! `calendar.refresh_minutes` and when the date changes, and redraws only
//! when what it would draw changed. A nudge ([`reread_soon`],
//! [`redraw_soon`]) wakes it early: the Today pane just read the calendar,
//! or the countdown setting was switched.
//!
//! The Today pane's last good read is kept here with its time (TUR-90), and
//! the worker takes it instead of reading again while it is younger than
//! `calendar.refresh_minutes`: one Today refresh reads each cloud calendar
//! once, not once for the pane and once more for the menu bar.
//!
//! What to draw is `menu_model`'s pure functions; this file only reads,
//! renders and reacts.

use std::sync::Mutex;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration as StdDuration, Instant};

use ::calendar::{Error, Event};
use chrono::{Local, NaiveDate, Timelike as _};
use tauri::menu::{
    IsMenuItem, MenuBuilder, MenuItem, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder,
};
use tauri::{AppHandle, Manager as _, Wry};
use tauri_plugin_opener::OpenerExt as _;

use super::menu_model::{
    self, BRIEF_LABEL, CalendarRead, JOIN_LABEL, MeetingEntry, MenuEntry, RECORD_LABEL,
};
use crate::config;
use crate::lifecycle::{self, NavigateTo};

/// "Calendar not connected".
const CONNECT_ITEM: &str = "tray-today-connect";
/// Menu item ids for one meeting: the prefix, then the event's id.
const JOIN_PREFIX: &str = "tray-join:";
const RECORD_PREFIX: &str = "tray-record:";
const BRIEF_PREFIX: &str = "tray-brief:";

/// The menu's fixed items, built once and put in every rebuilt menu, so the
/// recording item's live label (`tray.rs`) survives a rebuild.
pub(super) struct Fixed {
    pub open: MenuItem<Wry>,
    pub toggle: MenuItem<Wry>,
    pub quit: MenuItem<Wry>,
}

/// Why the worker was woken early.
enum Nudge {
    /// Read the calendar again now.
    Reread,
    /// Draw again now (a setting changed); the last read is still good.
    Redraw,
}

/// The Today pane's last good read: when, for which day, and what.
struct PaneRead {
    at: Instant,
    day: NaiveDate,
    events: Vec<Event>,
}

/// Managed state: the last read, for the item clicks, the pane's last read,
/// and the worker's inbox.
#[derive(Default)]
pub(super) struct Today {
    read: Mutex<Option<CalendarRead>>,
    pane: Mutex<Option<PaneRead>>,
    nudge: Mutex<Option<Sender<Nudge>>>,
}

impl Today {
    /// The pane's events, if it read `day` less than `every` ago.
    fn pane_events(&self, every: StdDuration, day: NaiveDate) -> Option<Vec<Event>> {
        let pane = self.pane.lock().unwrap_or_else(|e| e.into_inner());
        fresh(pane.as_ref(), every, day).map(<[Event]>::to_vec)
    }

    fn event(&self, id: &str) -> Option<Event> {
        let read = self.read.lock().unwrap_or_else(|e| e.into_inner());
        match read.as_ref()? {
            CalendarRead::Events(events) => events.iter().find(|e| e.id == id).cloned(),
            _ => None,
        }
    }

    fn send(&self, nudge: Nudge) {
        if let Some(tx) = self
            .nudge
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            // A worker that has gone has nobody to wake.
            let _ = tx.send(nudge);
        }
    }
}

/// The menu for `entries`, with the fixed items under the "Today" section.
pub(super) fn menu(
    app: &AppHandle,
    entries: &[MenuEntry],
    fixed: &Fixed,
) -> tauri::Result<tauri::menu::Menu<Wry>> {
    let mut items: Vec<Box<dyn IsMenuItem<Wry>>> = Vec::new();
    for entry in entries {
        items.push(match entry {
            MenuEntry::Heading(text) | MenuEntry::Note(text) => {
                Box::new(MenuItemBuilder::new(*text).enabled(false).build(app)?)
            }
            MenuEntry::ConnectCalendar(text) => {
                Box::new(MenuItemBuilder::with_id(CONNECT_ITEM, *text).build(app)?)
            }
            // Greyed like the Today pane's solo rows, and never acted on.
            MenuEntry::Meeting(meeting) if meeting.solo => Box::new(
                MenuItemBuilder::new(&meeting.label)
                    .enabled(false)
                    .build(app)?,
            ),
            MenuEntry::Meeting(meeting) => Box::new(submenu(app, meeting)?),
        });
    }
    let separator = PredefinedMenuItem::separator(app)?;
    let mut builder = MenuBuilder::new(app);
    for item in &items {
        builder = builder.item(item.as_ref());
    }
    builder
        .item(&separator)
        .item(&fixed.open)
        .separator()
        .item(&fixed.toggle)
        .separator()
        .item(&fixed.quit)
        .build()
}

fn submenu(app: &AppHandle, meeting: &MeetingEntry) -> tauri::Result<tauri::menu::Submenu<Wry>> {
    let id = &meeting.event_id;
    let mut builder = SubmenuBuilder::new(app, &meeting.label);
    if meeting.join_url.is_some() {
        builder = builder
            .item(&MenuItemBuilder::with_id(format!("{JOIN_PREFIX}{id}"), JOIN_LABEL).build(app)?);
    }
    builder
        .item(
            &MenuItemBuilder::with_id(format!("{RECORD_PREFIX}{id}"), RECORD_LABEL)
                .enabled(meeting.can_record)
                .build(app)?,
        )
        .item(&MenuItemBuilder::with_id(format!("{BRIEF_PREFIX}{id}"), BRIEF_LABEL).build(app)?)
        .build()
}

/// Start the worker. `tray_id` names the tray to redraw.
pub(super) fn start(app: &AppHandle, tray_id: &'static str, fixed: Fixed) {
    let (tx, rx) = mpsc::channel();
    app.manage(Today::default());
    *app.state::<Today>()
        .nudge
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(tx);
    let handle = app.clone();
    if let Err(error) = std::thread::Builder::new()
        .name("meet-ai-tray-today".to_string())
        .spawn(move || run(&handle, tray_id, &fixed, &rx))
    {
        tracing::warn!(%error, "could not start the menu bar's Today section");
    }
}

/// The Today pane just read `events`: show them now, so a grant or an edit
/// reaches the menu bar without waiting for the next refresh, and without a
/// read of its own (TUR-90).
pub fn reread_soon(app: &AppHandle, events: &[Event]) {
    if let Some(today) = app.try_state::<Today>() {
        *today.pane.lock().unwrap_or_else(|e| e.into_inner()) = Some(PaneRead {
            at: Instant::now(),
            day: Local::now().date_naive(),
            events: events.to_vec(),
        });
        today.send(Nudge::Reread);
    }
}

/// Draw again now: the countdown setting changed.
pub fn redraw_soon(app: &AppHandle) {
    if let Some(today) = app.try_state::<Today>() {
        today.send(Nudge::Redraw);
    }
}

fn run(app: &AppHandle, tray_id: &str, fixed: &Fixed, rx: &mpsc::Receiver<Nudge>) {
    let mut last_read: Option<(Instant, NaiveDate)> = None;
    // The day of the last read that worked: a failed read may keep its
    // events, never yesterday's (TUR-88).
    let mut good_day: Option<NaiveDate> = None;
    let mut shown: Option<(Vec<MenuEntry>, Option<String>)> = None;
    let mut reread = true;
    loop {
        let now = Local::now();
        let every = StdDuration::from_secs(u64::from(config::calendar().refresh_minutes) * 60);
        let stale =
            last_read.is_none_or(|(at, day)| at.elapsed() >= every || day != now.date_naive());
        if reread || stale {
            // The pane's read when it is fresh enough; a read of our own
            // only when it is not.
            let result = match app.state::<Today>().pane_events(every, now.date_naive()) {
                Some(events) => Ok(events),
                None => read_today(app, &now),
            };
            if result.is_ok() {
                good_day = Some(now.date_naive());
            }
            let today = app.state::<Today>();
            let mut shown_read = today.read.lock().unwrap_or_else(|e| e.into_inner());
            let next = settle(
                shown_read.as_ref(),
                good_day == Some(now.date_naive()),
                result,
            );
            *shown_read = Some(next);
            drop(shown_read);
            last_read = Some((Instant::now(), now.date_naive()));
            reread = false;
        }

        let min_attendees = config::detection().min_attendees as usize;
        let drawn = {
            let read = app
                .state::<Today>()
                .read
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
                .unwrap_or(CalendarRead::Pending);
            let recording = app
                .try_state::<crate::recording::Recorder>()
                .is_some_and(|r| r.status().phase != crate::recording::Phase::Idle);
            let entries = menu_model::build_menu_model(
                &read,
                &now,
                min_attendees,
                recording,
                crate::platform::clock(),
            );
            let title = config::app()
                .menu_bar_countdown
                .then(|| menu_model::countdown_title(&read, &now, min_attendees))
                .flatten();
            (entries, title)
        };
        if shown.as_ref() != Some(&drawn) {
            draw(app, tray_id, fixed, &drawn.0, drawn.1.as_deref());
            shown = Some(drawn);
        }

        // The top of the next minute, so "Now" and the countdown change on time.
        let wait = StdDuration::from_secs(u64::from(60 - Local::now().second().min(59)));
        match rx.recv_timeout(wait) {
            Ok(Nudge::Reread) => reread = true,
            Ok(Nudge::Redraw) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// Today's events, local midnight to midnight, from every calendar that can
/// be read now.
///
/// Never asks for calendar access, like the reminders: the Today pane does,
/// where the user can see why. Until Calendar.app's prompt is answered the
/// other calendars are still read (TUR-88, `calendar::readable`); with none
/// left, the read is `PermissionDenied`.
fn read_today(app: &AppHandle, now: &chrono::DateTime<Local>) -> Result<Vec<Event>, Error> {
    let Some(state) = app.try_state::<crate::calendar::CalendarState>() else {
        return Err(Error::PermissionDenied);
    };
    let (from, to) = crate::calendar::today_bounds(now);
    state.events_between_unprompted(from, to)
}

/// `pane`'s events when they are `day`'s and younger than `every`.
fn fresh(pane: Option<&PaneRead>, every: StdDuration, day: NaiveDate) -> Option<&[Event]> {
    pane.filter(|pane| pane.day == day && pane.at.elapsed() < every)
        .map(|pane| pane.events.as_slice())
}

/// What the menu shows after a read, given what it showed before.
///
/// An unreachable calendar (offline, a Wi-Fi blip, Graph's 429 or 503) is
/// passing, so the last good list stays while it is still today's (TUR-88),
/// the way the reminders keep theirs. "Not connected" is for what the user
/// has to fix (access denied, an expired sign-in, nothing set up) and for an
/// unreachable calendar with nothing read yet today.
fn settle(
    previous: Option<&CalendarRead>,
    previous_is_today: bool,
    read: Result<Vec<Event>, Error>,
) -> CalendarRead {
    match read {
        Ok(events) => CalendarRead::Events(events),
        Err(error @ Error::Unreachable { .. }) => match previous {
            Some(CalendarRead::Events(events)) if previous_is_today => {
                tracing::debug!(%error, "the menu bar could not read the calendar; keeping the last read");
                CalendarRead::Events(events.clone())
            }
            _ => {
                tracing::debug!(%error, "the menu bar could not read the calendar");
                CalendarRead::NotConnected
            }
        },
        Err(error) => {
            tracing::debug!(%error, "the menu bar could not read the calendar");
            CalendarRead::NotConnected
        }
    }
}

fn draw(app: &AppHandle, tray_id: &str, fixed: &Fixed, entries: &[MenuEntry], title: Option<&str>) {
    let Some(tray) = app.tray_by_id(tray_id) else {
        return;
    };
    match menu(app, entries, fixed) {
        Ok(menu) => {
            if let Err(error) = tray.set_menu(Some(menu)) {
                tracing::warn!(%error, "could not update the menu-bar menu");
            }
        }
        Err(error) => tracing::warn!(%error, "could not build the menu-bar menu"),
    }
    crate::platform::set_tray_title(&tray, title);
}

/// A click on one of the "Today" items. `false` when `id` is not one.
pub(super) fn on_click(app: &AppHandle, id: &str) -> bool {
    if id == CONNECT_ITEM {
        lifecycle::navigate(app, NavigateTo::Settings);
    } else if let Some(event_id) = id.strip_prefix(JOIN_PREFIX) {
        join(app, event_id);
    } else if let Some(event_id) = id.strip_prefix(RECORD_PREFIX) {
        record(app, event_id);
    } else if let Some(event_id) = id.strip_prefix(BRIEF_PREFIX) {
        match app.state::<Today>().event(event_id) {
            Some(event) => lifecycle::navigate(app, NavigateTo::Brief { title: event.title }),
            None => tracing::warn!("the meeting for Open brief is no longer on today's list"),
        }
    } else {
        return false;
    }
    true
}

/// Open the call's link in the browser or the meeting app that owns it.
fn join(app: &AppHandle, event_id: &str) {
    let Some(url) = app
        .state::<Today>()
        .event(event_id)
        .and_then(|event| event.join_url)
    else {
        tracing::warn!("the meeting for Join has no link any more");
        return;
    };
    // The providers' fields are someone else's data: open only a link a
    // browser would take to a known call service (TUR-86), checked again here
    // so nothing between the read and the click can slip one in.
    if !calendar::join_url::is_safe_join_url(&url) {
        tracing::warn!("refusing to open a meeting link that is not a known call service");
        return;
    }
    if let Err(error) = app.opener().open_url(url, None::<&str>) {
        tracing::warn!(%error, "could not open the meeting link");
    }
}

/// Start a recording named from this meeting (TUR-29's auto-title), off the
/// main thread like the menu bar's Start recording.
fn record(app: &AppHandle, event_id: &str) {
    let Some(event) = app.state::<Today>().event(event_id) else {
        tracing::warn!("the meeting for Record is no longer on today's list");
        return;
    };
    let app = app.clone();
    if let Err(error) = std::thread::Builder::new()
        .name("meet-ai-tray-record".to_string())
        .spawn(move || {
            match crate::folder_move::start_recording(&app, Some(event)) {
                Ok(status) => tracing::info!(phase = ?status.phase, "recording started from the menu bar's Today"),
                Err(error) => {
                    tracing::warn!(message = %error.message, "recording from the menu bar refused");
                    crate::notify::refusal(&app, false, &error);
                }
            }
        })
    {
        tracing::error!(%error, "could not spawn a worker thread for Record");
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone as _, Utc};

    use super::*;

    fn standup() -> Event {
        let start = Utc
            .with_ymd_and_hms(2026, 10, 5, 9, 0, 0)
            .single()
            .expect("valid test time");
        Event {
            id: "standup".into(),
            title: "Standup".into(),
            start,
            end: start + chrono::Duration::minutes(15),
            attendees: 3,
            attendee_names: Vec::new(),
            ical_uid: None,
            join_url: None,
        }
    }

    fn offline() -> Result<Vec<Event>, Error> {
        Err(Error::Unreachable {
            provider: "Microsoft",
            detail: "Graph answered 503".into(),
        })
    }

    #[test]
    fn a_network_blip_keeps_todays_list() {
        let shown = CalendarRead::Events(vec![standup()]);
        assert_eq!(settle(Some(&shown), true, offline()), shown);
    }

    #[test]
    fn a_network_blip_with_nothing_read_yet_is_not_connected() {
        for previous in [
            None,
            Some(CalendarRead::Pending),
            Some(CalendarRead::NotConnected),
        ] {
            assert_eq!(
                settle(previous.as_ref(), true, offline()),
                CalendarRead::NotConnected
            );
        }
    }

    #[test]
    fn a_network_blip_never_keeps_yesterdays_list() {
        let yesterday = CalendarRead::Events(vec![standup()]);
        assert_eq!(
            settle(Some(&yesterday), false, offline()),
            CalendarRead::NotConnected
        );
    }

    #[test]
    fn what_the_user_must_fix_is_not_connected_at_once() {
        let shown = CalendarRead::Events(vec![standup()]);
        for error in [
            Error::PermissionDenied,
            Error::SignInExpired { provider: "Google" },
        ] {
            assert_eq!(
                settle(Some(&shown), true, Err(error)),
                CalendarRead::NotConnected
            );
        }
    }

    fn pane(age: StdDuration, day: NaiveDate) -> PaneRead {
        PaneRead {
            at: Instant::now()
                .checked_sub(age)
                .expect("the test's age fits in an Instant"),
            day,
            events: vec![standup()],
        }
    }

    #[test]
    fn a_fresh_pane_read_is_reused_instead_of_reading_again() {
        let day = NaiveDate::from_ymd_opt(2026, 10, 5).expect("valid test date");
        let every = StdDuration::from_secs(5 * 60);
        let read = pane(StdDuration::from_secs(1), day);
        assert_eq!(fresh(Some(&read), every, day), Some(&[standup()][..]));
    }

    #[test]
    fn an_old_missing_or_yesterdays_pane_read_is_not_reused() {
        let day = NaiveDate::from_ymd_opt(2026, 10, 5).expect("valid test date");
        let every = StdDuration::from_secs(5 * 60);
        assert_eq!(fresh(None, every, day), None);
        let old = pane(StdDuration::from_secs(6 * 60), day);
        assert_eq!(fresh(Some(&old), every, day), None);
        let yesterday = pane(
            StdDuration::from_secs(1),
            day.pred_opt().expect("a day before"),
        );
        assert_eq!(fresh(Some(&yesterday), every, day), None);
    }

    #[test]
    fn a_good_read_replaces_whatever_was_shown() {
        let read = settle(
            Some(&CalendarRead::NotConnected),
            false,
            Ok(vec![standup()]),
        );
        assert_eq!(read, CalendarRead::Events(vec![standup()]));
    }
}
