# TUR-177 manual checks

A pure refactor: the checks below confirm nothing changed on hardware the
headless tests cannot reach.

## 1. System audio on Windows

- Run: on a real Windows PC, record a 1-minute call with audio playing to the default output, then again with the call on a headset that is not the default (TUR-95's follow).
- Expected: `system.wav` has the call audio at normal speed in both, the log shows `system loopback: asking for ... buffers` once per segment, and a default-device change still opens a new segment.
- Skipped: needs a real Windows machine. The WASAPI backend now calls `loopback::cpal_stream::start_silence` / `start_capture` (the Linux path); `cargo check` and `cargo clippy --target x86_64-pc-windows-msvc -p audio --all-targets` pass here. The shared capture also converts U16 and F64 mix formats, which the old Windows copy refused; WASAPI's shared-mode mix format is F32, so verify it on the PC.

## 2. System audio on Linux

- Run: on a real Linux desktop (PipeWire and PulseAudio), record a call and unplug the headset mid-call.
- Expected: same recording as before; the stream loss still opens a new segment (`system loopback stream lost`).
- Skipped: needs Linux hardware. `on_stream_error` now uses the shared `cpal_stream::stream_is_lost`, which has the same two kinds (unit-tested in `cpal_stream.rs`).

## 3. macOS tap, device list and headphone warning

- Run: a signed build on a Mac. Record a call through AirPods (HFP 16 kHz), open Settings, Microphone, and plug in wired headphones.
- Expected: `system.wav` at normal speed (rates log line unchanged), the input device list as before, the headphone warning as before.
- Skipped: needs a signed build and real devices. Every Core Audio read now goes through `macos/props.rs` (size-checked); the property smoke tests (`device_watch`, `input_devices`, `headphones`, `activity`, `props`) pass on this Mac.

## 4. Whisper and Parakeet transcripts

- Run: transcribe the same meeting with `"engine": "whisper"` and `"engine": "parakeet"`, batch and live.
- Expected: the same lines and timestamps as before this change.
- Skipped: needs model files, which are never downloaded here. The shared span pipeline (`crates/stt/src/span_driver.rs`) is unit-tested with a fake decode.
