# Manual checks: TUR-78 (Notifications settings, reminder lead time, Join)

These need the running, signed app on a real Mac with calendar and
notification permission and real meetings, so they were not run here: an
agent run cannot open the app, grant a permission, click a banner or join a
call.

The headless parts are covered by:

- `cargo test -p meet-ai --lib config::detection_section`: the new
  `detection.remind_before_minutes` key (0, 1, 2, 5, 10, 15 read as written;
  16, -1, 2.5, "5", null, true and a huge number logged and read as 1, with
  the other keys kept), the writer keeps comments and other sections, a disk
  round trip, and the schema's type, range and default match the code.
- `cargo test -p meet-ai --lib detection::reminder`: the TUR-30 rules on the
  fake clock, plus the lead time: 1/5/10 min fire at T-1/T-5/T-10 once; 0
  fires at the start and never later than `AT_START_GRACE`; a 10-minute lead
  sees an event just past the next refresh; a moved event re-arms with a
  longer lead; sleeping through the lead and the start stays quiet; a changed
  lead time or `min_attendees` applies on the next tick; the loop reads the
  settings on every tick and, with reminders switched off, reads no calendar.
- `cargo test -p meet-ai --lib detection::`: each switch turns its prompt off
  (`notify::allowed`), a switched-off process list / device source is never
  read and is read again once switched on, without a restart (`Switched`),
  the reminder's text ("“Standup” starts in 2 min, with 3 people invited.",
  rounded up; "is starting now"), the notification body mentions joining only
  with a link, only `http(s)` links can be joined, the reminded-events store,
  the test reminder's flag and text, the live config cache, and the card's
  settings ↔ config keys and range checks.
- `pnpm vitest run src/ui/NotificationSettings.test.tsx
  src/ui/DetectionPrompt.test.tsx`: each card control writes its key, the
  lead-time options, a hand-set value shown, a failed save, the OS-blocked
  line, the test button; the banner's button order (Join and record primary,
  Join, Record, Open brief, Dismiss), Join leaves the banner up, Record and
  Join and record send the event id, a test reminder never records or joins.

Not run here: `cargo check --target x86_64-pc-windows-msvc -p meet-ai` (ring's
C build needs a Windows toolchain; `just check-windows` skips src-tauri for
the same reason). No Windows CI job exists yet (TUR-36, #83, adds it), so
until #83 merges this is a manual check: on a Windows machine with MSVC, run
`cargo check -p meet-ai` and expect it to pass.

## Known limit: "macOS is blocking meet-ai's notifications"

The line uses tauri-plugin-notification's `permission_state`, as decided. On
desktop, version 2.4.0 returns `Granted` without asking the OS
(`src/desktop.rs`), so on macOS the line never shows today. It will once the
plugin (or a native `UNUserNotificationCenter` check behind
`platform::notifications_blocked`) reports the real state. Check 5 below
confirms the link itself.

## 1. Lead time 2 minutes, with Join

1. `just bundle-signed`, open meet-ai, grant calendar access. Have a meeting
   with 2+ attendees and a Zoom link starting in about 5 minutes.
2. Settings → Notifications → How early: "2 minutes before". Do not restart.
3. Expected: `config.jsonc` has `"remind_before_minutes": 2` under
   `detection`, comments kept. About 2 minutes before the start (±10 s) a
   notification "Record this meeting?" says "“<title>” starts in 2 min, with
   N people invited. Open meet-ai to join, record it or read the brief.",
   and the banner shows Join and record (tinted), Join, Record, Open brief,
   Dismiss.

## 2. Join

1. In the banner from 1, click Join. Expected: Zoom (or the browser for a
   Meet link) opens the meeting; the banner stays; nothing records.

## 3. Join and record

1. Repeat 1 with another meeting; click Join and record. Expected: the call
   opens and a recording starts, titled after the meeting even though it is
   still 2 minutes away (the event is pinned).

## 4. Switching a prompt off

1. Turn "Ask when a meeting app is running" off, then open Zoom. Expected: no
   prompt, and no restart needed. Turn it on again: the next Zoom launch
   asks.
2. Same for "Ask when my mic and speakers are both in use" (a call in a
   browser) and "Remind me before meetings" (no reminder for the next
   meeting; the lead-time select greys out).
3. Set "Only for meetings with at least" to 5 people. Expected: a 3-person
   meeting does not remind.

## 5. Test reminder and the OS link

1. Click "Send a test reminder". Expected: a notification and the banner for
   "Test meeting" starting in the configured minutes; every button closes it
   and nothing records or opens.
2. While recording, click it again. Expected: "Not while recording."
3. Turn meet-ai's notifications off in System Settings. Expected (once the
   plugin reports it, see the known limit): the "macOS is blocking…" line,
   whose Open System Settings button opens System Settings → Notifications.
