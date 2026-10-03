# Manual checks: TUR-49 (calendar settings: Calendar app + Google/Microsoft sign-in)

These need the running, signed app, a real Google and Microsoft account with
meetings, a browser for the consent screen, the real OS keystore and the macOS
Calendar permission, so they were not run here: an agent run must not start
the app, cannot click through a consent screen, must not write to the real
keychain, and has no Windows or Linux machine.

What is covered headless:

- `cargo test -p calendar merge`: `merge_events` — one meeting in two sources
  shown once with the first source's id and the richer copy's attendees; the
  same title at different times kept apart (including 2 minutes apart); two
  titles at the same time kept apart; a shared iCalUID matched even when the
  title differs; a shared UID on another day (a repeating meeting) kept apart;
  a UID only one copy has kept.
- `cargo test -p meet-ai --lib calendar`: `CalendarState` reads the merged
  list (`a_meeting_in_two_calendars_is_read_once_with_the_first_providers_id`);
  `config::calendar_section` — `[]` default and the `"eventkit"` error off
  macOS, `["eventkit"]` on macOS, the providers writer keeping comments and
  client ids; `calendar::sources` — the card's wire shape and per-OS rows.
- `pnpm vitest run src/ui/calendar src/ui/TodayPane.test.tsx
  src/routes/onboarding/CalendarStep.test.tsx`: the Settings card on macOS
  and off it, signed in / expired / signed out / no client id; the Today
  pane's "Sign in with Google or Microsoft" state; the onboarding step only
  off macOS, with Skip.

Before the sign-in checks: set up the Google and Microsoft clients as in
SETUP.md, "Calendar sign-in (optional)".

## 1. Calendar.app + Google with the same meeting → one entry (macOS)

Why skipped: needs Calendar access, a Google sign-in in a browser and the
signed app.

1. Add a Google account to Calendar.app (System Settings → Internet
   Accounts) with a meeting today that has at least 2 attendees.
2. `just bundle-signed`, open meet-ai, Settings → Calendars. The Calendar app
   switch is on. Choose **Sign in with Google** and finish in the browser.
3. Expected: the Google row shows the account's email and **Disconnect**;
   `~/Meetings/.app/config.jsonc` now has `"providers": ["eventkit", "google"]`
   with any comments kept.
4. Open Meetings → Today. Expected: the meeting is listed **once**. Its
   reminder a minute before fires once, not twice.
5. Turn the Calendar app switch off. Expected: `eventkit` leaves
   `calendar.providers` and Today still shows the meeting once, from Google.

## 2. Disconnect and the expired state (macOS, Windows, Linux)

1. Settings → Calendars → Google → **Disconnect**. Expected: the row shows
   **Sign in with Google**; `google` is gone from `calendar.providers`; the
   keystore entry `pro.saleschat.meetai` / `calendar-google` is deleted.
2. Sign in again, then revoke the app at https://myaccount.google.com/permissions
   and relaunch. Expected: the row reads "<email> · Sign-in expired" with
   **Sign in again**, and Today shows the sign-in-expired error.

## 3. Windows and Linux: no Calendar app

Why skipped: no Windows or Linux machine; CI runs only the headless tests.

1. Fresh install, no `config.jsonc`. Run the onboarding. Expected: a
   "See today's meetings" step after the folder step, with both sign-in
   buttons and **Skip**. Skip it.
2. Today shows "Sign in with Google or Microsoft to see today's meetings"
   and both buttons, never "No meetings on your calendar today."
3. Settings → Calendars shows only Google and Microsoft, no Calendar app row.
4. Put `"providers": ["eventkit"]` in `config.jsonc`. Expected: the log says
   `calendar.providers "eventkit" is the macOS Calendar app, which this system
   does not have; …` and the app runs with the defaults (`[]`).
5. Remove `calendar.google.client_id`. Expected: the Google row reads "Add
   calendar.google.client_id to config.jsonc (see SETUP.md)" with no button.

## 4. Linux with no Secret Service

1. Sign in on a Linux session without a Secret Service. Expected: the row
   reads "<email> · Signed in until meet-ai quits: this system has no
   keystore to remember it".

## Notes

- `Event.join_url` (TUR-77, PR #88) is not on main yet. When it lands,
  `richness` in `crates/calendar/src/merge.rs` should count it, so the copy
  with a join link wins a merge.
