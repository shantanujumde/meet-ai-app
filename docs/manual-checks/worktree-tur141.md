# Manual checks: tur141

TUR-141 (parent): know when a call starts and ends in any meeting app, ask
to record on the start, ask to stop on the hang-up. Its pieces: TUR-142 lists
the apps using the mic (`audio::mic_users`), TUR-147 is the card and its
countdown, TUR-143 the call-start prompt and the "Never detect" list, TUR-144
the call-end countdown (this file's own checks, below), TUR-145 the silence
and sleep stops. Run the overall check once all of them are on main; each
ticket's own file (`worktree-tur142.md`, `worktree-tur147.md`, ...) has the
finer checks.

Use a signed build of main on macOS 26 (mic and system-audio permission
granted), with real accounts in Zoom, Chrome (Google Meet), WhatsApp,
FaceTime and Slack, and a second person or device to call. Keep
`cargo run -p audio --example mic_users 600` running in a terminal beside
it: it prints which apps use the mic every 2 s.

## The overall check (TUR-141)

1. For each of Zoom, Meet in Chrome, WhatsApp, FaceTime and a Slack huddle
   (Slack desktop, and the huddle in Chrome): start a call with meet-ai
   idle.
   Expect: the example prints the app (`Zoom [CallApp]`,
   `Google Chrome [Browser]`, `WhatsApp [CallApp]`, `FaceTime [CallApp]`,
   `Slack [CallApp]`) as soon as the call has the mic, i.e. Core Audio's
   `IsRunningInput` is on for that process; within about 15 s the card asks
   "Zoom call" / "Call detected in Chrome" with Record (TUR-143). Nothing
   records until Record is clicked (L15).
   Why skipped: needs real calls, accounts and a signed build.
2. Click Record, talk for a minute, then hang up from the call app.
   Expect: the example drops the app within one line (2 s) of the hang-up,
   i.e. `IsRunningInput` turns off; about 5 to 7 s after the hang-up the
   card shows "Zoom call ended" ("Call ended in Google Chrome" for Meet and
   the Chrome huddle, "FaceTime call ended", "WhatsApp call ended", "Slack
   call ended") with a ring counting 10 to 0; at 0 the recording stops and
   saves, as if Stop was pressed, and the meeting shows in the list.
   Why skipped: as above. Verify it: each app really releases the mic on
   hang-up (some apps keep a mic stream open in their lobby or after the
   call; if one does, note which and how long).
3. Mute yourself in each app for 30 s mid-call.
   Expect: no end card. Verify it: whether muting in each app keeps
   `IsRunningInput` on (the example keeps listing the app). An app that
   closes the mic on mute would show the card after 5 s of mute; note it.
   Why skipped: needs real calls.
4. Dismiss a start prompt, then **Never for Zoom** on another (TUR-143).
   Expect: no prompt for the same app for 10 min after Dismiss; never again
   for Zoom until it is removed from Settings, Notifications, Never detect.
   Why skipped: needs real calls; TUR-143's own checks cover the details.
5. macOS 13 or older (if a Mac is at hand), and a Linux desktop with no
   sound server.
   Expect: the example says `not supported here`; start prompts fall back
   to "Zoom is open" (TUR-27) and audio activity; no end card ever, no
   error; a recording only stops by hand or by the backup stops (TUR-145).
   Why skipped: no such machine here.
6. Windows 11 and Linux (PipeWire with pipewire-pulse): checks 1 and 2 with
   Zoom and Meet in Chrome.
   Expect: the same cards; the end card 5 to 7 s after the hang-up.
   Why skipped: needs those machines and calls; CI runs only the unit tests.

## TUR-144: the call-end countdown

The rules are `crates/detect/src/call_end.rs` (pure, unit-tested with fake
readings and a fake clock: 28 tests in `call_end/tests.rs`); the wiring is
`src-tauri/src/detection/call_end.rs` (the 2 s loop, the 250 ms countdown
thread `drive`, tested with a fake card and clock). Nothing below ran: no
app was launched and no call was made.

1. Hang up in Zoom, Meet (Chrome), WhatsApp, FaceTime and a Slack huddle
   while recording (started from the call prompt).
   Expect: the countdown card within about 5 to 7 s (5 s off the mic,
   measured from the first 2 s reading that saw it gone); at 0 the
   recording stops and saves. The log says `off the mic for 5 s: asking to
   stop`, then `the call ended: stopping the recording`.
   Why skipped: needs real calls and a signed build.
2. A short mic drop: mid-call, switch the input in the call app's audio
   settings (built-in mic to a USB mic and back), and connect and
   disconnect a Bluetooth headset.
   Expect: no card. Verify it: how long each app's process is off the mic
   (`IsRunningInput` off) during a switch; under 5 s never asks. If a
   Bluetooth reconnect keeps it off for longer, note the app and the time.
   Why skipped: needs real calls and the devices.
3. Rejoin during the countdown: hang up, wait for the card, rejoin the same
   call (or start a new one in the same app) before 0.
   Expect: the card goes quietly within about 2 s of the app taking the mic
   again; the same recording goes on (the same meeting in the list, no new
   one, no gap in the audio). Also rejoin during the 5 s wait: no card at
   all.
   Why skipped: needs real calls.
4. Keep recording: hang up, press **Keep recording**.
   Expect: the card closes, the recording goes on, and no card comes back
   while the app stays off the mic. Join a new call in the same app and
   hang up again: the card comes back.
   Why skipped: needs real calls.
5. Stop now: hang up, press **Stop now**.
   Expect: the recording stops and saves at once.
   Why skipped: needs real calls.
6. Stop by hand during the countdown: the Stop button, ⌘⇧R, and the menu
   bar's Stop.
   Expect: the recording stops, and the card closes within about a quarter
   of a second; nothing stops twice and no error shows.
   Why skipped: needs the running app.
7. A manual recording (the Record button, ⌘⇧R) and a calendar one (a
   reminder's Record) with no call app on the mic, then a WhatsApp call
   joined during it, then hung up.
   Expect: WhatsApp is followed (the first call app seen); hanging it up
   shows "WhatsApp call ended". A recording during which no call app ever
   uses the mic never shows the card.
   Why skipped: needs real calls.
8. Two call apps: record from Zoom's prompt while Chrome also uses the mic
   (a Meet tab), then close the Meet tab.
   Expect: no card (Zoom is followed, not Chrome); hanging up Zoom shows
   "Zoom call ended".
   Why skipped: needs real calls.
9. Settings, Notifications, **Ask to stop when a call ends** off, then hang
   up a recorded call.
   Expect: no card; the recording goes on. On again: the next hang-up asks.
   `config.jsonc` gets `detection.call_end`.
   Why skipped: needs the running app and a call.
10. While another card is up (a reminder) when the call ends.
    Expect: the countdown replaces it. If a reminder replaces the
    countdown, the recording is kept (as Keep recording), never stopped.
    Why skipped: needs the running app.

## Decisions taken (TUR-144)

- The countdown's zero comes from the card (`CountdownEnd::TimedOut`); the
  rules also stop 1 s after zero (`TIMEOUT_GRACE`) if that report never
  arrives. A card that could not be shown never stops a recording.
- A card closed without an answer (replaced by another card) counts as
  Keep recording.
- The followed app must first be seen on the mic: a recording started after
  the call already ended never shows the card.
- A prompt's app is matched to the mic list by id or by name, without case
  (`Prompt::app` is the label, "Zoom"). The "Never detect" list (TUR-143)
  does not change which app a recording follows.
- Dictation and audio tools (`AppKind::IgnoredSystem`) are never followed.
