# Manual checks: tur59

TUR-59: on Windows and Linux, a detection or reminder prompt ("Record this
meeting?") shows in a small popup window of meet-ai's own
(`src-tauri/src/detection/popup/`, `src/ui/PromptPopup.tsx`) with **Record**
and **Dismiss** (plus **Join and record** / **Join** for a reminder with a
meeting link). It hides itself after 20 s (`popup::AUTO_HIDE`). macOS is
unchanged: `popup/platform/mod.rs` keeps `USE_POPUP = false` there.

Tested headless on a Mac: the prompt state (replace, timeout, dismiss records
nothing, Record starts exactly once, stale clicks do nothing, test reminders
never record), the top-right placement maths, and the React popup with a
faked IPC. None of the window code ran: the macOS build never makes the popup,
and nothing here was run on Windows or Linux.

## Run by hand

Use a build of this branch on each of: Windows 11, Linux X11 (e.g. Ubuntu
24.04 on Xorg), Linux GNOME Wayland (Ubuntu 24.04 default session).

1. Settings → Notifications → **Send a test reminder**.
   Expect: a ~360×132 borderless popup top-right of the primary monitor's
   work area (below a top panel, left of nothing), with "Record this
   meeting?", the test sentence, Join and record / Join / Record / Dismiss.
   It does not take focus from the window you were typing in, has no
   taskbar entry, and stays above other windows. Any button closes it;
   nothing records. Left alone, it disappears after about 20 s.
   Why skipped: needs a real Windows / Linux desktop and the running app.
2. Connect a calendar with an event starting in a few minutes that has a
   Zoom/Meet/Teams link; wait for the reminder.
   Expect: the popup names the event and when it starts. **Record** starts a
   recording titled after the event (same auto-title as the macOS banner);
   **Join** opens the link and the popup stays; **Join and record** does both;
   **Dismiss** closes it and nothing records.
   Why skipped: needs a real calendar account and the running app.
3. Open Zoom (or Teams) with meet-ai idle.
   Expect: the popup says "Zoom is open." with Record / Dismiss. Record
   starts one recording; Dismiss closes it; waiting 20 s closes it; in no
   case does a recording start without a click.
   Why skipped: needs a real meeting app on Windows / Linux.
4. With the popup up, start a recording another way (tray or the shortcut).
   Expect: the popup closes.
   Why skipped: needs the running app on Windows / Linux.
5. Trigger two prompts in a row (test reminder, then open Zoom).
   Expect: one popup, showing the newer prompt; the old one's 20 s timer does
   not close the new one early.
   Why skipped: needs the running app on Windows / Linux.
6. Fallback: on Linux, run with a broken webview environment if you can (or
   check the log for `could not show the prompt popup`).
   Expect: the old system notification (no buttons) appears instead, and the
   main window's banner still asks.
   Why skipped: no way to make the window fail headless.

## Known limits

- **Wayland:** a normal client cannot place its own window or keep it on top
  without the layer-shell protocol. gtk-layer-shell is not a dependency of
  this app, and the ticket's rule is no new hard runtime dependency for the
  .deb, so on GNOME Wayland the compositor decides where the popup goes
  (usually centred) and it may not stay above other windows. Check 1 on
  Wayland records what actually happens.
- **Focus on show:** the window is built with `focused(false)`; whether
  `show()` of an already-made, hidden window activates it on Windows is not
  verified (check 1).
- The main window still gets the prompt on `detection://prompt`, so if it is
  open its banner asks too. Dismissing the popup does not clear that banner;
  starting a recording from either clears both.
