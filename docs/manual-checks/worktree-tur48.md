# Manual checks: TUR-48 (Google calendar provider, events.list with singleEvents=true)

These need a real Google account with meetings on it, a browser, the real OS
keystore and the running app on each OS, so they were not run here: an agent
run has no account, cannot click through a consent screen, must not write to
the real keychain, and must not start the app. There is no UI for sign-in yet
(TUR-49), so sign in from the webview's devtools console as in
`docs/manual-checks/worktree-tur44.md` §1.

The headless parts are covered by `cargo test -p calendar --test google_provider`
(fixture JSON in `crates/calendar/tests/fixtures/google/`, a fake HTTP layer
answering for both Google's token endpoint and the Calendar API, and the real
`CalendarAuth` over an in-memory store):

- the request: `calendars/primary/events`, `timeMin`/`timeMax` in UTC,
  `singleEvents=true`, `orderBy=startTime`, `timeZone=UTC`, `maxResults=250`,
  `fields=…`, the bearer token, and `pageToken` on the second page;
- skips: cancelled, declined by the `self: true` attendee, all-day
  (`start.date` only); kept: tentative, declined by someone else; rooms
  (`resource: true`) neither counted nor named; `iCalUID` kept on the event;
- recurring: each occurrence its own id, a cancelled and a declined
  occurrence dropped, a moved occurrence at its new time;
- tokens: reused between reads; a 401 refreshes once and retries; a second
  401, or a refused (`invalid_grant`) refresh, or no sign-in at all is
  `SignInExpired { provider: "Google" }`;
- errors: a 403 names its status and reason but never Google's message (it
  can name the account); no network and an unreadable page are
  `Unreachable`, never an empty day.

`cargo test -p meet-ai --lib config::calendar_section` covers `google` now
being an available provider.

Before any check: set up the Google client as in SETUP.md, "Calendar sign-in
(optional)", with the consent screen **In production**, and put `"google"` in
`calendar.providers` in `~/Meetings/.app/config.jsonc`.

## 0. The fixture tests on Linux and Windows

Why skipped: the ticket's Done-when asks for the fixture tests to pass on all
three CI OSes, but `.github/workflows/check.yml` runs `cargo test` on
`macos-26` only. Ubuntu runs fmt and `just check-windows`, a `cargo check` of
the library that does not build or run the tests. That stays true until
TUR-36 (PR #83) adds the Windows and Linux jobs, and this run has no Linux or
Windows machine.

1. On a Linux machine (Ubuntu 24.04, `libsecret-1-dev` installed for
   `keyring`) and on Windows 11 (MSVC toolchain), check out this branch and run
   `cargo test -p calendar --test google_provider`.
2. Expected: 15 passed, 0 failed (recurring, cancelled, declined, all-day,
   paging, 401 renewal, revoked token, 403, offline). Nothing reaches the
   network or the OS keystore: the tests use a fake HTTP layer and
   `MemoryStore`.
3. Once TUR-36 is merged, a green `rust` job on ubuntu and windows on this PR
   (or on main after it) replaces this check.

## 1. Today's meetings from a real Google account, on macOS, Windows and Linux

1. In the Google account, have for today: a one-off meeting with two or more
   guests, an occurrence of a repeating meeting, a meeting you declined, a
   cancelled meeting, an all-day event, and a meeting with a room booked.
2. Start a dev build and sign in to Google (`calendar_sign_in`). On the first
   sign-in, expect the one-time "Google hasn't verified this app" screen:
   **Advanced → Go to … (unsafe)**.
3. Open the Today pane (or run
   `await window.__TAURI_INTERNALS__.invoke("todays_meetings")`).
4. Expected: the one-off and the repeating occurrence, at the right local
   times, with the room not counted in `attendees`. Not shown: the declined,
   the cancelled and the all-day event.
5. On a Mac whose Calendar app also has this Google account, with
   `["eventkit", "google"]`, the same meeting shows twice for now: dedupe by
   `ical_uid` is TUR-49's job. Note it, it is expected.

## 2. Still there a day later (refresh works)

1. Quit the app fully. The next day (and, to catch a consent screen left in
   *Testing*, again after 7 days), start it and open the Today pane.
2. Expected: that day's meetings, with no browser opening and no sign-in
   error. The log has no "rejected" warning.

## 3. Revoked access

1. Revoke the app at myaccount.google.com → Security → Third-party access.
2. Wait for the next refresh of the Today pane (or restart).
3. Expected: `todays_meetings` fails with kind `calendar-sign-in-expired`
   ("your Google sign-in has expired and needs renewing") when Google is the
   only provider; with EventKit also listed, EventKit's events still show and
   the log has one "could not read a calendar" warning for Google.

## 4. Calendar API not enabled

1. In the Cloud project, disable the Google Calendar API, then reload the
   Today pane.
2. Expected: kind `calendar-unreachable`, the message naming `403`,
   `PERMISSION_DENIED` and `accessNotConfigured`, and not the account's email.

## Notes for reviewers

- Only the primary calendar is read, as the ticket says; other calendars the
  account subscribes to are not.
- Reminders (`detection/reminder.rs`) only read calendars once EventKit's
  permission has been answered. On a Mac that never opened the Today pane,
  a Google-only setup gets no reminders until it does. Off macOS that check
  is always true. Not changed here.
- `src-tauri/config.schema.json` still describes `calendar` as "Not read by
  the app yet". Not changed: that file is shared and this wave only adds
  lines to it; it needs a follow-up once TUR-47 and TUR-48 are both in.
