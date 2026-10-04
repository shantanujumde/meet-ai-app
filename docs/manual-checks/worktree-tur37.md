# Manual checks: TUR-37 (Windows recording: system audio + mic through cpal loopback)

These need a real Windows 10/11 PC with speakers or headphones, a microphone,
a headset to plug in, and a real call, so none were run here. They are the
Windows run of SPEC §5's Phase 0 gate (drift < 200 ms, survives a headset
switch, survives a hard kill) for the parity gate (TUR-43, TUR-69).

What is covered without a device, in every `cargo test -p audio` on macOS,
Windows and Linux:

- `crates/audio/src/loopback/clock.rs`: QPC ticks to ns on cpal's 100 ns grid,
  frame/ns conversions, and the gap rule (a packet starting more than one
  previous packet, and at least `GAP_THRESHOLD_MIN_NS` = 10 ms, after the
  previous one ended is a gap; jitter, early, missing and backwards times are
  not), capped at `MAX_GAP_FILL_NS`.
- `crates/audio/src/loopback/capture.rs` and `splice.rs`: a gap and a full
  ring both become silence at the right sample, on a frame boundary; gaps do
  not read as a slow device to the rate meter.
- `crates/audio/src/loopback/silent.rs`: a buffer flagged silent is
  zero-filled; NaN and infinity become 0.
- `crates/audio/src/loopback/follower.rs`: a new default endpoint counts only
  after `DEVICE_SWITCH_CONFIRMATIONS` (2) reads in a row agree, read every
  `DEVICE_CHECK_INTERVAL` (2 s); a one-read flap, a failed read and a stale
  watch are handled.
- `crates/audio/src/loopback/buffer.rs`: about 80 ms capture buffers.
- `crates/audio/src/loopback/source/tests.rs`, over a fake backend: the
  silence keepalive starts before the capture and stops with it, a failed
  keepalive does not fail the recording, a failed capture stops the
  keepalive; 0.5 s and 60 s gaps come out as silence of the right length;
  silent buffers are written as zeros; `position()` pairs the latest capture
  time with the frames written and matches the header after stop; the tee
  and the head-pad match the WAV; a reopen appends to the same file.

On the `rust (windows)` CI job only: `crates/audio/tests/windows_loopback.rs`
opens the real WASAPI loopback on the default output device. A runner with no
audio endpoint prints `SKIPPED: this machine has no usable audio output
device (...)` and passes; with an endpoint it records 3 s with nothing
playing and checks the track keeps the capture-time clock. The unit tests in
`crates/audio/src/platform/windows_devices.rs` check the QPC clock and that
the default-device reads do not panic. Which case the runner hit is in the
PR's CI log. On this Mac the Windows files were only type-checked and
linted (`cargo clippy --target x86_64-pc-windows-msvc -p audio`).

Not done in this PR: `AUDCLNT_BUFFERFLAGS_SILENT` is not visible through
cpal 0.18.2, so the zero-fill is wired but never triggered on Windows today
(TUR-43 measures whether it matters). Loopback follows the default render
endpoint only; following a non-default endpoint that a meeting app plays to
(anarlog's `render_device_in_use`) is a follow-up ticket (A1).

Setup for every check: a Windows 10 (1703 or later) or 11 PC, the repo at
this branch, Rust from `rust-toolchain.toml`, and in a terminal at the repo
root `cargo build -p audio --bins`. `meet-rec` and `drift-check` below are
`target\debug\meet-rec.exe` and `target\debug\drift-check.exe`.
`meet-rec` prints no `tracing` log, so the checks read the files it writes.
The log lines quoted (in backticks) show in the app's `meet-ai.log` when the
same recording is made with the app's Record button instead.

## 1. 10-minute drift with a real call

1. Speakers or wired headphones as the default output, a microphone as the
   default input (Settings → System → Sound).
2. Join a real call (Teams desktop, Zoom, or Google Meet in Edge) with someone
   talking, or play a long YouTube video with speech next to the mic.
3. `meet-rec --out C:\tmp\tur37-drift --duration 600`. Talk now and then.
4. Expected: `C:\tmp\tur37-drift` holds `mic.wav`, `system.wav` and
   `segments.json`, whose segment has `sys_rate` 16000 (not 0, which means
   mic-only) and `sys_device_rate` the speakers' rate (usually 48000). In
   the app's log: `system loopback: <device> at 48000 Hz, 2 ch` and
   `silence keepalive running`.
5. `drift-check C:\tmp\tur37-drift`. Expected: exit code 0, both channels
   under 200 ms.
6. Open both WAVs in Audacity: `system.wav` has the call's voices, `mic.wav`
   yours, both about 10:00 long, and a clap you make mid-call lines up in
   both to within 0.2 s.

## 2. Nothing playing still records a full-length system track

1. Close every app that plays sound. `meet-rec --out C:\tmp\tur37-quiet
   --duration 60`.
2. Expected: `system.wav` is 60 s of silence (not shorter, not missing), and
   `drift-check C:\tmp\tur37-quiet` exits 0.
3. In the app: the log has `silence keepalive running`. If it says `no
   silence keepalive` instead, the track must still be 60 s long, with
   `ms the device did not deliver, written as silence` lines.

## 3. Headset plugged in and out mid-recording

1. Start with the laptop speakers as default output and mic as input.
   `meet-rec --out C:\tmp\tur37-switch --duration 180` while a video plays.
2. At about 0:40 plug in a USB or Bluetooth headset that Windows makes the
   default (output and input). At about 1:40 unplug or disconnect it.
3. Expected: `segments.json` has 3 or more segments, each starting within
   about 4 s of a change with reason `default_output_device_changed` or
   `default_input_device_changed`, and the new segments carry the new device's
   `sys_device_rate` (a Bluetooth headset in a call: 16000 or 32000; music:
   44100 or 48000).
4. `drift-check C:\tmp\tur37-switch`: exit code 0. `system.wav` has the video
   for the whole 3 minutes, with at most a short gap at each switch.
5. Repeat with a Bluetooth headset during a Teams call (the hands-free
   profile): the recording keeps both tracks and does not end. In the app's
   log: `default output device changed — reopening segment`.

## 4. Hard kill

1. `meet-rec --out C:\tmp\tur37-kill` (no duration) with a video playing.
2. After about 45 s, kill it from another terminal:
   `taskkill /F /IM meet-rec.exe`.
3. Expected: `segments.json` is valid JSON with anchors up to the last 5 s
   checkpoint, both WAVs open in Audacity with about 40 s or more of audio,
   and `drift-check C:\tmp\tur37-kill` measures (exit code 0 or 1, never a
   crash). Then the app's recovery (TUR-97) lists the meeting as interrupted
   when it is next opened.

## 5. Teams desktop is recorded (endpoint loopback, not per-process)

1. A Teams desktop call (the new Teams, `ms-teams.exe`) with someone talking.
2. `meet-rec --out C:\tmp\tur37-teams --duration 60`.
3. Expected: `system.wav` has the other side's voice, not silence.

## 6. Follow-up to check before the parity gate (not built here)

Teams or Zoom set to play on a device that is not the Windows default (for
example the headset while the laptop speakers stay default). Expected today:
`system.wav` is silent, because loopback follows the default endpoint only.
That is the follow-up ticket for anarlog's `render_device_in_use` (A1); note
the result there.
