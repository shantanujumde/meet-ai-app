# Manual checks: TUR-155

A bad value in `config.jsonc` now costs only its own key (`config/keyed.rs`).
That key reads as its default, a warning is logged, and every good key is
kept. The Settings card for that section says which value was bad and why.
Setters write only the keys the user changed, so a bad key the user did not
touch stays as written. The only refusal left is a file that does not parse
at all. The readers, the writers, the symlink handling and the cards' notes
are unit-tested headless (`cargo test -p meet-ai --lib config`, vitest on
`NotificationSettings`, `AppearanceSettings`, `EngineSummary`, `errors`).
Nothing below was run here, because each check needs the running, signed
app.

## Run by hand

1. Set `"detection": { "min_attendees": 20 }` in config.jsonc and open
   Settings, Notifications.
   Expect: "Only for meetings with at least" shows 10 people, and a red line
   under the card starts "This setting in config.jsonc was not valid and is
   shown as the default: detection.min_attendees 20 is outside 1 to 10".
   Turning "Ask when a meeting app is running" off saves. config.jsonc still
   says `20` and now also has `"processes": false`.
   Why skipped: needs the running app.
2. Set `"transcription": { "engine": "whispr", "language": "mr" }` and open
   Settings, Speech.
   Expect: Automatic is picked, a line under "Engine" names
   `transcription.engine`, and the spoken-language picker still shows Marathi.
   Why skipped: needs the running app.
3. Set `"appearance": { "theme": 3 }` and open Settings, Appearance.
   Expect: System is picked, a line names `appearance.theme 3`, and the
   glass switch saves. Picking Dark replaces the `3` and the line goes.
   Why skipped: needs the running app.
4. Set `"agent": { "timeout_sec": 0 }` and save a tracker in Settings,
   Tracker.
   Expect: the save succeeds, with no `invalid-config` error. Reopening the
   card shows the saved tracker.
   Why skipped: needs the running app.
5. Make `~/Meetings/.app/config.jsonc` a symlink to a file elsewhere, such as
   a dotfiles repo, then change any setting.
   Expect: config.jsonc is still a symlink (`ls -l`), and the file it points
   at has the change.
   Why skipped: needs the running app and the real meetings folder.

## Known

- Fixed key by key in this ticket: `transcription`, `app`, `appearance`,
  `detection` and the calendar providers write. `agent` and `tickets` still
  return their error to the Setup screen (A11), and the `audio` retention
  reader still pauses the retention job when its section is bad (TUR-85).
- Settings, General (the Dock and menu-bar switches) has the backend
  (`config_problem("app")`) but no note on its card yet.
