# Manual checks: TUR-134 (meetings folder watch problem note)

## Linux: inotify limit shows the note

- Run: on Linux, `sudo sysctl fs.inotify.max_user_watches=64`, keep more than 64 subfolders in the meetings folder, launch meet-ai and open Meetings. Then make one more meeting folder while it runs.
- Expected: under the Meetings header, "meet-ai cannot watch the meetings folder, so changes made in other apps show up after a restart." followed by the message naming `fs.inotify.max_user_watches`. Raise the limit back, change the meetings folder in Settings (or restart): the note goes away.
- Why skipped: needs a real Linux machine and root; this worker runs headless on macOS. Whether the startup watch fails or only later folders fail depends on how notify 8.2 reports ENOSPC during the recursive add: verify it.

## Look of the note

- Run: same as above, look at the page.
- Expected: two footnote-size secondary-colour lines under the header, long paths wrap.
- Why skipped: jsdom has no layout.
