# TUR-137 manual checks

`transcription.live: false` is covered headless by
`live_transcript::after_stop::tests` (fake batch engine) and
`config::transcription_tests`. These need a signed build, a microphone and a
real call, so they were not run here.

## 1. A call with `live: false`

- Run: set `"live": false` under `transcription` in
  `~/Meetings/.app/config.jsonc`, start a recording, talk for a minute with
  someone on a call, press Stop.
- Expected: no live lines while recording, and no `transcript.md` in the
  meeting folder until Stop. After Stop, `transcript.md` appears with both
  `You` and `Others` lines in time order, and the notes run (if
  `agent.auto_run` is on) starts only after it. The log has
  "transcribing the meeting after Stop" and then "the meeting was transcribed
  after Stop".
- Skipped: needs a signed build, the mic and system-audio permissions, and a
  meeting app.

## 2. A 45-minute meeting with `live: false`: notes start after the batch

- Run: as check 1, with a meeting of about 45 minutes, on whisper and on
  Apple Speech.
- Expected: Stop returns within about 10 seconds. The meeting shows that it
  is finishing the transcript until the batch ends, however long it takes
  (there is no cap), then the notes run starts. With
  `audio.retention_days: 0`, the audio is deleted after the batch, not before.
- Skipped: needs a signed build and a real long recording; batch speed on
  real hardware is not known here, verify it.

## 3. Back to `live: true`

- Run: set `"live": true` (or remove the key), record a short call.
- Expected: live lines in the pane while recording, as before TUR-137.
- Skipped: needs a signed build and a real call.

## Known follow-up

- With `live: false` the live pane says "Starting transcription..." for the
  whole call, because its status stays `idle` and `src/ui/LiveTranscript.tsx`
  words idle that way. No UI change in this ticket (no new events, no
  Settings toggle); a follow-up should give the pane its own wording.
- A meetings-folder move started while the after-Stop batch is still running
  is not held off by the folder gate, the same as a live engine abandoned at
  Stop today. Verify it before relying on a move right after a long meeting.
