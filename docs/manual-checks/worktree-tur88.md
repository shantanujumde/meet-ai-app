# Manual checks: TUR-88 (calendar reads: cloud-only reminders, network blips, Microsoft 1:1s and account names, the sign-in callback)

These need the running, signed app on a real Mac with a real Google or
Microsoft account, so they were not run here: an agent run cannot open the
app, sign in, answer the Calendar prompt, or turn the Wi-Fi off.

The headless parts are covered by:

- `cargo test -p meet-ai --lib calendar::readable`: cloud-only providers are
  read whatever Calendar.app's prompt says; with `["eventkit","google"]` and
  the prompt unanswered, background reads take Google alone and the Today
  pane still asks Calendar.app; Calendar.app alone and unanswered leaves
  nothing to read in the background.
- `cargo test -p meet-ai --lib detection::reminder`:
  `a_mac_with_only_cloud_calendars_gets_its_reminders` (Google and
  Microsoft, Calendar.app never answered) and
  `a_sign_in_next_to_an_unanswered_calendar_app_still_reminds` (Calendar.app
  is never read before its prompt is answered).
- `cargo test -p meet-ai --lib tray::today`: an `Unreachable` read keeps
  today's list, never yesterday's; `PermissionDenied`, `SignInExpired`, or a
  blip with nothing read yet is "Calendar not connected".
- `cargo test -p calendar --test microsoft`:
  `the_organizer_counts_once_so_a_one_on_one_is_not_solo` (invited 1:1,
  organized 1:1, organizer already listed, own focus block).
- `cargo test -p calendar --lib oauth`: Microsoft asks for `profile`;
  `preferred_username` falls back to `email`; a keystore that cannot be read
  is `Unreachable`, not signed out or expired; a rejected client id is tried
  again once `config.jsonc` names another.
- `cargo test -p meet-ai --lib calendar::loopback calendar::signin`: a ~3 KB
  Microsoft redirect in one write, a 4.4 KB one in seven pieces, an idle
  pre-opened connection, an oversized head (431), wrong paths and methods
  (404, listener keeps waiting), a wrong `state` (sign-in fails, nothing
  stored), the first callback closing the port, and the port closed after a
  timeout.

## 1. A Mac with only a cloud calendar gets reminders (macOS)

1. `just bundle-signed`. In the meetings folder's `.app/config.jsonc` set
   `calendar.providers` to `["google"]` (or pick it in Settings → Calendar),
   and check in System Settings → Privacy & Security → Calendars that
   meet-ai has never been asked (reset is `tccutil reset Calendar
   pro.saleschat.meetai`, run by a person, not an agent).
2. Sign in to Google in Settings → Calendar.
3. In Google Calendar make an event starting in 3 minutes with one other
   attendee.

Expected: "meeting in 1 minute" arrives one minute before, and the menu bar
lists the event with Join/Record and a countdown. No Calendar prompt is shown
at any point. Repeat with `["microsoft"]` and a Microsoft account.

## 2. Calendar.app unanswered next to a sign-in (macOS)

1. As §1 but `calendar.providers = ["eventkit","google"]`, Calendar prompt
   never answered, and do not open the main window.

Expected: the menu bar lists today's Google events (not "Calendar not
connected"), and a Google event still gets its reminder. Opening the Today
pane then shows the Calendar prompt; after granting, Calendar.app's events
join the list on the next read.

## 3. A network blip keeps the menu (macOS)

1. Google or Microsoft only, signed in, at least one event later today.
2. Turn the Wi-Fi off, and wait past `calendar.refresh_minutes` (set it to 1
   for the test).

Expected: the menu bar still lists today's events with Join/Record and the
countdown; it does not say "Calendar not connected". Turn the Wi-Fi back on:
the next read updates normally. Quit and relaunch with the Wi-Fi off: the
menu says "Calendar not connected" (nothing read yet), which is expected.

## 4. Microsoft 1:1s are not solo (real account)

1. Microsoft connected. Make a Teams 1:1 that you organize with one
   colleague, and have a colleague send you a 1:1 invite.

Expected: both show in the Today pane without the solo/greyed look, both
have Join/Record in the menu bar, and both get a reminder. A focus block with
no attendees stays greyed. This is the real-token check the fixture cannot
give: whether Graph really leaves the organizer out of `attendees` (the code
does not double-count either way).

## 5. The Microsoft account name shows (real account)

1. Sign out of Microsoft if signed in, then sign in again (the new `profile`
   scope only reaches a new consent), with a work account and with a personal
   account.

Expected: the Settings card names each account (an address like
`ada@contoso.com`). The consent screen may now also list "View your basic
profile".

## 6. Microsoft work-account sign-in completes (real account)

1. Sign in with a Microsoft work account in Chrome, then in Safari.

Expected: the browser shows "Signed in. You can close this tab and go back
to meet-ai." and the app shows the account within a second or two; it never
spins for five minutes. The redirect is now read by our own listener
(`src-tauri/src/calendar/loopback.rs`), so the browser makes one request and
no follow-up `fetch`.

## 7. A locked keychain (macOS)

1. Signed in to Google, quit meet-ai, lock the login keychain (Keychain
   Access → File → Lock), relaunch.

Expected: the Today pane says the calendar could not be reached, not "sign
in again", and the menu bar keeps whatever it had. Unlocking the keychain:
the next read works with no new sign-in. (The Settings card's own state for
this case is unchanged by TUR-88: `CalendarAuth::accounts` still reports an
unreadable keystore as signed out.)
