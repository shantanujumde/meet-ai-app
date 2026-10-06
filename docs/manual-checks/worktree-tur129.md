# Manual checks: TUR-129 (tray follows the system's 12/24-hour clock)

Automated: `cargo test -p meet-ai --lib tray` covers both time formats
(`9:30 AM` / `14:05`) and reading the macOS and Windows patterns, quoted text
included. The OS reads themselves need a real desktop.

## macOS: 12-hour

1. Signed build, a calendar with a meeting later today.
2. System Settings, General, Date & Time: turn "24-hour time" off.
3. Within a minute, the tray's Today menu shows `2:30 PM  <title>`.
4. Turn it on again: `14:30  <title>`.

Skipped here: needs the running signed app and a calendar.
Verify it: `dateFormatFromTemplate:"j"` follows the switch (not only the locale).

## Windows: 12-hour

1. Settings, Time & language, Region, Regional format: set the short time to
   `h:mm tt`, then `HH:mm`.
2. The tray's Today times follow within a minute.
3. Verify it: `HKCU\Control Panel\International\sShortTime` changes with the
   setting (`reg query "HKCU\Control Panel\International" /v sShortTime`).

Skipped here: needs a real Windows machine.

## Linux

Always 24-hour by design (no GNOME/KDE setting is read).
