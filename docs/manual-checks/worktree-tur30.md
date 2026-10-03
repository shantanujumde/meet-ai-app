# Manual checks: TUR-30 (remind one minute before a meeting, with Record and the brief)

These need the running, signed app, a real calendar invite, calendar and
notification permission, a real meeting app and a Mac that sleeps, so they
were not run here. The headless parts are covered by tests:
`src-tauri/src/detection/reminder/tests.rs` (fake clock and
`calendar::fake::FakeProvider`: fires once at T-60s, solo block silent,
`min_attendees`, a moved event re-arms, a move or a cancellation seen just
before firing, an unreadable calendar keeps the last read, sleep across the
minute skips, waking before the start still reminds, waking re-reads the
calendar, a meeting already running at launch, two meetings at once, and that
Record at the reminder's moment picks this event for the auto-title),
`src-tauri/src/detection/merge.rs` (one prompt per call, both orders),
`src-tauri/src/detection/mod.rs` (the `detection.calendar` switch),
`src-tauri/src/detection/notify.rs` (the notification copy) and
`src/ui/DetectionPrompt.test.tsx` (Open brief, Record, the in-place update).

How it works: a thread ticks every 10 s. It reads the calendar (the app's
`CalendarState`, so the providers `calendar.providers` names) every
`calendar.refresh_minutes`, and again just before a reminder fires. An event
with at least `detection.min_attendees` attendees (default 2) whose start is
60 s or less away, and that has not been reminded about for that start, fires
once: a notification "Record this meeting?" with "“<title>” starts in a
minute, with N people invited. Open meet-ai to record it or read the brief.",
and the in-app banner with **Record**, **Open brief** and **Dismiss**. macOS
notifications from the plugin have no buttons, so the buttons are on the
banner. A tick more than 30 s after the last one (a sleep) re-reads the
calendar first; a meeting that has already started is never reminded about.
The reminder never shows the calendar permission prompt itself: until access
has been answered (the Today pane asks), it reads nothing.

## 1. A test invite with one other attendee → one reminder a minute before

1. Build and run the signed app (`just bundle-signed`, then open it). Let it
   post notifications, and grant calendar access from the Today pane.
2. In Calendar.app, create an event starting 5 minutes from now that invites
   one other person. Call it "Reminder test".
3. Expected at about one minute before the start (within 10 s): one
   notification, and the banner in the meet-ai window reading "“Reminder
   test” starts in a minute, with 2 people invited." `meet-ai.log` has
   "reminding a minute before each meeting" at startup and "a meeting looks
   like it started; asking whether to record" with `Calendar`.
4. Wait until 5 minutes past the start. Expected: no second reminder.

## 2. Record and Open brief

1. Repeat 1 with a new event. When the banner shows, press **Open brief**.
   Expected: the Brief screen for "Reminder test"; the banner is still up.
2. Press **Record**. Expected: a recording starts, and within a few seconds
   the meeting list names it "Reminder test" (TUR-29's auto-title).

## 3. A solo block stays quiet

1. Create an event starting in 3 minutes with no invitees.
2. Expected: no notification and no banner, before or after its start.

## 4. Moved and cancelled events

1. Create an invite (one other attendee) starting in 4 minutes. Before its
   reminder, move it 5 minutes later. Expected: one reminder, a minute before
   the new start; none at the old time.
2. Create another starting in 4 minutes and delete it 2 minutes later.
   Expected: no reminder.

## 5. Sleep across the reminder

1. Create an invite starting in 3 minutes. Put the Mac to sleep at once, and
   wake it 5 minutes later.
2. Expected: no reminder for that meeting after waking (it already started).
   `meet-ai.log` has "the clock jumped (a sleep, most likely); re-reading the
   calendar".

## 6. Zoom plus the reminder → one prompt

1. Create an invite starting in 2 minutes. Open Zoom right after the reminder
   fires. Expected: no second notification or banner for Zoom.
2. Another time, open Zoom (the "Zoom is open." banner shows) and within 3
   minutes have an invite's reminder fire. Expected: no second notification;
   the open banner changes to the reminder's text with **Open brief**. If the
   Zoom banner was dismissed first, nothing comes back.

## 7. `detection.calendar = false`

1. Set `"detection": { "calendar": false }` in
   `~/Meetings/.app/config.jsonc` and restart the app.
2. Expected: no reminders; `meet-ai.log` has "detection.calendar is off; no
   meeting reminders".

## Notes

- `detection.min_attendees` and `calendar.refresh_minutes` are read when the
  app starts; changing them needs a restart for reminders (the Today pane
  reads them on every refresh).
- Which events were reminded about is kept in memory, so quitting and
  reopening the app inside the minute before a meeting reminds again.
