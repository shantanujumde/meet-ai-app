# Changelog

All notable changes to meet-ai are recorded here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
this project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Distribution is **personal only** at this stage — SPEC.md §8.1 puts public
release in v2. Builds are signed with a self-signed local identity, not a
Developer ID, and are not notarized.

## [Unreleased]

## [0.3.0] — 2026-09-30

The Record button now records for real, and the permission check in
onboarding measures real access instead of always saying "not checked".

### Added

- Recording from the app. The Record button (and ⌘⇧R) opens the microphone
  and the system-audio tap and writes `mic.wav`, `system.wav` and
  `segments.json` into the meeting folder; stop closes both cleanly. The
  capture loop moved out of the `meet-rec` binary into
  `audio::session::RecordingSession`, which `meet-rec` now also uses.
  Starting and stopping run off the UI thread, and a double-tapped shortcut
  is ignored rather than opening a second tap.
- Start refuses with a clear reason when permission is denied, and removes
  the empty meeting folder it would otherwise leave. A tap that fails to
  open is reported as an error instead of sitting in "Recording" with
  nothing written.
- The onboarding permission check is real. System audio is checked by
  playing the permission chime and hearing it back through the tap; the mic
  is checked with `AVCaptureDevice.authorizationStatus` first, so a stored
  "Denied" is caught even though the mic still opens and returns audio.
- A "Change…" button next to the meetings folder, in Settings and in the
  onboarding folder step. Picking a new folder moves every existing meeting
  into it (merging rather than overwriting if the folder already has
  something in it); nothing is left behind at the old location. Refused
  while a recording is in progress.
- `RELEASING.md`: the release steps, signing-keychain unlock, and recovery
  for the "Keychain Not Found" dialogs.

### Fixed

- The window can be dragged by its titlebar again. CSS
  `-webkit-app-region: drag` does nothing in Tauri's macOS webview, so the
  titlebar now uses `data-tauri-drag-region`.
- A user who had finished onboarding could be left on a stale setup page
  when the window was refocused. They are now sent back to the app.
- With the transcription engine set to Auto, the message now says why
  Apple's engine could not be used (sidecar missing, OS too old, on-device
  model not installed) instead of only "unavailable".

## [0.2.0] — 2026-09-28

Recording now survives a device switch mid-call, and the transcription
engine is chosen from `config.jsonc` instead of being hardcoded.

### Added

- Mid-recording device changes (for example, swapping to AirPods) close the
  current segment and open a new one, instead of writing one segment for the
  whole call. `meet-rec` polls the default input and output devices on its
  existing 200 ms loop and rebuilds both audio sources on a change. Verified
  on real hardware.
- `WavWriter::open_append`, so a new segment continues the same per-channel
  WAV file rather than truncating it.
- `drift-check` binary: reads `segments.json` and the WAV headers, reports
  per-channel drift, skew and boundary gaps, and exits 0/1/2 for
  pass/fail/not-measurable (SPEC §6).
- `transcription.engine` and `transcription.model` are read from
  `config.jsonc` (SPEC §3.5). A missing or broken file falls back to the
  defaults; an unknown engine name is logged and rejected rather than
  silently treated as `auto`.
- `offline_meeting` example, which runs the full transcription path over a
  copied meeting folder with one engine forced, to prove it works with the
  network off.
- `just check-whisper` documented in CONTRIBUTING.md as the manual gate for
  the real-model whisper tests, with when to run it.

### Fixed

- A force-quit could leave the WAV headers declaring more frames than
  `segments.json` accounted for. The writer now freezes the synced frame
  count at fsync time and the header only ever declares that count.
- Segment reopening read the mic position before stopping it, which could
  reproduce the same header-ahead race one layer up.

### Changed

- Whisper word-error-rate threshold tightened from 30% to 15% now that it has
  been measured (3.2% on `mic.wav`, 0.0% on `system.wav` with
  `small.en-q5_1`).
- The whisper silence-hallucination guard has now been run against the real
  model and holds without changes (SPEC amendment A9).

## [0.1.0] — 2026-09-27

First tagged release. Audio capture and local speech-to-text are built and
tested against real hardware; the app shell renders over them. Calendar,
Claude Code analysis and the MCP server are not in this release — see
"Not in this release" below.

### Added

**Audio capture (`crates/audio`, SPEC §2.2)**

- `AudioSource` trait with the macOS implementations behind it, keeping the
  Windows seam open from day one (SPEC §8.2).
- `MicSource` — real microphone capture through cpal, resampled to 16 kHz and
  written straight to WAV. Verified closed-loop against real hardware.
