# Manual checks: TUR-174

Calendar audit fixes: Today flags a calendar that failed while another
answered, providers are read in parallel, merge never joins different UIDs or
untitled events, a waiting sign-in has Cancel, and the sign-in listener serves
at most 16 connections at once. The decision logic (`calendar/read.rs`,
`merge.rs`, `calendar/cancel.rs`, the loopback cap) is unit-tested headless
with fakes and real 127.0.0.1 sockets. Nothing below was run here: each needs
the running app, a real Google or Microsoft account, or the macOS prompt.

## Run by hand

1. Calendar app on and a Google sign-in connected; revoke meet-ai's access at
   myaccount.google.com (or wait for the refresh token to lapse), then open
   Today.
   Expect: the Calendar app's meetings still listed, and above them "Google
   needs you to sign in again, so its meetings are missing here." with
   "Open Calendar settings", which lands on Settings scrolled to Calendars.
   Why skipped: needs a real Google account and the running app.
2. Same, with the network off (Wi-Fi off) and the Calendar app answering.
   Expect: a "could not be read" line for each cloud calendar, the Calendar
   app's meetings still shown.
   Why skipped: needs the running app and real sign-ins.
3. Fresh Mac (calendar prompt never answered), Calendar app plus a Google
   sign-in. Open Today and leave the macOS prompt up for a minute.
   Expect: nothing shows until the prompt is answered (Today waits for the
   whole read), but the Google read is no longer queued behind it: answering
   the prompt shows both at once, with no extra Google wait. Verify it.
   Why skipped: needs the macOS prompt (tccutil reset is off limits here).
4. Settings → Calendars → Sign in with Google, then close the browser tab.
   Expect: the row says "Waiting for the browser…" with a Cancel button next
   to it; Cancel returns the row to "Sign in with Google" at once, with no
   error line, and nothing is stored. Same from the Today pane's
   "Sign in with Google or Microsoft" state and onboarding's calendar step.
   Why skipped: needs the running app and a browser.
5. Start a sign-in, do nothing for 5 minutes.
   Expect: unchanged from before: the row shows the cancelled-or-timed-out
   error line.
   Why skipped: needs the running app.
6. Two meetings both titled "Interview" at 10:00, each its own invite (own
   UID), one in Calendar.app and one only in Google.
   Expect: both listed on Today. Two untitled holds at the same time: both
   listed.
   Why skipped: needs real calendars.

## Known

- A Cancel pressed in the instant between clicking Sign in and the listener
  starting (before the browser opens) finds nothing to cancel; the sign-in
  then runs as usual and can be cancelled again.
- The Windows and Linux builds are checked by the `rust (windows | linux)` CI
  job; `cargo check --target x86_64-pc-windows-msvc -p calendar` passed here.
