# Manual checks: TUR-28 (Today's meetings pane, straight from the calendar)

These need the running, signed app, real events in Calendar.app and a real
calendar permission prompt, so they were not run here: an agent run cannot
answer a TCC prompt, and macOS only keeps calendar permission for a signed
bundle. The headless parts are covered by:

- `cargo test -p meet-ai --lib calendar`: `CalendarState` merges and sorts
  every provider's events, a denied calendar is an error (kind
  `calendar-denied`), not an empty day, one readable provider is enough,
  local midnight to midnight, solo below `min_attendees`, the camelCase wire
  shape.
- `pnpm vitest run src/ui/TodayPane.test.tsx`: loading, no meetings today,
  denied (with **Open System Settings**), the list with greyed solo blocks,
  another error, the `refresh_minutes` timer and the re-read on window focus.

The app's entitlements now include
`com.apple.security.personal-information.calendars`, which the hardened
runtime needs before macOS will show the calendar prompt at all. Check 1
confirms it.

## 1. Today's real events show up

1. On a signed build, make three events for today in Calendar.app: a meeting
   with two or more invitees, one with a single invitee, and one with none.
   Add an all-day event too.
2. Open meet-ai and go to Meetings. Allow calendar access when macOS asks.
3. Expected: a **Today** section at the top lists the three timed events in
   start order, each with its start–end time, title and attendee count
   ("4 people", "1 person", "Just you"). The single-invitee and no-invitee
   events are greyed (with `detection.min_attendees` at its default of 2).
   The all-day event is not listed. Clicking a row does nothing yet (TUR-32).
4. Add a new event in Calendar.app, then click back into the meet-ai window.
   Expected: it appears without a restart (the pane re-reads on focus, and
   every `calendar.refresh_minutes`).

## 2. Revoke access: the denied state, not an empty list

1. In System Settings → Privacy & Security → Calendars, turn meet-ai off (or
   run `tccutil reset Calendar pro.saleschat.meetai` and press **Don't Allow**
   at the next prompt).
2. Click back into the meet-ai window.
3. Expected: Today shows "meet-ai can't read your calendar, so Today and
   meeting reminders are off." with **Open System Settings**, never "No
   meetings on your calendar today." The pane also shows on the "Reading
   your meetings folder…" and folder-error screens.
4. Click **Open System Settings**. Expected: System Settings opens at
   Privacy & Security → Calendars.
5. Turn access back on and press **Check again** (or click back into
   meet-ai). Expected: today's events come back without restarting the app.
6. With access still off, set `calendar.refresh_minutes` to 1 in
   `config.jsonc` and leave the window in front. Expected: the pane re-reads
   about once a minute (watch the log for "could not read a calendar"), so
   the configured interval holds even while every read fails.

## 3. No meetings today

1. With access allowed, on a day with no timed events (or with every event
   all-day or declined), open Meetings.
2. Expected: Today says "No meetings on your calendar today."