- `SystemSource` — system-audio capture via a Core Audio process tap, ported
  from the Phase 0a Swift spike. Verified against real hardware.
- Device-rate → 16 kHz resampler shared by both capture paths.
- Crash-safe incremental WAV writer, with checkpointing split into an fsync
  step and a header-patch step so a `kill -9` leaves a playable file.
- `segments.json` reader and writer — the on-disk drift contract (SPEC
  amendment A5), plus 18 committed fixtures covering drift, refusal and
  mid-recording device switches.
- A recording whose anchors stop before its audio does is refused rather than
  silently accepted.
- `meet-rec` CLI — orchestrates mic and system-audio capture end to end with
  no Tauri and no UI.

**Speech-to-text (`crates/stt`, `sidecar/meet-stt`, SPEC §2.6)**

- `SttEngine` trait with two implementations: Apple's on-device
  `SpeechTranscriber` via the Swift sidecar, and a whisper.cpp fallback.
- Live session seam — chunked transcription over audio that is still
  arriving, with the VAD state machine split out so live and batch paths
  agree on where an utterance ends.
- Silence gate that holds on the streaming path, guarding against whisper's
  hallucination-on-silence failure (SPEC §7, Phase 1 gate).
- `live_replay` example — replays a recorded meeting through the live session
  API as NDJSON, so UI work does not need a microphone or a model.
- Engine probe (`just stt-probe`) that decides offline between the Apple
  engine and the whisper fallback.

**Model download (`crates/modelfetch`)**

- Lazy, resumable, SHA-256-verified downloader for the pinned whisper models,
  with a `meet-stt-model` CLI (`just models`, `just model`).

**App shell (`src/`, `src-tauri/`, SPEC §2.7)**

- Tauri 2 + React 19 shell over the Rust workspace: sidebar, meetings list,
  review view, notes pane and record control.
- `/onboarding` route with the three steps SPEC §8.1 requires — permission,
  model download, meetings folder — including the denial path with a retry
  button and a System Settings deep link.
- Menu bar tray with rasters that invert correctly in light and dark.
- Typed IPC layer with a shared error taxonomy between Rust and TypeScript.
- Design-system tokens and app stylesheet carrying the meet-ai identity.

**Permissions and signing (SPEC §2.9, §8.1)**

- Positive-control permission check: a ~200 ms tone is played from meet-ai's
  own process and confirmed back through the tap. Return codes and RMS floors
  are both proven not to work on a denied tap (FINDINGS §10.1–10.2, §10.7).
- `just sign` — inside-out bundle signing that never uses `--deep`, gives
  nested helpers no entitlements, and fails loudly when a declared
  `externalBin` is missing from the bundle.
- Signing identity reads from `$SIGN_IDENTITY`, so a Developer ID build in v2
  is one environment variable and no code change.
- Bundle identifier fixed at `pro.saleschat.meetai` and the updater public key
  compiled in with `active: false` — both irreversible after the first
  install, both done now.

**Repo**

- `just check` as the single health gate: rustfmt, clippy with `-D warnings`,
  the full cargo test suite, the Windows cross-check seam, biome, tsc and
  vitest.
- Apache-2.0 license with a NOTICE file.

### Fixed

- The app window painted `transparent` because `--surface-canvas` was
  referenced but never defined. It is now an opaque token in both themes; the
  native material still shapes corners, shadow and active-state dimming, but
  is no longer asked to carry body-text contrast.
- App icon no longer nests the tile on macOS 26 — the 16pt and 32pt `.icns`
  representations are dropped, with a proxied legacy downscale for older
  systems.
- Traffic lights sit inside the window via `titleBarStyle: "Overlay"`.
- `NSUUID` crash in the system-audio tap.
- Nested helper binaries no longer inherit the app's microphone entitlement.
- Bundle signing no longer leaves a half-signed bundle that passes
  `codesign --verify --deep`.

### Changed

- macOS floor raised to 26.0 (SPEC amendment A8).
- Capture resolved to in-process Rust rather than the Swift sidecar SPEC §5
  had pre-committed to (SPEC amendment A6).

### Not in this release

Calendar integration, Claude Code analysis, ticket generation, the MCP server,
notarization, and Windows support. See SPEC.md §7 for the phase order and §8
for the seams each of those lands on.

[unreleased]: https://github.com/shantanujumde/meet-ai-app/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/shantanujumde/meet-ai-app/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/shantanujumde/meet-ai-app/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/shantanujumde/meet-ai-app/releases/tag/v0.1.0
