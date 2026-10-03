# Manual checks: TUR-76 (keep running in the menu bar when the main window is closed)

These need the running, signed app on a real Mac (and a Windows and a Linux
desktop for §6), a real recording, the Dock and the menu bar, so they were not
run here: an agent run cannot open the app, click, or record.

The headless parts are covered by:

- `cargo test -p meet-ai --lib lifecycle`: close → hide with a tray, close →
  quit without one; quit while starting/recording/stopping asks first, idle
  quits; "Stop and quit" lets the held exit through; an updater restart is
  never held; the Dock icon follows `app.show_in_dock_when_closed`; the
  one-off notice is shown once, survives a relaunch, keeps other keys in
  `.app/state.json`, and a corrupt file reads as "not shown".
- `cargo test -p meet-ai --lib config::app_section`: the `app` section reads,
  defaults, rejects a bad value, writes back keeping comments, and matches
  `config.schema.json`.
- `pnpm vitest run src/ui/QuitPrompt.test.tsx src/ui/DockSetting.test.tsx`:
  the dialog (Cancel focused, Escape, Tab trapped, Stop and quit calls
  `confirm_quit`, closes when the recording ends elsewhere) and the toggle.

Not run here: `cargo check --target x86_64-pc-windows-msvc -p meet-ai` (ring's
C build needs a Windows toolchain; `just check-windows` skips src-tauri for
the same reason). No Windows CI job exists yet (TUR-36, #83, adds it), so
until #83 merges this is a manual check: on a Windows machine with MSVC, run
`cargo check -p meet-ai` and expect it to pass.

## 1. Close hides; the app keeps running (macOS)

1. Build signed (`just bundle-signed`), open meet-ai, finish onboarding.
2. Click the red close button. Expected: the window goes, the menu-bar "m."
   stays, the Dock icon goes away, and a notification says "meet-ai is still
   running in the menu bar. Quit from the menu bar icon or with ⌘Q."
3. Open it again (§3) and close it with ⌘W. Expected: same, but no
   notification this time. Quit and relaunch, close again: still none.
   `~/Meetings/.app/state.json` has `"stillRunningNoticeShown": true`.
4. With the window closed, wait for a calendar reminder (an event starting
   in two minutes with 2+ attendees). Expected: the notification fires, and
   the window comes back (Dock icon too) without taking focus, the "Record
   this meeting?" banner on it. Same for opening Zoom (detection).
5. Start a recording with ⌘⇧R, close the window, talk for a minute, stop
   with ⌘⇧R. Expected: the meeting is complete, nothing cut at the close.

## 2. Show in Dock setting

1. Settings → Menu bar → "Show in Dock when the window is closed": on.
   `config.jsonc` gains `"app": { "show_in_dock_when_closed": true }`.
2. Close the window. Expected: the Dock icon stays. Click it: the window
   comes back (`RunEvent::Reopen`).
3. Turn it off again: closing removes the Dock icon.

## 3. Reopen

With the window closed, each of these brings it back in front with its Dock
icon: menu-bar "Open meet-ai"; launching meet-ai again from Finder or
Spotlight; the Dock icon (with §2 on). Also click a reminder notification:
macOS may or may not send `Reopen` for it; note which.

## 4. ⌘Q while recording asks first

1. Start a recording. Press ⌘Q (window open). Expected: an in-window dialog
   "Stop recording and quit?" with Cancel focused and a red "Stop and quit".
2. Cancel: the recording goes on. Escape: same.
3. Close the window, then menu-bar "Quit meet-ai". Expected: the window
   comes back with the same dialog.
4. "Stop and quit": the app quits; the meeting is finished, not labelled
   Interrupted, and its WAVs play to the end.
5. Not recording: ⌘Q and the menu-bar Quit quit at once, no dialog.

## 5. Logout / shutdown does not ask

Start a recording, log out of macOS. Expected: logout is not held by a
dialog; after logging back in the meeting is finished (not Interrupted).
Dock right-click → Quit takes the same path (AppKit `terminate:`) and does
not ask either; note that.

## 6. Windows and Linux

1. Windows: closing the window hides it to the tray; the tray's "Open
   meet-ai" brings it back; tray Quit while recording shows the dialog.
2. Linux with a tray (KDE, or GNOME with the AppIndicator extension): same.
3. Linux without libayatana-appindicator: closing quits (asking first while
   recording). GNOME *with* the library but without the extension builds an
   invisible tray, so closing hides and the user must relaunch to get the
   window back (single-instance focuses it); see `platform::has_tray`.
