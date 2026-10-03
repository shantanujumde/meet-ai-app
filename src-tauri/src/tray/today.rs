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
//! What to draw is `menu_model`'s pure functions; this file only reads,
//! renders and reacts.

use std::sync::Mutex;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration as StdDuration, Instant};

use ::calendar::Event;
use chrono::{Local, NaiveDate, Timelike as _};
use tauri::menu::{
    IsMenuItem, MenuBuilder, MenuItem, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder,
};
use tauri::{AppHandle, Manager as _, Wry};
use tauri_plugin_opener::OpenerExt as _;

use super::menu_model::{
    self, BRIEF_LABEL, CalendarRead, JOIN_LABEL, MeetingEntry, MenuEntry, RECORD_LABEL,
};
use crate::config::{self, Provider};
use crate::lifecycle::{self, NavigateTo};
use crate::recording::auto_title::PinnedEvent;

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

/// Managed state: the last read, for the item clicks, and the worker's inbox.
#[derive(Default)]
pub(super) struct Today {
    read: Mutex<Option<CalendarRead>>,
    nudge: Mutex<Option<Sender<Nudge>>>,
}

impl Today {
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
        .item(&MenuItemBuilder::with_id(format!("{RECORD_PREFIX}{id}"), RECORD_LABEL).build(app)?)
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

/// Read the calendar again now: the Today pane just did, so a grant or an
/// edit reaches the menu bar without waiting for the next refresh.
pub fn reread_soon(app: &AppHandle) {
    if let Some(today) = app.try_state::<Today>() {
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
    let mut shown: Option<(Vec<MenuEntry>, Option<String>)> = None;
    let mut reread = true;
    loop {
        let now = Local::now();
        let every = StdDuration::from_secs(u64::from(config::calendar().refresh_minutes) * 60);
        let stale =
            last_read.is_none_or(|(at, day)| at.elapsed() >= every || day != now.date_naive());
        if reread || stale {
            let read = read_today(app, &now);
            *app.state::<Today>()
                .read
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(read);
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
            let entries = menu_model::build_menu_model(&read, &now, min_attendees);
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

/// Today's events, local midnight to midnight, or why there are none.
///
/// Never asks for calendar access, like the reminders: the Today pane does,
/// where the user can see why. Until it has been answered the menu says the
/// calendar is not connected.
fn read_today(app: &AppHandle, now: &chrono::DateTime<Local>) -> CalendarRead {
    let providers = config::calendar().available_providers();
    if providers.is_empty()
        || (providers.contains(&Provider::EventKit) && !::calendar::eventkit::access_answered())
    {
        return CalendarRead::NotConnected;
    }
    let Some(state) = app.try_state::<crate::calendar::CalendarState>() else {
        return CalendarRead::NotConnected;
    };
    let (from, to) = crate::calendar::today_bounds(now);
    match state.events_between(from, to) {
        Ok(events) => CalendarRead::Events(events),
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
            let pin = app.state::<PinnedEvent>();
            pin.pin(event);
            let started = crate::folder_move::start_recording(&app);
            // Taken by the start if it got that far; never left for a later one.
            pin.clear();
            match started {
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
