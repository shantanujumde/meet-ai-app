# TUR-95 — Phase 2b-2 — put the real recorder behind the Record button

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **blocked** |
| Priority | high |
| Owner | Nia |
| Created | 2026-09-28 07:08 UTC by Alen |
| Parent | [TUR-92](TUR-92.md) Phase 2b — wire the real recorder and live transcription behind the Record button |
| Blocked because | Waiting on TUR-127 (in_progress) |

## Sub-tasks

- [TUR-115](TUR-115.md) **done** — TUR-95 gate: signed-bundle real recording + drift-check + permission-revoked check

## Description

This is the ticket that makes TUR-90 stop being true. Today `src-tauri/src/recording.rs` runs a correct start/stop state machine over nothing: its own header says it writes no audio and no transcript lines, and `Status.stub` is hardcoded `true` (L57).

#### Scope

- Replace the stub with the session from 2b-1. On start, open the tap and begin writing into the meeting folder that `recording.rs` already creates: `audio/mic.wav`, `audio/system.wav`, `segments.json` (SPEC §3.1).
- On stop, close the session cleanly and leave a folder that is complete without any later repair step.
- Delete `Status.stub` and every piece of UI copy that currently tells the user the recorder is a stub.
- Keep the transitions that already work. `Starting` and `Stopping` exist for exactly this moment — opening a Core Audio tap and flushing a WAV header both take long enough to see, and a double-tapped ⌘⇧R in that window must be ignored, not queued.
- Refuse to start when audio permission is absent, using the existing `permission::measure()` positive-control check rather than a return code (SPEC §8.1 — a denied tap returns `noErr` and delivers bit-exact zeros, so a return code cannot tell you). The controls should already be disabled in that state; this is the backstop.
- Surface a real failure to the user. If the tap will not open, say so on screen — do not sit in `Recording` with nothing being written, which is the exact shape of the TUR-90 report.

#### Gate

- In the **signed** bundle (`just bundle-signed` — `tauri dev` proves nothing about capture), press Record, talk for a minute with system audio playing, press Stop.
- Both WAVs are non-empty and play back with the right content on the right track.
- `cargo run -p audio --bin drift-check -- <meeting>/audio/` passes.
- Recording with permission revoked gives a clear on-screen reason and no empty meeting folder.

## Commits that mention this task

- `02b400d` 2026-09-28 — TUR-95: put the real recorder behind the Record button

## Comments (2)

### Nia · 2026-09-28 14:39 UTC

> TUR-115 (gate run) is done: steps 1-6 PASS on real hardware (signed bundle, real mic+system audio capture, drift-check worst 39.9ms well under the 200ms gate). Step 7 — the permission-revoked check that is explicitly part of this tick's own gate — FAILS: with Microphone explicitly toggled off in System Settings (confirmed denied, not just reset-to-unknown) and system-audio TCC entry removed, a fresh launch still went straight to Recording and wrote real, non-silent audio into a new meeting folder. No refusal, no on-screen reason.
>
> Tess filed TUR-127 (critical) with full repro and root-cause leads: either `crates/audio/src/permission_check.rs` is not correctly detecting a real TCC denial for the capture-stream code path, or the recording stream construction in `crates/audio` does not go through the same permission-checked path as the `permission::measure()` probe that `recording.rs:175` gates on. That second possibility would mean my guard in `recording.rs` is checking the right thing but the actual tap-open path ignores it — worth confirming once Rune has root-caused it, in case the fix also needs a change on my side.
>
> Rune is already checked out and actively running on TUR-127 (execution run `c269eab1`, phase `working`) as of this comment, so I am not duplicating that investigation or touching `recording.rs` concurrently with their run.
>
> Marking TUR-95 `blocked` on TUR-127 rather than `done`: the ticket's own gate explicitly requires the permission-revoked case to refuse cleanly, and it currently does not. Unblock owner: Rune (TUR-127). Unblock action: land the fix for the permission-denial detection / capture-path gap, then re-run TUR-95's gate step 7 to confirm before closing this out.

### Nia · 2026-09-28 12:23 UTC

> Implemented: recording.rs now drives audio::session::RecordingSession (TUR-94) instead of the stub. Status.stub and the stub UI copy are gone. Start refuses (with a clear reason, and no leftover meeting folder) when permission::measure()'s positive control comes back Denied; a tap-open failure surfaces as a real error instead of sitting in Recording with nothing written. toggle_recording/stop_recording run off a blocking thread (spawn_blocking), and the ⌘⇧R handler gets its own worker thread, so neither can freeze the app; the phase claim itself stays a fast mutex check so a double-tap is ignored, not raced. Committed as 02b400d.
>
> Verified automatically: cargo check/test/clippy/fmt -p meet-ai (37 tests pass, clean fmt/clippy on changed files), pnpm tsc --noEmit, pnpm vitest run (25 tests pass), pnpm biome check on changed files.
>
> Not verified by this run: the ticket's own exit gate (signed bundle, 1 minute of real mic + system audio, drift-check, permission-revoked path) — this sandbox has no codesigning identity (`security find-identity -v -p codesigning` shows 0 valid identities) and the gate needs a human physically pressing Record and talking. Filed TUR-115 for Tess with exact repro steps and blocked this issue on it.
