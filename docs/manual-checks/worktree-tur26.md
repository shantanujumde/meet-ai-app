# Manual checks: TUR-26 (EventKit calendar provider, permission and the denied path)

These need a signed build, a real calendar permission prompt and real accounts
in Calendar.app, so they were not run here: an agent run cannot answer a TCC
prompt, and macOS only keeps calendar permission for a signed bundle. The
headless parts are covered by `cargo test -p calendar`: the filter in
`crates/calendar/src/raw.rs` (all-day, declined, rooms, names, range, ids) and
the fake provider against `crates/calendar/tests/fixtures/events.json`,
including "denied is an error, not an empty day".

Nothing in the app calls the calendar yet. TUR-28 (next wave) adds the
`todays_meetings` command, the Today pane and the denied copy; it maps
`calendar::Error::PermissionDenied` to the error kind `calendar-denied`. Check
3 below can only be run once TUR-28 has landed. Checks 1 and 2 can be run now
through the ignored test.

## 1. The prompt, and reading real events

1. On a signed build, reset the grant: `tccutil reset Calendar pro.saleschat.meetai`.
2. From a terminal with the same signing setup, run the ignored test:
   `cargo test -p calendar -- --ignored reads_todays_events_from_calendar_app --nocapture`.
   (A bare `cargo test` binary is not the signed app; if macOS will not show a
   prompt for it, do this check through the app once TUR-28 has landed.)
3. Expected: macOS asks for calendar access, and the prompt reads "meet-ai
   reads your calendar to show today's meetings and name your recordings. It
   never changes your calendar." After **Allow**, the test passes and prints
   the number of today's events.

## 2. A Google account with no OAuth (SPEC §6 Phase 5a)

1. In System Settings → Internet Accounts, add a Google account and turn on
   Calendars. Make a timed meeting today in that Google calendar, with one
   invitee, plus an all-day event and a meeting you have declined.
2. Read today's events (check 1, step 2, or the Today pane after TUR-28).
3. Expected: the Google meeting appears with its title, times and attendee
   count; meet-ai never opens a browser or asks you to sign in. The all-day
   event and the declined meeting do not appear.
4. A repeating meeting shows once per day with a different id each day
   (`<id>@<unix seconds>`).

## 3. Deny access: the denied state, not an empty day (after TUR-28)

1. `tccutil reset Calendar pro.saleschat.meetai`, open the signed app and
   press **Don't Allow** at the calendar prompt. (Or turn meet-ai off in
   System Settings → Privacy & Security → Calendars.)
2. Expected: the app shows the "can't read your calendar" state with a way to
   System Settings, never "no meetings today".
3. Turn access back on in System Settings and reopen the view. Expected:
   today's events appear without restarting the app (each read makes a fresh
   `EKEventStore`, so a new grant is seen at once).

## Not checked here

- `cargo check --target x86_64-pc-windows-msvc -p calendar` passed on this
  machine (the target was already installed); the full `just check-windows`
  was not run, since `just check` is off limits for agent runs.
