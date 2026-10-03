# Manual checks: TUR-47 (Microsoft calendar provider: Graph calendarView, Windows → IANA time zones)

These need a real Microsoft account (personal and work), a signed-in app
(TUR-44's sign-in, which needs a browser and the real OS keystore) and the
running app on each OS, so they were not run here: an agent run has no
account, cannot click through a consent screen, must not write to the real
keychain and makes no calls to Microsoft. There is no UI for choosing
providers yet (TUR-49); the Today pane already shows what the providers read.

The headless parts are covered by:

- `cargo test -p calendar --test microsoft`: fixture JSON in
  `crates/calendar/tests/fixtures/microsoft/` through a fake `HttpClient`.
  Two pages joined through `@odata.nextLink`; cancelled, declined and all-day
  events skipped; the occurrences of a repeating meeting each their own event
  with their own id; a "Pacific Standard Time" event read at UTC-7; rooms not
  counted; iCalUID kept; the request carries `Bearer`, `Prefer:
  outlook.timezone="UTC"`, the range, `$select` and `$top=50`; a 401 renews the
  token once (also through the real `CalendarAuth`, which refreshes instead of
  handing back its cached token); a second 401, or a rejected refresh, is
  `SignInExpired { provider: "Microsoft" }`; 429, 503, a dropped connection, a
  non-JSON reply and a failing second page are `Unreachable`, never an empty
  day; a `nextLink` off graph.microsoft.com is refused before the token is
  sent.
- `cargo test -p calendar --lib`: `windows_tz` (the copied CLDR table and its
  chrono-tz test), `microsoft::graph_time` (UTC, Windows and IANA zone names,
  DST, a wall time in a DST gap), `oauth` (`reject_access_token`,
  `has_sign_in`).
- `cargo test -p meet-ai --lib config::calendar_section`: `microsoft` in
  `calendar.providers` is now read; `google` is still skipped.
- `cargo check --target x86_64-pc-windows-msvc -p calendar`: passes (it
  compiles for Windows; it runs no tests).

Where the fixture tests run: today only on macOS CI (`rust (macos-26)` in
`.github/workflows/check.yml`). The ubuntu job runs only `cargo fmt` and the
Windows cross-check, not `cargo test`. They run on Windows and Linux CI once
TUR-36 (PR #83) adds those test jobs; until then, check 0 below is manual.

Before any check: set `calendar.microsoft.client_id` in
`~/Meetings/.app/config.jsonc` (SETUP.md, "Calendar sign-in (optional)"), and
sign in as in `worktree-tur44.md` §1 step 4.

## 0. Fixture tests pass on Windows and Linux CI (until TUR-36 lands)

1. After TUR-36 (PR #83) is merged, rebase this branch, or open any PR on
   top of both, and look at the new Windows and Linux test jobs. Without CI,
   run `cargo test -p calendar` on a Windows and a Linux machine instead.
2. Expected: `tests/microsoft.rs` (13 tests) and the `windows_tz` and
   `microsoft` unit tests pass on both. They use no network and no OS API,
   so they should behave the same as on macOS.
3. Why skipped: those CI jobs do not exist on main yet, and this run has no
   Windows or Linux machine.

## 1. Today's meetings from a Microsoft account, on macOS, Windows and Linux

1. In `config.jsonc` set `"calendar": { "providers": ["microsoft"] }` (on
   macOS, also try `["eventkit", "microsoft"]`).
2. In Outlook (web or desktop) make, for today: a one-off meeting with two
   other people and a room, a daily repeating meeting, an all-day event, a
   meeting you declined, and one the organiser cancelled. Make one of them in
   a different time zone than the computer's (Outlook → time zone picker).
3. Open the app's Today pane (it re-reads every `calendar.refresh_minutes`).
4. Expected: the one-off and today's occurrence of the repeating meeting
   appear at the right local times; the all-day, declined and cancelled ones
   do not; the one-off counts 2 attendees (the room is not counted); the
   other-time-zone meeting is at the right local time.
5. Repeat with a work (Entra ID) account and with a personal (outlook.com)
   account.
6. With both `eventkit` and `microsoft` on a Mac whose Calendar.app also has
   the same Microsoft account, expected for now: the meeting shows twice.
   Dedup on `ical_uid` is TUR-49.

## 2. Expired or revoked sign-in

1. Revoke the app (account.live.com/consent/Manage or myapps.microsoft.com),
   then wait for the Today pane to re-read (or restart the app).
2. Expected: the Today pane shows its sign-in-expired state (error kind
   `calendar-sign-in-expired`), not an empty day.
3. Sign out (`calendar_sign_out` with `provider: "microsoft"`). Expected: the
   Microsoft calendar is left out quietly, and with no other provider the
   pane shows an empty day.

## 3. Offline

1. Turn Wi-Fi off and let the pane re-read.
2. Expected: `calendar-unreachable`, not an empty day; when another provider
   (EventKit) answers, its events still show.

## Notes for reviewers

- Meeting reminders (TUR-30, `src-tauri/src/detection/reminder.rs`,
  `AppCalendar`) on macOS wait until EventKit access has been answered, even
  when `calendar.providers` is only `["microsoft"]`; so a Mac reading only
  Microsoft gets no reminders until the Calendar prompt was answered once.
  Off macOS `access_answered()` is always true, so Windows and Linux are not
  affected. Not changed here (not this ticket's file); worth a follow-up.
- `config.schema.json` still says the `calendar` section is "Not read by the
  app yet". Left alone (shared file, only-add-lines rule); TUR-49 can reword it.
- Graph throttling (429) is not retried within a read; the next refresh
  retries.
