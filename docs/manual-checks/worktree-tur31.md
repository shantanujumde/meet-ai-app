# Manual checks: TUR-31 (audio-activity signal, for calls with no invite and no known app)

These need the running, signed app, a real mic and speakers, a real browser
call and permission to post notifications, so they were not run here. The
headless parts are covered by tests in `crates/detect/src/activity.rs` (the
hold window, music alone, a mic check alone, any off reading restarting the
window, once per call, the 60 s re-arm, recording), `crates/detect/src/detector.rs`
(audio activity alone asks as audio activity, Slack is named instead, Zoom
plus its call asks once, nothing while recording), `crates/detect/src/poll.rs`
(the loop asks once) and `src-tauri/src/detection/` (the
`detection.audio_activity` switch, the loop with `detection.processes` off).

How it works: every 2 s the app reads Core Audio's
`kAudioDevicePropertyDeviceIsRunningSomewhere` on the default input and the
default output device. Those are property reads: no stream, no tap, no
permission prompt. When both have been running for 20 s in a row
(`AUDIO_HOLD`), and meet-ai is not recording, it tells the meeting-app loop.
That loop names Slack or Discord if one is open, stays quiet if a prompt went
out in the last 5 minutes (Zoom opening, say), and otherwise asks "Record this
meeting?" with "Your microphone and speakers are both in use, like on a call."
After that it asks again only once mic and speakers have not both been running
for 60 s in a row (`AUDIO_REARM`), so a muted mic mid-call does not ask twice.

## 1. Google Meet in Chrome → one prompt

1. Build and run the signed app (`just bundle-signed`, then open it). Let it
   post notifications when macOS asks. Quit Zoom, Teams, Webex, Slack and
   Discord.
2. With nothing recording, join a Google Meet call in Chrome with mic and
   camera on, and someone (or a second device) talking.
3. Expected about 20 s after the call's audio starts: one notification titled
   "Record this meeting?" with "Your microphone and speakers are both in use,
   like on a call. Open meet-ai to record it.", and the same banner in the
   meet-ai window. `meet-ai.log` has "watching the mic and speakers" at
   startup and "a meeting looks like it started; asking whether to record".
4. Stay in the call for 5 minutes, muting and unmuting once. Expected: no
   second prompt.
5. Leave the call, wait over a minute, join another. Expected: one new prompt.

## 2. Music with no call → no prompt

1. With no call, play music in Music or Spotify for 5 minutes.
2. Expected: no notification and no banner.
3. Also: open Voice Memos and record for a minute with nothing playing.
   Expected: no prompt (mic alone).

## 3. No prompt while recording

1. Start a recording with ⌘⇧R, then join a Meet call.
2. Expected: no prompt, not during the recording and not right after it is
   stopped while that call goes on.

## 4. Zoom call → one prompt, not two

1. Open Zoom (expected: TUR-27's "Zoom is open." prompt; press **Dismiss**).
2. Start a Zoom call within 5 minutes. Expected: no second prompt.

## 5. Slack huddle → named as Slack

1. Open Slack and start a huddle with mic on and someone talking.
2. Expected after about 20 s: one prompt saying "Slack is open.", not the
   audio-activity sentence.

## 6. `detection.audio_activity = false`

The switch is read once at startup, so a change needs a restart.

1. Set `"detection": { "audio_activity": false }` in
   `~/Meetings/.app/config.jsonc`, restart the app, and repeat check 1.
2. Expected: no prompt, and `meet-ai.log` says "detection.audio_activity is
   off; not watching the mic and speakers".
3. Set `"processes": false` and `"audio_activity": true` instead, restart, and
   repeat check 1. Expected: the prompt from check 1 (audio activity runs its
   own copy of the loop that sees no apps).

## 7. No permission prompt

Neither the example binary below nor the app's audio-activity loop should ever
trigger a macOS microphone prompt: the reads open no stream. During check 1 on
a fresh install, the only prompts should be the notification one and, when
recording starts, the usual microphone and audio-capture ones.

## Idle CPU

Measured on this Mac with the release example, which does exactly the app's
reads at the app's cadence:

```sh
cargo build --release -p audio --example activity_poll
/usr/bin/time -l target/release/examples/activity_poll 90
```

Result on 2026-10-03 (Apple Silicon, nothing playing, mic idle): 45 reads in
90.80 s real, 0.00 s user + 0.01 s sys, so about 0.01 % of one core (at the
timer's 10 ms resolution; 79 M cycles in total, about 1.8 M per read). 10 MB
peak RSS for the whole example process. No permission prompt appeared. Run
under `perl -e 'alarm shift; exec @ARGV' 95` because macOS has no `timeout`.

## Known limits

- `DeviceIsRunningSomewhere` does not say which process is using a device.
  Another app recording the mic while something else plays sound (a dictation
  tool over music, a game with voice chat) looks like a call and asks once.
- A Bluetooth headset that keeps its input open when idle makes "mic in use"
  true all the time; music on it would then ask once. Not seen here; check
  with AirPods if possible.
- Audio activity hours after a Zoom prompt, with Zoom still open, asks with
  the audio-activity sentence rather than "Zoom is open.": Zoom's own
  once-per-session rule already used its prompt.

## Decisions (answered by Shann in `.agent/ANSWER.md`)

- One call is one prompt: after firing, it re-arms only after mic and
  speakers have not both been running for 60 s straight.
- Audio activity first counts as a call signal (so Slack and Discord can be
  named), and asks as audio activity only when that poll names no app and
  nothing was asked in the last 5 minutes.
