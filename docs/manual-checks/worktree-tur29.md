# Manual checks: TUR-29 (name the recording from the calendar event)

These need the running, signed app, a real calendar invite and macOS
calendar permission, so they were not run here. The headless parts are
covered by tests: `crates/calendar/src/matching.rs` (which event a recording
belongs to), `crates/store/src/meeting_event.rs` (what is written into
`meeting.md` and what is kept), `src-tauri/src/recording/auto_title/tests.rs`
(the whole lookup against `calendar::fake::FakeProvider`: one match, two
overlapping events, no match, a solo block, a hand-typed title, a denied
calendar), `src-tauri/src/meetings/tests.rs` (attendees reach the list row)
and `src/App.test.tsx` (the attendee line under the title).

How it works: once a recording is live, a background thread reads the events
from the recording's start to 5 minutes after it. It picks the event that is
running now or starts within those 5 minutes and has at least
`detection.min_attendees` attendees (default 2); if several fit, the one whose
start is closest to now. It writes `title`, `attendees` and
`calendar_event_id` into the meeting's `meeting.md`. The folder keeps its
`YYYY-MM-DD-HHMM-meeting` name. The title is only set while the meeting still
has its default title ("Meeting"); attendees and the event id only when the
file has none. No match, or no calendar access, leaves the meeting untitled,
with one debug line in `meet-ai.log`.

The events come from the app's `CalendarState` (TUR-28), the same reader
the Today pane uses, so whatever providers `calendar.providers` names in
`config.jsonc` (EventKit today) are the ones searched.

## 1. Record during a real invite → the list shows its title and attendees

1. In Calendar.app, create an event that starts now, lasts 30 minutes, and
   invites at least one other person (two attendees in all). Call it
   "Platform Standup".
2. Build and run the signed app (`just bundle-signed`, then open it). Grant
   calendar access when macOS asks.
3. Start a recording with ⌘⇧R.
4. Expected within a few seconds: the meeting list shows the row as
   "Platform Standup", with the attendee names, comma-separated, in a muted
   line under the title. `meeting.md` in the meeting folder has `title:
   Platform Standup`, an `attendees` list and `calendar_event_id`. The folder
   is still named `YYYY-MM-DD-HHMM-meeting`. `meet-ai.log` has "named the
   recording from its calendar event".

## 2. Starting a few minutes early still matches

1. Create an invite that starts 3 minutes from now. Start recording now.
2. Expected: the meeting takes that event's title.

## 3. A solo block or no event → untitled, no error

1. With only a "Focus time" block on the calendar (no other attendees), or
   nothing at all, start a recording.
2. Expected: the row stays "Meeting", no error is shown, and `meet-ai.log`
   has one debug line ("no calendar event fits the recording").

## 4. Calendar access denied → untitled, no error

1. In System Settings → Privacy & Security → Calendars, turn meet-ai off.
2. Start a recording during an invite.
3. Expected: the row stays "Meeting", the recording runs normally, no error
   is shown, and `meet-ai.log` has "could not read the calendar".

## 5. A hand-typed title wins

1. While a recording is starting during an invite (before access is granted,
   so the lookup is still waiting), open the meeting's `meeting.md` and set
   `title: Budget review`. Then grant access.
2. Expected: the title stays "Budget review"; `attendees` and
   `calendar_event_id` are still filled in.
