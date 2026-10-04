# TUR-58 manual checks

Tray icon, Ctrl+Alt+R and start at login on Windows and Linux. Built and unit
tested on a Mac only (`cargo test -p meet-ai`, clippy, vitest). None of the
steps below were run: each needs a real Windows or Linux desktop, or the
running app. CI builds and runs the unit tests on Windows and Linux; it does
not draw a tray or press keys.

## Windows 11

1. Install the PR's Windows build. Taskbar on dark (Settings, Personalization,
   Colors, "Choose your mode": Dark). Expected: a light "m." in the tray
   overflow or tray, clearly visible.
2. With meet-ai idle, switch the mode to Light. Expected: the tray icon turns
   into a dark "m." within a moment, with no recording needed. Switch back to
   Dark: light again. (Driven by the window's ThemeChanged event; if the icon
   does not follow while the window is hidden, note it.)
3. Press Ctrl+Alt+R with another app focused. Expected: recording starts, the
   tray icon turns red and the menu item reads "Stop recording". Press again:
   it stops and the icon goes back.
4. Ctrl+Shift+R in a browser still hard-reloads the page.
5. Settings, Menu bar, "Start at login" on. Sign out and in. Expected: meet-ai
   is running (tray icon), not recording. Turn it off; sign out and in: it
   does not start. Check `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
6. The record button and empty states show "Ctrl+Alt+R", not "⌘⇧R".

## Linux X11 (for example Ubuntu on Xorg, KDE on X11)

1. Expected: an ember (orange) "m." in the tray on light and dark panels; red
   while recording.
2. Ctrl+Alt+R toggles recording from another focused window.
3. Every action (Open meet-ai, Start/Stop recording, Quit, today's meetings)
   works from the tray menu.
4. "Start at login": a meet-ai `.desktop` file in `~/.config/autostart` appears and goes.

## Linux Wayland (GNOME and KDE)

1. Bind `meet-ai --toggle-recording` as in the README. With meet-ai running,
   press the key: recording toggles, no second app, no window pops up.
2. Quit meet-ai and press the key: meet-ai opens and does **not** record
   (SPEC L15; log line "started with --toggle-recording and no app running").
3. GNOME without the AppIndicator extension: no tray icon. Close the window;
   launch meet-ai again: the window comes back.

## macOS

1. Nothing changes: template glyph in the menu bar in every state, ⌘⇧R still
   toggles, labels still read "⌘⇧R".
2. "Start at login" on adds a meet-ai plist in `~/Library/LaunchAgents`;
   log out and in: meet-ai runs, not recording. Off removes it.
