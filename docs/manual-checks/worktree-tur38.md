# Manual checks: TUR-38 (Linux recording: system audio + mic through cpal's PipeWire host)

These need a real Linux desktop (Ubuntu 24.04 with PipeWire first) with
speakers or headphones, a microphone, a headset to plug in and a real call,
so they were not run on hardware here. They are the Linux run of SPEC §5's
Phase 0 gate (drift < 200 ms over 10 minutes, survives a headset switch,
survives a hard kill) for the Linux parity gate. Every question the TUR-43
spike was meant to answer is marked "confirm with TUR-43".

## What was run, and where

On this Mac: `cargo test -p audio`, `cargo clippy -p audio --all-targets -D
warnings`, `cargo fmt --all --check`, `cargo check --target
x86_64-pc-windows-msvc -p audio --all-targets`.

In an Ubuntu 24.04 container (Docker on this Mac, arm64, the apt packages
CI installs plus `pipewire pipewire-pulse wireplumber`): `cargo build`,
`cargo clippy --all-targets -D warnings` and `cargo test` for `-p audio`,
with and without `--features stub-audio`. Then `meet-rec` against a real
PipeWire 1.0.5 + WirePlumber + pipewire-pulse running in the container, with
`module-null-sink` virtual sinks and a virtual source as the devices (no
sound card), `paplay` playing a 440 Hz tone:

- PipeWire host, 26 s with a tone three times, the default sink switched at
  about 7 s and the new sink removed while default at about 22 s:
  `system.wav` has the tone in exactly the three places, `segments.json` has
  3 segments (`start`, then `default_output_device_changed` twice: the
  switch and the removal), `drift-check` PASS, worst 55 ms.
- `kill -9` 10 s into a recording: both WAVs open (Python `wave`, 10.3 s
  each, the tone in the system track), `segments.json` has the anchors up to
  the last checkpoint, `drift-check` PASS, worst 53 ms.
- PulseAudio host (pipewire-pulse only, the PipeWire socket hidden from
  `meet-rec`): both tracks, `drift-check` PASS, worst 160 ms. Before the
  20 ms fragment fix in this PR it was 2.2 s (PulseAudio's default record
  fragment).
- No sound server (cpal falls back to ALSA, the container has no card):
  `meet-rec` fails with "no usable input config"; ALSA's `null` device is
  the only one listed and is never opened as a microphone.
- PipeWire host, 10 minutes, a tone every 20 s, a default-sink switch at
  5 min: 2 segments, `drift-check` PASS, worst 79 ms (system 45 ms, mic
  79 ms), both WAVs 600.2 s. The first run, with a 100 ms gap
  floor, lost 1 ms of system audio per second (-387 ms at 10 min): the
  container's PipeWire skips about one 21 ms cycle a minute idle and more
  under load (no real-time scheduling there), and those skips were not
  filled. PipeWire's capture times are exact, so a skip over half a packet
  is now silence-filled.

The container's numbers are not a desktop's: no RT scheduling, virtual
devices, one shared clock. Measure them on hardware.

## Setup for every check

Ubuntu 24.04 desktop (GNOME, PipeWire as shipped), the repo at this branch,
Rust from `rust-toolchain.toml`, and:

```sh
sudo apt-get install -y libasound2-dev libpulse-dev libpipewire-0.3-dev clang libclang-dev
cargo build -p audio --bins
```

`meet-rec` and `drift-check` below are `target/debug/meet-rec` and
`target/debug/drift-check`. `meet-rec` prints no `tracing` log, so the
checks read the files it writes; the quoted log lines are in the app's
`meet-ai.log` when the same recording is made from the app.

## 1. 10-minute drift with a real call

1. Speakers or wired headphones as the default output, a microphone as the
   default input (Settings, Sound).
2. Join a real call (Zoom, Teams or Google Meet in Firefox or Chrome) with
   someone talking, or play a long video with speech.
