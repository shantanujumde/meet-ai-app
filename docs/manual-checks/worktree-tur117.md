# Manual checks: tur117

TUR-117: the tray's Today submenus grey out Record while a recording is
starting, running or stopping. The model (`can_record`) is unit-tested; the
greyed item in the real menu bar is not.

## Run by hand

1. Connect a calendar with a meeting later today. Click the menu-bar icon,
   open the meeting's submenu.
   Expect: Record is clickable.
2. Start a recording (Start recording, or the shortcut). Open the menu and the
   meeting's submenu again, also while it says "Starting…" or "Stopping…".
   Expect: Record is greyed out and does nothing when clicked.
3. Stop the recording and reopen the submenu.
   Expect: Record is clickable again.
   Why skipped: needs the running signed app, a calendar account and a real
   menu bar; a greyed native menu item cannot be seen headless. On Windows and
   Linux, verify it too.
