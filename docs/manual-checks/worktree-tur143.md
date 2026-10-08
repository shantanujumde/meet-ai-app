# TUR-143: Call-started prompt naming the app, with a "Never for" list

All of these need the signed app running on a real Mac (macOS 26), a real
call and real apps on the mic, which the worker cannot do. The rules
themselves are unit-tested with fake readings and a fake clock
(`crates/detect/src/call_start.rs`, `mic_poll.rs`).

## A call app names itself

- Run: a signed build, Settings → Notifications with "Ask to record when a
  call starts" on. Start a WhatsApp (or FaceTime, Zoom, Teams) call and talk
  for 20 seconds.
- Expected: about 15 to 17 s after the call picks up the mic, the card says
  "WhatsApp call detected" with Record and Not now; its ⋯ menu has "Never for
  WhatsApp". Nothing records until Record is pressed.
- Why skipped: needs a real call and the Core Audio process list.

## A browser call names the browser

- Run: join a Google Meet in Chrome (then Safari, Arc, Firefox, Edge, Brave,
  Helium, Dia, Comet if installed) and talk for 20 s.
- Expected: "Call detected in Google Chrome" (Safari: "Call detected in
  Safari", through its WebKit GPU process).
- Why skipped: needs a real browser call. Verify it: Dia and Comet are
  matched by their app folder name only ("Dia.app", "Comet.app"); their
  bundle ids are not confirmed, so check `mic_users` names them as browsers.

## Not now, then 10 minutes of quiet

- Run: get the WhatsApp prompt, press Not now, hang up, and call again within
  a few minutes. Then wait more than 10 minutes after the click and call
  again.
- Expected: no prompt for the second call; the third call asks.
- Why skipped: needs real calls.

## Never for <App>

- Run: on a call prompt, ⋯ → Never for WhatsApp. Then open Settings →
  Notifications, and call again.
- Expected: `detection.never_detect` in `~/Meetings/.app/config.jsonc` holds
  `"WhatsApp"`; the "Never detect" row lists WhatsApp with a Remove button;
  the next WhatsApp call never asks. After Remove, it asks again.
- Why skipped: needs the running app and a real call.

## Dictation apps and Krisp never ask

- Run: with music playing, dictate with Superwhisper, Wispr Flow, MacWhisper,
  Raycast and Siri for 30 s each. Separately, join a Zoom call with Krisp as
  the mic.
- Expected: no prompt for the dictation apps (neither "call detected" nor
  "Audio activity"); the Zoom call asks once as "Zoom call detected", never
  as Krisp. Verify it: Krisp's bundle id `ai.krisp.krispMac` comes from the
  ticket; check what Core Audio reports for Krisp on a real Mac.
- Why skipped: needs the real apps on the mic.

## Teams open all day with no call

- Run: leave Teams (and Slack, WhatsApp) open for a working day without a
  call.
- Expected: no prompt at all ("Microsoft Teams is open" no longer asks where
  the app list can be read). Verify it: that Teams does not hold the mic open
  while idle.
- Why skipped: needs a day of real use.

## Already recording

- Run: start a recording by hand, then start a WhatsApp call; stop the
  recording while the call goes on.
- Expected: no prompt during the recording, and none after Stop for the same
  call. The next call asks.
- Why skipped: needs a signed build and a real call.

## A calendar reminder and the call are one prompt

- Run: a calendar event with 2+ people starting in a minute, then join its
  Zoom call within 3 minutes of the reminder.
- Expected: the reminder card only; no second "Zoom call detected" card.
- Why skipped: needs a real calendar and call.

## An app we do not know still asks as audio activity

- Run: record a voice memo in an app not in the table (QuickTime audio
  recording, say) with music playing, for 30 s.
- Expected: "Audio activity" after about 20 s, as TUR-31 did.
- Why skipped: needs real devices.

## macOS older than 14: the old prompts

- Run: on macOS 13 (or with `mic_users` returning NotSupported), open Zoom.
- Expected: "Zoom call" from the app-is-open rule (TUR-27), as before; no
  error shown. WhatsApp, FaceTime, Telegram and Signal being open ask only
  with a call signal, like Slack. Verify it: the process names `Telegram`
  and `Signal` (macOS) were not checked on a Mac with them installed;
  `WhatsApp` and `FaceTime` were (`CFBundleExecutable`).
- Why skipped: meet-ai targets macOS 26; no older Mac here. Windows and
  Linux were not given the new chat apps in `processes.json` (their names
  are not sourced); they read the mic users from WASAPI and PulseAudio
  anyway.
