# worktree-tur90-part2 manual checks (TUR-90 part 2, calendar cleanup)

The shared token source and paging loop are covered by the existing Google
and Microsoft fixture tests (unchanged, same counts). What needs a real
account and the running app:

1. One read per Today refresh.
   Run: a signed build signed in to Google and Microsoft, `RUST_LOG=calendar=debug,meet_ai=debug`,
   open the Today pane, then watch a proxy (or the debug log) through one refresh.
   Expected: one `calendars/primary/events` and one `/me/calendarView` series
   per refresh; the menu bar's Today updates at the same moment with the same
   meetings.
   Skipped: needs real accounts, a signed build and the running app.

2. Keep the last good read on a blip (TUR-88, unchanged).
   Run: with the pane open, turn Wi-Fi off past `calendar.refresh_minutes`.
   Expected: the menu bar keeps today's meetings; it shows "not connected"
   only after midnight or with nothing read yet.
   Skipped: same reason.

3. Sign-in and reads share one HTTP client.
   Run: sign in to Google from Settings, then open Today.
   Expected: sign-in and the read both work (one reqwest client, built on the
   first request).
   Skipped: needs a browser sign-in.
