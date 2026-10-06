# Manual checks: tur108

TUR-108: meeting reminders show as a compact Granola-style card window
(`src/ui/PromptPopup.tsx`, `src-tauri/src/detection/popup/`): accent bar,
title, time range, and one split button (**Join Meet & record** or
**Record**) whose chevron opens a native menu (Join only, Record only, Open
brief, Dismiss). The window is 380×72 logical px, transparent around the
rounded card. Reminders use it on every OS, macOS included
(`popup::uses_popup`); detection prompts use it on Windows and Linux only.

Tested headless on a Mac: `cargo test -p meet-ai --lib detection::` (prompt
fields, `join_service`, Open brief, `uses_popup`/`popup_for` for both
platform switches, the 380×72 size and the top-right maths including a work
area under a menu bar), `cargo test -p calendar --lib join_url`, and
`pnpm vitest run src/ui/PromptPopup.test.tsx` (reminder with a link, without
a link, detection prompt, each menu item's answer, with the menu module
faked). None of the window code ran: no app was launched.

## Run by hand

Use a build of this branch (signed on macOS) on macOS 26, Windows 11, Linux
X11 and GNOME Wayland.

1. Settings, Notifications, **Send a test reminder**.
   Expect: a 380×72 card top-right of the primary screen. On macOS it sits
   just under the menu bar (about 16 px below it and from the right edge),
   not under it and not over it. Title "Test meeting", time range with
   "(test, nothing records)", a **Join Meet / & record** button and a
   chevron. Every choice just closes it; nothing records. Left alone it
   goes after about 20 s.
   Why skipped: needs the running app. Verify it: macOS `work_area` really
   excludes the menu bar (tauri-runtime-wry reads `NSScreen.visibleFrame`;
   its source says so, nothing here ran it), also with "Automatically hide
   and show the menu bar" on, and on a second display.
2. Same card: look at the corners.
   Expect: rounded corners with the desktop showing through, no white or
   black square behind them. macOS draws its shadow around the card shape.
   Why skipped: needs a real window. Verify it: transparency on Windows
   WebView2 (the main window turns transparency off there for Mica) and on
   Linux (without a compositor the corners may show black).
3. Focus and Dock: type in another app, then trigger the test reminder.
   Expect: typing carries on in that app (the card takes no focus), no
   new Dock icon or taskbar entry appears, and the main meet-ai window does
   not come forward. A first click on the card's button works without an
   extra click to focus it.
   Why skipped: needs the running app. Verify it on all three OSes.
4. Click the chevron.
   Expect: a native menu just under the chevron, even though it is taller
   than the 72 px window. Join only opens the link and leaves the card up;
   Record only starts a recording; Open brief brings the main window to
   that meeting's brief and closes the card; Dismiss closes it. Nothing
   records without a click.
   Why skipped: needs the running app. Verify it: the menu pops up from a
   window that is not key/focused on macOS and Windows, and under GNOME
   Wayland.
5. A real calendar event with a Google Meet link, starting in a few minutes
   (lead time from Settings, 1 min by default).
   Expect: the card appears at the lead time with the event's title and
   time. **Join Meet & record** opens Meet and starts a recording titled
   after the event. With a Zoom or Teams link the button says Join Zoom or
   Join Teams; with no link it says **Record** and the menu has Open brief
   and Dismiss only.
   Why skipped: needs a signed-in calendar account and the running app.
6. On macOS, open Zoom with meet-ai idle.
   Expect: unchanged from before: the system notification and the in-app
   banner, no card. On Windows and Linux: the card titled "Zoom is open."
   with **Record** and Dismiss in the menu.
   Why skipped: needs a real meeting app.
7. With the card up, start a recording another way (tray or ⌘⇧R).
   Expect: the card closes.
   Why skipped: needs the running app.
8. Switch Appearance between Light and Dark with the card up (send a test
   reminder after each switch).
   Expect: title, time and buttons readable in both.
   Why skipped: needs the running app.
9. Fallback: if the card cannot be shown (log line `could not show the
   prompt popup`), the system notification appears instead on every OS.
   Why skipped: no way to make the window fail headless.

## Known limits

- Wayland: as in TUR-59, the compositor decides where the card goes.
- The bar is the accent colour; the calendar's own colour is not stored.
