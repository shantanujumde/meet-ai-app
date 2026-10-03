# Manual checks: TUR-80 (other side of the call lost on Bluetooth headsets)

Ticket: https://linear.app/meet-ai-app/issue/TUR-80/other-side-of-the-call-is-lost-with-bluetooth-headsets-system-track

None of this could run here: it needs the system-audio tap and its macOS
permission, a Bluetooth headset, a real call, a signed build and the running
app. The headless tests cover the logic without a device:

- `crates/audio/src/macos/tap_rate.rs`: `effective_input_rate` picks the
  input stream's format, then the aggregate's nominal rate, then the tap
  format (48 kHz tap + 16 kHz aggregate gives 16 kHz).
- `crates/audio/src/macos/tap_pipeline.rs`: 16 kHz frames with a 48 kHz
  "reported" tap format come out 1:1 (before the fix: 1/3); 3 s at 48 kHz then
  a switch to 16 kHz for 4 s comes out as 7 s, and the 48 kHz part flushes to
  exactly 3 s; switching back up flushes exactly too.
- `crates/audio/src/segments/rate_guard.rs`: the owner's 1/3 system track is
  caught with both frame rates; in-step channels pass.

## What changed

- The tap now resamples from the rate the IO proc really delivers (the
  aggregate's input stream format, else the aggregate's nominal rate, else
  the tap format). All of them, plus the output device's rate, are logged at
  start: `system tap rates (start): tap format .. Hz, aggregate nominal ..,
  input stream .., output device ..; resampling from .. Hz`.
- `kAudioDevicePropertyNominalSampleRate` listeners on the aggregate and on
  the output device. On a change the worker flushes the old resampler (its
  partial chunk and filter tail, trimmed to exactly `input * 16000 / rate`
  frames) and builds a new one at the next chunk boundary. The WAV is not
  reopened and `segments.json` gets no new segment: `sys_rate` is the output
  rate (16 kHz) and does not change. Logs: `system tap input rate changed
  48000 Hz -> 16000 Hz mid-recording` and `system tap resampler switched ..`.
- Every checkpoint compares what each channel gained since the previous
  anchor; more than 5 % apart logs `system track is not keeping pace with the
  microphone: ... mic gained .. frames/s, system .. frames/s (ratio ..)`.
- The permission check uses the same `SystemSource`, so it gets the same rate.

## Checks to run

| # | What to do | Expected | Why skipped here |
|---|---|---|---|
| 1 | Owner's Mac, signed build of this branch, realme Buds Air7 (or any Bluetooth earbuds) as output and input. Start recording, then a WhatsApp call for 2+ minutes. | `system.wav` duration ≈ `mic.wav` (within a second); the other person appears in the live transcript; `segments.json` `sys_frames` ≈ `mic_frames`. Log `system tap rates (start)` shows `resampling from 16000 Hz`. No `not keeping pace` warning. | Needs Bluetooth hardware, a real call, a signed build and the tap permission. |
| 2 | Earbuds connected in music mode (A2DP, 48 kHz). Start recording, play something, then start a WhatsApp/Zoom call mid-recording. | Log `input rate changed 48000 Hz -> 16000 Hz` and `resampler switched`; `system.wav` duration still ≈ `mic.wav`; no `not keeping pace` warning after the switch; both sides transcribed. | Same. |
| 3 | Built-in speakers, then wired headphones (48 kHz). Record a Zoom/YouTube clip for a minute each. | `resampling from 48000 Hz`, durations equal, no warning: no regression. | Needs real output devices and the tap permission. |
| 4 | Earbuds in a call (HFP 16 kHz). Onboarding or Settings: "Check again". | Granted; the log's `captured=` grows at real time (≈16 000 per second, not ≈5 300). | Needs the tap permission and the running app. |
| 5 | If check 1 or 2 fails: read the `system tap rates` lines. | If `aggregate nominal` stays 48000 while `output device` reads 16000, the aggregate does not follow its main sub-device; the fix then is to prefer the output device's rate in `effective_input_rate`. | Needs the real machine. |

## Choices made without an answer

- No new `segments.json` segment on a rate change: the existing schema's
  `sys_rate` is the WAV rate (always 16 kHz) and a reopen only exists for
  default-device changes; flushing + a new resampler keeps the file
  continuous and the timeline exact.
- The rate-change listener's small state is leaked on purpose (a few dozen
  bytes per segment) because Core Audio does not promise that an in-flight
  notification has returned when the listener is removed.
