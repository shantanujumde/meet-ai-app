# Manual checks: TUR-163 (no silent audio loss)

What changed in `crates/audio`: the microphone and the macOS tap now use the
loopback's capture ring (`loopback/drain.rs`): whole frames only, frames a
full ring had no room for are written as silence and counted (a `warn` line
per loss), and the ring holds 4 s of the device's own rate and channels. The
microphone is a raw `cpal` stream converted by `cpal_stream::to_f32` (F32,
I16, I32, U16, F64, I16 over 32768). Checkpoint fsyncs run outside the append
lock. Every checkpoint reopens the segment (`stream_restart`) when a channel's
position is more than 2 s behind the host clock, and a microphone stream error
that means the device went away does the same at the next tick. A build that
panics is a device error, not "permission denied". A failing checkpoint ends
the recording only after 30 s without a good one.

The logic is unit-tested headless with stub sources and fake buffers. Nothing
below was run here: each needs a real microphone, a meeting app, a Bluetooth
headset, a slow disk or the signed app.

## Run by hand

1. MacBook with AirPods as the default input (setting "Use the Mac's own mic
   when Bluetooth headphones are connected" on), record, then close the lid
   onto an external display and keyboard, keep talking for 30 s, stop.
   Expect in `meet-ai.log`: `microphone stream lost: ... opening a new segment`
   (or `the microphone track has written nothing for N ms`), a second segment
   with `"reason":"stream_restart"` in `segments.json`, and your voice in
   `mic.wav` after the lid closed (now from the AirPods mic).
   Why skipped: needs the hardware; whether macOS removes the built-in mic
   when the lid closes on a dock is the ticket's claim, verify it.
2. Record while a USB interface whose default input format is I32 (or U16,
   F64) is the default input; on Linux, any USB or pro interface reporting
   S32.
   Expect: the log's `microphone device rate ... I32` line, and `mic.wav`
   with speech at a normal level (not 0 dB clipping, not near silence).
   Why skipped: needs such a device.
3. Record to a meetings folder on a slow disk (a USB 2 stick, or an SMB share)
   with a big copy running to it, for 5 minutes.
   Expect: no `the capture ring was full` warning, or if there is one, the
   same duration in `mic.wav` and `system.wav` (the lost frames are silence,
   not missing). Checkpoint failures are logged and retried; the recording
   only ends if none succeeds for 30 s.
   Why skipped: needs the running app and a slow disk.
4. Unplug a USB microphone (the default input) mid-recording, with the Mac's
   own mic still present.
   Expect: a new segment within a tick or two (the default input change, or
   `stream_restart`), and the recording goes on with the built-in mic.
   Why skipped: needs the hardware.
5. Kill `coreaudiod` (`sudo killall coreaudiod`) mid-recording.
   Expect: within one checkpoint (5 s) a `has written nothing for` warning and
   a `stream_restart` segment; the recording goes on once Core Audio is back,
   or ends with a microphone error if it does not come back within the
   first-buffer wait (10 s).
   Why skipped: needs `sudo` and the running app.
6. On Windows and Linux, record 2 minutes with nothing playing.
   Expect: no `stream_restart` segment. A loopback with no silence keepalive
   (the log says `no silence keepalive`) is never judged stalled, because it
   sends nothing while nothing plays.
   Why skipped: needs a Windows or Linux machine.

## TUR-180 suggestions (`docs/findings/tur-180-silent-system-track.md`)

Not applied here: the owner's answer (Q1) was to leave suspects A and C to a
hardware run, with no behaviour change in this PR.

- Suspect C, the check chime while another app is on the mic: skipping the
  chime during a call contradicts SPEC A7 ("plays every recording") and A25's
  during-recording check, and the findings mark the suspect unconfirmed until
  their reproduction plan runs.
- Suspect A, the aggregate device on a Bluetooth output in call mode: building
  the aggregate with no main sub-device, or delaying the system track, cannot
  be verified headless and could leave the tap silent for every Bluetooth
  output.
- Suspect B (mic choice during a call) was not in this task's list.
- What this PR does add for that incident: a system track whose IO proc stops
  is now noticed at the next checkpoint and reopened. A tap that keeps running
  but delivers exact zeros (the incident's `system.wav`) is not detected here.

### Reproduction plan for suspects A and C (from the findings)

Needs a Mac, a Bluetooth headset as both default input and output, a second
phone or account for a WhatsApp or FaceTime call where the other side talks
or plays music the whole time, and a dev build. For each run note: can you
hear the other person before and after, the `system tap input rate changed`
lines in `meet-ai.log`, and `system.wav`'s loudness per second
(`ffmpeg -i system.wav -af astats=metadata=1:reset=1,ametadata=print:key=lavfi.astats.Overall.RMS_level -f null -`).

0. Baseline, no recording: call for 60 s. The other side stays audible and
   the headset stays at its call rate (Audio MIDI Setup). If it goes silent
   here, meet-ai is not the cause; stop.
1. Full recording, as in the incident: call, wait 10 s, press Record. Three
   times; count the runs that go silent.
2. Suspect C off: during the call run
   `cargo run -p audio --bin meet-rec -- --out <dir> --duration 60`, which
   starts the same mic, tap and aggregate but never plays the chime. Audible
   here but silent in run 1 points at C.
3. Suspect A off: a local, uncommitted patch with `let sys = None;` at
   `src-tauri/src/recording.rs` (the system source) and `check.spawn(app)`
   skipped. Audible here points at A (or A with C); compare with run 2.
4. Suspect B: turn off "Use the Mac's own mic when Bluetooth headphones are
   connected" and repeat run 1; then run 3's patch with it back on.
5. Wired or built-in output, headset disconnected: repeat run 1. Audible here
   means the problem is specific to Bluetooth call mode.

The findings' "Recommended fix per suspect" section says what to change once a
run points at one.

## Known

- The Linux loopback keeps its own `stream_is_lost` copy
  (`platform/linux/loopback.rs`); the shared one is
  `loopback::cpal_stream::stream_is_lost`. Not merged because the Linux build
  cannot be checked on this Mac; CI's `rust (linux)` job covers the rest.
- A source that stalls again after every reopen gets a new segment at every
  checkpoint (every 5 s), each costing the microphone a restart. Not seen in
  the tests here; watch for repeated `stream_restart` segments in check 5.
