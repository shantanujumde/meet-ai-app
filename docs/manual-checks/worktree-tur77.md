# Manual checks: TUR-77 (today's meetings in the menu bar, with Join and Record)

These need the running, signed app on a real Mac with a calendar permission
grant and real meetings (and a Windows and a Linux desktop for §6), so they
were not run here: an agent run cannot open the app, grant a permission,
click a menu or join a call.

The headless parts are covered by:

- `cargo test -p calendar join_url`: `extract_join_url` finds Zoom (URL
  field), Google Meet (location), Teams (HTML notes, `&amp;` unescaped), a
  Zoom link in notes after other links, Webex, Slack huddles; none for no
  link, landing pages or look-alike hosts; URL wins over location wins over
  notes; a bare `zoom.us/j/…` gets `https://`; trailing punctuation trimmed.
- `cargo test -p meet-ai --lib tray::menu_model`: the menu model's ordering
  (start, then id; ended events dropped), the cap of 5, greyed solo events,
  "Now", the empty day, "Calendar not connected", "Reading your calendar…",
  Join only with a link, times in the local zone, long and blank titles, and
  the countdown (within 60 min only, rounded up, solo events skipped).
- `cargo test -p meet-ai --lib recording::auto_title`: a pinned event names
  the recording even hours away; a pin is taken once and can be cleared.
- `cargo test -p meet-ai --lib config::app_section lifecycle`: the
  `app.menu_bar_countdown` key (default off, schema default matches, saving
  keeps the Dock switch) and the `NavigateTo` payload's shape.
- `pnpm vitest run src/ui/MenuBarCountdownSetting.test.tsx
  src/lib/routes.test.ts src/App.test.tsx`: the Settings toggle, and the
  window following the menu bar's Open brief / Calendar not connected (but
  not during setup).

Not run here: `cargo check --target x86_64-pc-windows-msvc -p meet-ai` (ring's
C build needs a Windows toolchain; `just check-windows` skips src-tauri for
the same reason). The Windows CI job checks it. `cargo check
--target x86_64-pc-windows-msvc -p calendar` passes.

## 1. The Today section (macOS)

1. `just bundle-signed`, open meet-ai, grant calendar access from the Today
   pane. Have today: one meeting now, three later with 2+ attendees (one with
   a Zoom link in its notes, one with a Google Meet link as its location, one
   with no link), and a solo focus block.
2. Click the menu-bar "m.". Expected: "Today" at the top, then the rest of
   today's events by start time, at most 5: `Now  <title>` for the current
   one, `14:30  <title>` for the others (24-hour, local time), the focus block
   greyed and not openable; then Open meet-ai, Start recording, Quit.
3. Hover a meeting with a link. Expected: Join, Record, Open brief. Hover the
   one without a link: Record and Open brief only.
4. Wait for a meeting to start (or end). Expected: within a minute it reads
   "Now" (or is gone), without reopening the app.
5. Add an event in Calendar.app, then open the window (the Today pane reads
   the calendar). Expected: the menu shows it at once; with the window closed
   it appears within `calendar.refresh_minutes`.

## 2. Join, Record, Open brief (macOS)

1. Join on the Zoom meeting. Expected: zoom.us opens (or the Zoom app). Join
   on the Meet one: meet.google.com opens in the browser.
2. Record on a meeting hours away. Expected: a recording starts (the window
   opens on it), and `meeting.md` has that meeting's title, attendees and
   `calendar_event_id`. Stop it. Start another with ⌘⇧R with no meeting near:
   it stays untitled (the pin did not linger).
3. Record while already recording. Expected: nothing changes; the recording
   keeps going (no stop).
4. Open brief with the window closed. Expected: the window comes back on
   `/brief?title=<title>` for that meeting.

## 3. Empty and not connected (macOS)

1. A day with nothing left. Expected: "No more meetings today", greyed.
2. Deny calendar access in System Settings › Privacy › Calendars, relaunch.
   Expected: "Calendar not connected"; clicking it opens the window on
   Settings. Also on a fresh install before the Today pane has asked: the
   menu never shows the permission prompt itself.
3. Set `"calendar": { "providers": [] }`. Expected: "Calendar not connected".

## 4. The countdown title (macOS)

1. Settings › Menu bar: "Show next meeting in the menu bar" is off. Expected:
   the icon alone.
2. Switch it on with a meeting 30 minutes away. Expected: at once, "Weekly
   sync in 30m" next to the icon, counting down each minute; nothing for a
   meeting more than 60 minutes away or a solo block; gone once it starts.
3. Switch it off. Expected: the text goes at once. `config.jsonc` has
   `"menu_bar_countdown": false`, and `show_in_dock_when_closed` is
   unchanged.

## 5. Redraws (macOS)

Leave the app running for an hour with `RUST_LOG` at debug. Expected: the
menu is rebuilt at most once a minute, and only when its text changed
(Activity Monitor: meet-ai near 0 % CPU while idle).

## 6. Windows and Linux trays

Windows: right-click the tray icon. Expected: the same Today section and
submenus, Join opens the link, no text next to the icon (the countdown is
macOS only). Linux (AppIndicator): the same menu from a click; no countdown
label.

## Follow-ups

- The time column is always 24-hour (`%H:%M`): there is no Rust helper for
  the system's 12/24-hour preference yet. A 12-hour-clock user sees `14:30`.
- "Calendar not connected" opens Settings, as the ticket says. Settings has
  no calendar section yet (the Google/Microsoft sign-in commands from TUR-44
  have no screen), and the macOS permission prompt is asked by the Today
  pane. Once a calendar section exists, this should land on it.
- Record is not greyed while a recording is running; clicking it does
  nothing then.
