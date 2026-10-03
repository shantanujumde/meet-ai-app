# Manual checks: TUR-25 (config.jsonc: read the calendar and detection sections)

TUR-25 only adds the readers (`config::calendar()`, `config::detection()`).
Nothing on main calls them yet: the calendar refresh loop (TUR-28) and the
detection loop (TUR-27) will. Parsing, defaults, overrides, the bad-provider
error and the fall-back-to-defaults rule are covered headless by the tests in
`src-tauri/src/config/calendar_section.rs` and `detection_section.rs`. The
checks below need the running, signed app with those loops merged, so they
were not run here.

## 1. No sections: defaults

1. Remove `calendar` and `detection` from `~/Meetings/.app/config.jsonc`
   (or delete the file) and start the app.
2. Expected: calendars are read from EventKit every 15 minutes; calendar,
   meeting-app and audio-activity detection are all on; an event with one
   attendee is not offered as a meeting, one with two is.

## 2. Each detection switch is a config edit only

1. Set `"detection": { "processes": false }`, restart the app, open Zoom.
2. Expected: no meeting prompt from the process signal. Repeat for
   `"calendar": false` (a calendar event starting does not prompt) and
   `"audio_activity": false` (mic + speakers in use does not prompt).

## 3. A bad provider name

1. Set `"calendar": { "providers": ["googel"] }` and restart the app.
2. Expected: the app starts normally. `meet-ai.log` has a WARN line
   `config.jsonc's calendar section is not valid; using defaults` with
   `calendar.providers "googel" is not one of eventkit, google, microsoft, ics`,
   and events still come from EventKit.

## 4. A known provider that is not built yet

1. Set `"calendar": { "providers": ["eventkit", "google"] }` and restart.
2. Expected: `meet-ai.log` has `calendar provider is not available yet;
   skipping it` with `provider=google`; EventKit events still appear. (This
   needs the refresh loop to call `CalendarConfig::available_providers()`.)

## Not changed

`src-tauri/config.schema.json` already matches SPEC §3.5 for both sections
(keys, defaults, provider enum; a test checks the enum against the code), so
it was left as is. Its "Not read by the app yet" descriptions stay true until
TUR-27/TUR-28 wire the readers in.
