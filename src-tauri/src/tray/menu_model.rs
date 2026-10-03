//! What the menu bar shows about today's meetings (TUR-77), as plain data.
//!
//! `tray.rs` reads the calendar and draws; every decision about *what* to
//! draw is a pure function here, so it is unit-tested without a menu bar,
//! Tauri or a calendar:
//!
//! - [`build_menu_model`]: the "Today" section at the top of the menu. The
//!   next meetings of the rest of today, at most [`MAX_MEETINGS`], each a
//!   submenu with **Join** (only with a link), **Record** and **Open brief**.
//!   One happening now is marked "Now". An event with fewer than
//!   `detection.min_attendees` people (a focus block) is shown greyed and
//!   cannot be opened, like the Today pane's `solo` rows.
//! - [`countdown_title`]: the optional "Weekly sync in 12m" next to the icon.

use chrono::{DateTime, Duration, TimeZone};

use ::calendar::Event;

/// How many of today's meetings the menu lists.
pub const MAX_MEETINGS: usize = 5;

/// How long before a meeting its countdown appears next to the icon.
pub const COUNTDOWN_FROM: Duration = Duration::minutes(60);

/// The section's heading.
pub const TODAY_HEADING: &str = "Today";
/// The marker for a meeting that is happening now, in place of its time.
pub const NOW_LABEL: &str = "Now";
/// The read worked and nothing else is on today.
pub const NO_MORE_MEETINGS: &str = "No more meetings today";
/// No calendar, or one meet-ai may not read. Clicking it opens Settings.
pub const NOT_CONNECTED: &str = "Calendar not connected";
/// Before the first read has answered.
pub const READING: &str = "Reading your calendar…";

/// The submenu's items.
pub const JOIN_LABEL: &str = "Join";
pub const RECORD_LABEL: &str = "Record";
pub const BRIEF_LABEL: &str = "Open brief";

/// Longest title the menu shows before cutting it with an ellipsis. A
/// pasted-in agenda as a title would otherwise make the menu screen-wide.
const MENU_TITLE_CHARS: usize = 40;
/// Longest title next to the icon: the menu bar is shared with every other
/// app's items.
const COUNTDOWN_TITLE_CHARS: usize = 24;

/// How the time column is written: `chrono` pattern, 24-hour. The system's
/// 12/24-hour preference is a follow-up (see the manual-checks file).
const TIME_FORMAT: &str = "%H:%M";

/// What the last calendar read said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalendarRead {
    /// Not read yet: the app just started.
    Pending,
    /// Read; today's events (any order, any of them maybe over).
    Events(Vec<Event>),
    /// No calendar set up, access denied or not yet granted, a sign-in
    /// expired, or no calendar on this OS: nothing the menu can list.
    NotConnected,
}

