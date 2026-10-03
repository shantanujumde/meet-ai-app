# Manual checks: TUR-27 (spot a running meeting app and ask before recording)

These need the running, signed app, real meeting apps and permission to post
notifications, so they were not run here. The headless parts are covered by
tests in `crates/detect` (matching, the once-per-session rule, Slack/Discord
needing a call signal, no prompt while recording, the loop),
`src-tauri/src/detection/` (the `detection.processes` switch, the one prompt
path, no prompt while recording) and `src/ui/DetectionPrompt.test.tsx` (the
banner, Record, Dismiss).

How it works: every 5 s the app lists running processes and matches their
names, exactly but ignoring case, against `crates/detect/processes.json`. A
match posts a "Record this meeting?" notification saying why ("Zoom is open.
Open meet-ai to record it.") and shows the same question as a banner in the
bottom-right of the window, with **Record** and **Dismiss**. macOS
notifications from `tauri-plugin-notification` cannot carry buttons, so the
answer is given in the window; clicking the notification brings meet-ai
forward.

## 1. Zoom opens → one prompt naming Zoom → Record records

1. Build and run the signed app (`just bundle-signed`, then open it).
   Let it post notifications when macOS asks.
2. With nothing recording, open Zoom.
3. Expected within about 5 s: one notification titled "Record this
   meeting?" with "Zoom is open. Open meet-ai to record it.", and the same
   banner in the meet-ai window. `meet-ai.log` has "a meeting looks like it
   started; asking whether to record".
4. Press **Record** in the banner. Expected: a recording starts exactly as if
   the record button had been pressed, and the banner goes away.

## 2. Dismiss → no repeat until Zoom restarts

1. Stop the recording. Quit Zoom, open it again, and press **Dismiss** on the
   new banner.
2. Leave Zoom open for a few minutes. Expected: no further notification or
   banner.
3. Quit Zoom (⌘Q) and open it again. Expected: one new prompt.

## 3. No prompt while recording

1. Quit Zoom. Start a recording with ⌘⇧R. Open Zoom.
2. Expected: no notification, no banner — not while recording, and not when
   the recording is stopped while that Zoom stays open.

## 4. Slack and Discord alone never prompt

1. Open Slack and Discord and leave them open. Expected: no prompt. They only
   prompt when a calendar event or audio activity was seen in the last
   5 minutes, and neither signal feeds detection yet (the calendar refresh
   loop, TUR-28, should call `detection::Detection::call_signal`).

## 5. The process names are right on a real Mac

`processes.json` names `zoom.us`, `Microsoft Teams`, `Webex`, `Slack` and
`Discord` (SPEC §2.3). These are what `sysinfo` should report as the process
name, but this was not checked against real installs here. With each app
open, run `ps -axco pid,comm | grep -iE 'zoom|teams|webex|slack|discord'` and
check the main process's `comm` matches a `name` in the file exactly. New
Teams in particular may run as `MSTeams`; if so, add an entry (a data edit,
no code).

## 6. `detection.processes = false`

The switch is read once at startup from TUR-25's config section
(`crate::config::detection().processes`, default `true`), so a change needs a
restart.

1. Set `"detection": { "processes": false }` in
   `~/Meetings/.app/config.jsonc` and restart the app. Open Zoom.
2. Expected: no notification, no banner, and `meet-ai.log` says
   "detection.processes is off; not watching for meeting apps".
3. Set it back to `true` (or remove the key), restart, and open Zoom.
   Expected: the prompt from check 1.

## Known limits

- The system notification is posted during onboarding too; only the in-app
  banner waits until onboarding is done.
- A meeting app already open at launch counts as just opened, so it prompts
  once on launch.

## Decisions taken without an answer

- The once-per-session rule keys on the app's name and its set of pids: a
  second process under the same name joins the session, and the session ends
  when the last of them exits.
- An app first seen while recording counts as handled, so stopping that
  recording does not trigger a prompt for the call that was just recorded.
- The prompt carries a `label` per app (`Zoom` for `zoom.us`), added to the
  ticket's `{ name, needs_call_signal }` entry shape so the notification
  names the app the way users know it.
