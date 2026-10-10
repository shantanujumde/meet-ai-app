# TUR-169: Record prompts and reminders that fail or vanish without telling the user

These need the signed app running, real meeting apps, a real calendar and
real permissions, which the worker cannot do. The rules are unit-tested with
fakes: the reminder replay in `src-tauri/src/detection/reminder/tests.rs`, the
per-start pinned event in `src-tauri/src/recording/auto_title/tests.rs`, the
"already recording" refusal in `src-tauri/src/folder_move.rs`, the popup's
refusal decision in `src-tauri/src/detection/popup/tests.rs`, and the
resident-app rule in `crates/detect/src/detector.rs`.

## A failed Record from the prompt card says why

- Run: a signed build on Windows or Linux (and macOS), with the main window
  hidden in the tray. Deny the microphone (or start a meetings-folder move),
  then press Record on a detection prompt card.
- Expected: the card stays up showing the reason in red, a notification
  "meet-ai did not start recording" with the same reason appears, and the
  card hides about 8 s later.
- Why skipped: needs the running app, the popup window and an OS permission.

## A successful Record from the card hides it

- Run: press Record on a prompt card with permissions granted.
- Expected: the card stays until the recording starts, then fades out and
  the window hides. No notification.
- Why skipped: needs the running app and real audio.

## A reminder that fires while recording is asked after

- Run: two calendar events with 2+ attendees, 10:00 to 11:00 and 11:00 to
  11:30 (any times a few minutes apart work). Record the first one past the
  second's reminder time, then stop before the second ends.
- Expected: no reminder while recording; within 10 s of stopping, the second
  meeting's reminder card shows ("is starting now"), with its Join link and
  Record naming the recording after it. Stopping after the second ended asks
  nothing.
- Why skipped: needs a real calendar and a recording.

## Record for meeting B while A records

- Run: while recording, click Record on another meeting in the menu bar's
  Today list, and on a reminder card.
- Expected: a notification "meet-ai did not start recording: A recording is
  already running. Stop it first to record this meeting." The running
  recording keeps going and keeps its own title.
- Why skipped: needs the running app and a recording.

## Two Record clicks at once keep their own titles

- Run: idle, click Record on meeting A in the menu bar's Today and, within a
  second, Record on meeting B's reminder card.
- Expected: one recording, titled after the click that won; the other click
  gets the "already running" notification. The title never comes from the
  other meeting.
- Why skipped: needs the running app and real timing.

## A record shortcut another app owns

- Run: have another app register ⌘⇧R (macOS) or Ctrl+Alt+R (Windows, Linux)
  first, then launch meet-ai.
- Expected: the titlebar shows the shortcut struck through, its tooltip says
  it is unavailable because another app is using it, and the empty meetings
  list says "Click Start recording" instead of "Press ⌘⇧R". With the shortcut
  free, both advertise it as before.
- Why skipped: needs the running app and a second app owning the shortcut.
  Verify it: on Linux under Wayland it is not confirmed whether the
  registration call fails or succeeds without effect; if it succeeds, the
  window still advertises the shortcut there (README's
  `meet-ai --toggle-recording` binding is the way on Wayland).
- Not changed: onboarding's folder step (`src/routes/onboarding/FolderStep.tsx`)
  still names the shortcut; that file belongs to TUR-165 in this run.

## Zoom, Teams and Webex open at login do not ask

- Run: on a system where the apps on the mic cannot be listed (the TUR-27
  fallback; on macOS 26 the list can be read, so this is the Windows/Linux or
  older-OS path), launch Teams (and Zoom, Webex) at login with no call.
- Expected: no "Microsoft Teams is open" prompt. Starting a call in Teams
  later (mic and speakers in use, with `detection.audio_activity` on) asks
  once, naming Teams; so does a calendar reminder while Teams is open.
- Why skipped: needs real meeting apps, a real call and the OS process list.
  With `detection.audio_activity` and `detection.calendar` both off, the
  fallback never asks about an open meeting app at all; that is by design.