/// One line of the "Today" section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry {
    /// The "Today" heading. Not clickable.
    Heading(&'static str),
    /// One meeting.
    Meeting(MeetingEntry),
    /// A line of text that does nothing: "No more meetings today".
    Note(&'static str),
    /// "Calendar not connected": opens Settings.
    ConnectCalendar(&'static str),
}

/// One of today's meetings, as the menu lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingEntry {
    /// The calendar event's id: the menu items' ids are built from it.
    pub event_id: String,
    /// `10:30  Weekly sync`, or `Now  Weekly sync`.
    pub label: String,
    /// Happening now.
    pub now: bool,
    /// Fewer people than `min_attendees`: greyed, no submenu.
    pub solo: bool,
    /// Present only when the event has a link: **Join** is shown only then.
    pub join_url: Option<String>,
}

/// The "Today" section for `read`, at `now` (whose time zone the times are
/// written in).
///
/// Lists the events that have not ended yet, by start (ties by id), at most
/// [`MAX_MEETINGS`]. `read` is expected to hold today's events already; one
/// starting after `now`'s day is still listed if it is there, since the
/// caller chose the range.
pub fn build_menu_model<Tz: TimeZone>(
    read: &CalendarRead,
    now: &DateTime<Tz>,
    min_attendees: usize,
) -> Vec<MenuEntry>
where
    Tz::Offset: std::fmt::Display,
{
    let mut entries = vec![MenuEntry::Heading(TODAY_HEADING)];
    let events = match read {
        CalendarRead::Pending => {
            entries.push(MenuEntry::Note(READING));
            return entries;
        }
        CalendarRead::NotConnected => {
            entries.push(MenuEntry::ConnectCalendar(NOT_CONNECTED));
            return entries;
        }
        CalendarRead::Events(events) => events,
    };
    let upcoming = upcoming(events, now);
    if upcoming.is_empty() {
        entries.push(MenuEntry::Note(NO_MORE_MEETINGS));
        return entries;
    }
    entries.extend(
        upcoming
            .into_iter()
            .take(MAX_MEETINGS)
            .map(|event| MenuEntry::Meeting(meeting_entry(event, now, min_attendees))),
    );
    entries
}

/// The text next to the menu-bar icon: the next meeting (with at least
/// `min_attendees` people) that starts within [`COUNTDOWN_FROM`], as
/// "Weekly sync in 12m". `None` when there is none, or nothing was read.
///
/// Minutes are rounded up, so it never says "in 0m": a meeting 30 seconds
/// away is "in 1m". A meeting that has started has no countdown.
pub fn countdown_title<Tz: TimeZone>(
    read: &CalendarRead,
    now: &DateTime<Tz>,
    min_attendees: usize,
) -> Option<String> {
    let CalendarRead::Events(events) = read else {
        return None;
    };
    let next = upcoming(events, now)
        .into_iter()
        .filter(|e| e.attendees >= min_attendees)
        .find(|e| e.start > *now)?;
    let until = next.start - now.to_utc();
    if until > COUNTDOWN_FROM {
        return None;
    }
    let seconds = until.num_seconds();
    let minutes = (seconds + 59) / 60;
    Some(format!(
        "{} in {minutes}m",
        shorten(&next.title, COUNTDOWN_TITLE_CHARS)
    ))
}

/// Events not yet over at `now`, by start then id.
fn upcoming<'a, Tz: TimeZone>(events: &'a [Event], now: &DateTime<Tz>) -> Vec<&'a Event> {
    let mut left: Vec<&Event> = events.iter().filter(|e| e.end > *now).collect();
    left.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.id.cmp(&b.id)));
    left
}

fn meeting_entry<Tz: TimeZone>(
    event: &Event,
    now: &DateTime<Tz>,
    min_attendees: usize,
) -> MeetingEntry
where
    Tz::Offset: std::fmt::Display,
{
    let happening = event.start <= *now;
    let when = if happening {
        NOW_LABEL.to_owned()
    } else {
        event
            .start
            .with_timezone(&now.timezone())
            .format(TIME_FORMAT)
            .to_string()
    };
    MeetingEntry {
        event_id: event.id.clone(),
        label: format!("{when}  {}", shorten(&event.title, MENU_TITLE_CHARS)),
        now: happening,
        solo: event.attendees < min_attendees,
        // TUR-86: a link the Join check refuses gets no Join at all.
        join_url: event
            .join_url
            .clone()
            .filter(|url| ::calendar::join_url::is_safe_join_url(url)),
    }
}

