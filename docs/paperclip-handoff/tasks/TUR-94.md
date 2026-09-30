# TUR-94 — Phase 2b-1 — lift the meet-rec record loop into crates/audio as a start/stop session

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | high |
| Owner | Vox |
| Created | 2026-09-28 07:08 UTC by Alen |
| Completed | 2026-09-28 08:45 UTC |
| Parent | [TUR-92](TUR-92.md) Phase 2b — wire the real recorder and live transcription behind the Record button |

## Description

The app cannot call the recorder today because the recorder is not callable. The whole capture loop is `fn record()` inside the **binary** `crates/audio/src/bin/meet-rec.rs:380` — it is private, it blocks until a `--duration` flag runs out, and nothing in `crates/audio/src/lib.rs` exposes it. `src-tauri` already depends on `audio`, but only reaches `permission_check`.

This is the first ticket because the shape chosen here decides every ticket after it.

#### Scope

- Move the loop out of the binary and into the library as a session the caller controls: start it, ask it how it is doing, stop it. Not a blocking call with a duration.
- Keep everything the loop already does correctly — segment reopen on a device change (`reopen_segment`, L254), the drift checkpoints (`checkpoint`, L341), the gap padding (`align_and_pad`, L194), the first-position wait (L165). This is a move, not a rewrite.
- `meet-rec` stays, as a thin caller over the same session. The CLI and the app must not be able to drift apart — that is the point of doing it this way rather than shelling out to the binary from the app.
- Stop must be clean: WAV headers finalised, `segments.json` written, no half-flushed track.

#### Why not shell out to the binary

It would work for a week. Then the app needs live samples for transcription (2b-3), and a subprocess boundary means piping audio across it or reading a growing file — which is exactly the question TUR-31 already answered for `stt`. Better to have one in-process seam now.

#### Gate

- `meet-rec` behaves the same as before: existing `-p audio` tests pass, and one manual `just rec` run still produces the same three files.
- A new test starts a session against the stub audio source, stops it, and gets back complete WAVs and a valid `segments.json`.
- `just check` clean, including the Windows `stub-audio` cross-check (SPEC §8.2 — the loop must not take a mac-only dependency on its way out of the binary).

## Commits that mention this task

- `79357df` 2026-09-28 — TUR-94: lift meet-rec's record loop into crates/audio as a start/tick/stop session

## Comments (1)

### Vox · 2026-09-28 08:45 UTC

> Done. Moved the capture loop out of `meet-rec.rs` (the binary) into `crates/audio/src/session.rs` as `RecordingSession` — a start/tick/stop lifecycle the caller drives, instead of a blocking call bounded by `--duration`.
>
> **Shape**
> - `RecordingSession::start(dir, mic, sys)` takes the two `AudioSource`s already built (mic always required, `sys` optional) — so `meet-rec` and, later, the app build sources the same way (via the new `audio::session::default_system_source()`) and cannot silently diverge on what "the real recorder" means.
> - `tick()` is one poll-loop iteration: macOS device-change reopen, and a checkpoint if 5s elapsed since the last one. The caller owns the cadence (meet-rec still polls every 200ms).
> - `status()` is the read-only "how's it doing" query.
> - `stop()` finalizes cleanly: both channels stopped (WAV headers patched), a final anchor latched, `segments.json` written — same contract §7 order as before.
> - `meet-rec.rs`'s `record()` is now about 40 lines: build real sources, start the session, loop calling `tick()`, stop, print the report. All the alignment/checkpoint/reopen logic moved verbatim (same functions, same doc comments), nothing rewritten.
>
> **Gate verification**
> 1. `cargo test -p audio` — all 75 unit tests plus integration tests pass unchanged.
> 2. Manual `just rec`-equivalent run against real hardware (`cargo run -p audio --bin meet-rec -- --out <dir> --duration 3`): produced mic.wav (51840 frames), system.wav (51156 frames), a valid segments.json with anchors, drift measured at 103.9ms/18.7ms — same three files, same behavior.
> 3. New tests in session.rs: a_session_against_stub_sources_produces_complete_wavs_and_valid_segments and a mic-only variant, both against a hardware-free StubSource that writes real WAV bytes via the crate's own WavWriter — start, tick, stop, then assert both WAV headers declare real frames and segments.json parses and satisfies the header invariant. No Core Audio, no mic TCC grant needed.
> 4. just check's Rust-relevant pieces all clean: cargo fmt -p audio --check, cargo clippy -p audio --all-targets -- -D warnings, cargo check --target x86_64-pc-windows-msvc -p audio (compiles; the only new warning is a pre-existing-shape dead-code warning on reopen_segment, same as before the move since it's still only called from the macOS-gated branch of tick() — not a regression). Also ran cargo check --workspace to confirm nothing else in the tree broke. Did not run the full just check (pnpm/biome/vitest/sidecar) since this ticket touched no frontend code and the workspace is shared with a concurrent run touching src/routes/Review.tsx.
>
> src-tauri's Recorder is untouched — wiring the app to RecordingSession is TUR-92's next child ticket, not this one.
>
> Commit: 79357df on docs/releasing-guide (this branch was already checked out for the run).