3. `meet-rec --out /tmp/tur38-drift --duration 600`. Talk now and then.
4. Expected: `segments.json` has `sys_rate` 16000 (not 0, which means
   mic-only) and `sys_device_rate` the sink's rate (usually 48000). In the
   app's log: `system loopback: <sink> (PipeWire sink_default) at 48000 Hz`
   and `silence keepalive running`.
5. `drift-check /tmp/tur38-drift`. Expected: exit code 0, both channels
   under 200 ms.
6. In Audacity: `system.wav` has the call, `mic.wav` you, both 10:00 long,
   and a clap lines up in both to within 0.2 s.

## 2. Nothing playing still records a full-length system track

1. Close every app that plays sound. `meet-rec --out /tmp/tur38-quiet
   --duration 60`.
2. Expected: `system.wav` is 60 s of silence, not shorter, and
   `drift-check` exits 0.
3. Confirm with TUR-43: does an idle sink keep calling back without the
   keepalive? The code does not rely on it (zeros are rendered, and any
   stretch with no callbacks becomes silence by the capture times).

## 3. Headset plugged in and out mid-recording

1. Laptop speakers as default. `meet-rec --out /tmp/tur38-switch --duration
   180` while a video plays.
2. At about 0:40 connect a USB or Bluetooth headset that GNOME makes the
   default; at about 1:40 disconnect it.
3. Expected: 3 or more segments, each within about 4 s of a change, reason
   `default_output_device_changed` (or `default_input_device_changed`), the
   new segments carrying the new device's `sys_device_rate`. `drift-check`
   exit code 0, and the video in `system.wav` throughout with at most a short
   gap at each switch.
4. With a Bluetooth headset in a call (HFP): the recording keeps both tracks
   and does not end. Confirm with TUR-43 what rate PipeWire runs the sink at.

## 4. Hard kill

1. `meet-rec --out /tmp/tur38-kill` (no duration) with a video playing.
2. After about 45 s: `pkill -9 meet-rec`.
3. Expected: `segments.json` valid with anchors to the last 5 s checkpoint,
   both WAVs open in Audacity with 40 s or more, `drift-check` measures (exit
   0 or 1, never a crash), and the app lists the meeting as interrupted.

## 5. PulseAudio-only distro (no PipeWire)

1. A distro or VM with PulseAudio and no PipeWire (Ubuntu 22.04, or
   `systemctl --user mask pipewire pipewire-pulse` and install pulseaudio).
2. Checks 1 and 4 again. Expected: the log says `(PulseAudio
   <sink>.monitor)`, both tracks, `drift-check` exit 0. The container got
   160 ms here, the closest to the gate of any run: measure it, and confirm
   with TUR-43 whether cpal's PulseAudio latency estimate is good enough.

## 6. ALSA only, and the ALSA `null` device

1. With no sound server running (`systemctl --user stop pipewire.socket
   pipewire pipewire-pulse wireplumber`), `meet-rec --out /tmp/tur38-alsa
   --duration 10`.
2. Expected: a microphone-only recording from the real card (system audio
   is absent, `sys_rate` 0), or, if ALSA's default resolves to the `null`
   device, `meet-rec` refuses with "the default input is ALSA's null device"
   instead of recording silence at full CPU.

## 7. Packaging (for the release ticket)

The AppImage or .deb must not bundle `libpipewire-0.3.so*` or `libspa-*`:
they load plugins from a build-time path and capture silence on other
distros (anarlog #7549). Check the bundle with `ldd` and `ls` on its `lib`
folder. No Flatpak: there is no audio-capture portal (xdg-desktop-portal
#957).

## 8. Follow-ups noticed (not built here)

- A meeting app playing to a sink that is not the default (anarlog's
  `sink_in_use`) records silence today; the loopback follows the default
  sink only.
- cpal's `realtime` feature (RT scheduling for the PipeWire callback thread)
  is off. Missed cycles become silence, so the track keeps time, but each
  one is a 21 ms hole in the audio. Measure how often it happens on a
  desktop (confirm with TUR-43).
- The session's head-pad takes each channel's current position as its
  first, which carries whatever the channel already wrote while the other
  one was starting (40 to 150 ms on PulseAudio here). Shared session code,
  not changed in this ticket.