/// `title` cut to `max` characters with an ellipsis; a blank title reads
/// "Untitled" rather than leaving an empty-looking row.
fn shorten(title: &str, max: usize) -> String {
    let title = title.trim();
    if title.is_empty() {
        return "Untitled".to_owned();
    }
    if title.chars().count() <= max {
        return title.to_owned();
    }
    let cut: String = title.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, Utc};

    use super::*;

    /// 2026-10-05 at `h:m` UTC.
    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 5, h, m, 0)
            .single()
            .expect("a valid time")
    }

    fn event(id: &str, start: DateTime<Utc>, minutes: i64, attendees: usize) -> Event {
        Event {
            id: id.into(),
            title: format!("Meeting {id}"),
            start,
            end: start + Duration::minutes(minutes),
            attendees,
            attendee_names: Vec::new(),
            ical_uid: None,
            join_url: None,
        }
    }

    fn meetings(entries: &[MenuEntry]) -> Vec<&MeetingEntry> {
        entries
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Meeting(m) => Some(m),
                _ => None,
            })
            .collect()
    }

    fn ids(entries: &[MenuEntry]) -> Vec<&str> {
        meetings(entries)
            .into_iter()
            .map(|m| m.event_id.as_str())
            .collect()
    }

    #[test]
    fn lists_what_is_left_of_today_in_start_order() {
        let read = CalendarRead::Events(vec![
            event("c", at(15, 0), 30, 3),
            event("over", at(8, 0), 30, 3),
            event("b", at(11, 0), 30, 3),
            event("a", at(11, 0), 15, 3),
        ]);
        let entries = build_menu_model(&read, &at(10, 0), 2);
        assert_eq!(entries[0], MenuEntry::Heading(TODAY_HEADING));
        assert_eq!(
            ids(&entries),
            vec!["a", "b", "c"],
            "ties by id, past dropped"
        );
        assert_eq!(meetings(&entries)[2].label, "15:00  Meeting c");
    }

    #[test]
    fn at_most_five() {
        let read = CalendarRead::Events(
            (0..8)
                .map(|i| event(&format!("m{i}"), at(11 + i, 0), 30, 2))
                .collect(),
        );
        let entries = build_menu_model(&read, &at(10, 0), 2);
        assert_eq!(ids(&entries), vec!["m0", "m1", "m2", "m3", "m4"]);
    }

    #[test]
    fn solo_events_are_greyed_but_listed() {
        let read = CalendarRead::Events(vec![
            event("focus", at(11, 0), 60, 1),
            event("sync", at(12, 0), 30, 2),
        ]);
        let entries = build_menu_model(&read, &at(10, 0), 2);
        let listed = meetings(&entries);
        assert!(listed[0].solo, "fewer than min_attendees");
        assert!(!listed[1].solo);
    }

    #[test]
    fn a_meeting_under_way_is_marked_now() {
        let read = CalendarRead::Events(vec![
            event("now", at(9, 45), 30, 2),
            event("next", at(10, 30), 30, 2),
        ]);
        let entries = build_menu_model(&read, &at(10, 0), 2);
        let listed = meetings(&entries);
        assert!(listed[0].now);
        assert_eq!(listed[0].label, "Now  Meeting now");
        assert!(!listed[1].now);
        assert_eq!(listed[1].label, "10:30  Meeting next");
    }

    #[test]
    fn a_meeting_that_just_ended_is_gone() {
        let read = CalendarRead::Events(vec![event("done", at(9, 30), 30, 2)]);
        assert_eq!(
            build_menu_model(&read, &at(10, 0), 2),
            vec![
                MenuEntry::Heading(TODAY_HEADING),
                MenuEntry::Note(NO_MORE_MEETINGS)
            ]
        );
    }

    #[test]
    fn an_empty_day_says_so() {
        assert_eq!(
            build_menu_model(&CalendarRead::Events(vec![]), &at(10, 0), 2),
            vec![
                MenuEntry::Heading(TODAY_HEADING),
                MenuEntry::Note(NO_MORE_MEETINGS)
            ]
        );
    }

    #[test]
    fn a_denied_calendar_offers_to_connect() {
        assert_eq!(
            build_menu_model(&CalendarRead::NotConnected, &at(10, 0), 2),
            vec![
                MenuEntry::Heading(TODAY_HEADING),
                MenuEntry::ConnectCalendar(NOT_CONNECTED)
            ]
        );
    }

    #[test]
    fn before_the_first_read_it_says_it_is_reading() {
        assert_eq!(
            build_menu_model(&CalendarRead::Pending, &at(10, 0), 2),
            vec![MenuEntry::Heading(TODAY_HEADING), MenuEntry::Note(READING)]
        );
    }

    #[test]
    fn join_only_with_a_link() {
        let mut zoom = event("zoom", at(11, 0), 30, 2);
        zoom.join_url = Some("https://zoom.us/j/1".into());
        let read = CalendarRead::Events(vec![zoom, event("room", at(12, 0), 30, 2)]);
        let entries = build_menu_model(&read, &at(10, 0), 2);
        let listed = meetings(&entries);
        assert_eq!(listed[0].join_url.as_deref(), Some("https://zoom.us/j/1"));
        assert_eq!(listed[1].join_url, None);
    }

    #[test]
    fn no_join_for_a_link_a_browser_would_open_elsewhere() {
        let mut phish = event("phish", at(11, 0), 30, 2);
        phish.join_url = Some("https://evil.example\\@meet.google.com/abc-defg-hij".into());
        let mut userinfo = event("userinfo", at(12, 0), 30, 2);
        userinfo.join_url = Some("https://meet.google.com@evil.example/abc".into());
        let read = CalendarRead::Events(vec![phish, userinfo]);
        let entries = build_menu_model(&read, &at(10, 0), 2);
        assert!(meetings(&entries).iter().all(|m| m.join_url.is_none()));
    }

    #[test]
    fn times_are_written_in_the_local_zone() {
        let ist = FixedOffset::east_opt(5 * 3600 + 1800).expect("a valid offset");
        let read = CalendarRead::Events(vec![event("a", at(11, 0), 30, 2)]);
        let entries = build_menu_model(&read, &at(10, 0).with_timezone(&ist), 2);
        assert_eq!(meetings(&entries)[0].label, "16:30  Meeting a");
    }

    #[test]
    fn long_and_blank_titles_are_tidied() {
        let mut long = event("long", at(11, 0), 30, 2);
        long.title = "A".repeat(80);
        let mut blank = event("blank", at(12, 0), 30, 2);
        blank.title = "  ".into();
        let entries = build_menu_model(&CalendarRead::Events(vec![long, blank]), &at(10, 0), 2);
        let listed = meetings(&entries);
        assert_eq!(listed[0].label, format!("11:00  {}…", "A".repeat(39)));
        assert_eq!(listed[1].label, "12:00  Untitled");
    }

    #[test]
    fn countdown_within_the_hour_only() {
        let read = CalendarRead::Events(vec![event("sync", at(10, 12), 30, 2)]);
        assert_eq!(
            countdown_title(&read, &at(10, 0), 2).as_deref(),
            Some("Meeting sync in 12m")
        );
        assert_eq!(countdown_title(&read, &at(9, 0), 2), None, "61 min away");
        assert_eq!(
            countdown_title(&read, &at(9, 12), 2).as_deref(),
            Some("Meeting sync in 60m")
        );
        assert_eq!(countdown_title(&read, &at(10, 15), 2), None, "started");
    }

    #[test]
    fn countdown_rounds_up_and_skips_solo_events() {
        let read = CalendarRead::Events(vec![
            event("focus", at(10, 1), 30, 1),
            event("sync", at(10, 20), 30, 2),
        ]);
        let now = at(10, 0) + Duration::seconds(30);
        assert_eq!(
            countdown_title(&read, &now, 2).as_deref(),
            Some("Meeting sync in 20m")
        );
        let soon = CalendarRead::Events(vec![event("x", at(10, 1), 30, 2)]);
        assert_eq!(
            countdown_title(&soon, &now, 2).as_deref(),
            Some("Meeting x in 1m")
        );
    }

    #[test]
    fn no_countdown_without_a_calendar() {
        assert_eq!(countdown_title(&CalendarRead::Pending, &at(10, 0), 2), None);
        assert_eq!(
            countdown_title(&CalendarRead::NotConnected, &at(10, 0), 2),
            None
        );
    }
}
