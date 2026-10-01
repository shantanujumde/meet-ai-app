# Manual checks: wave3-b-tests

Phase 7 (rest): `tempfile` in crate tests, `test-support` crate, split of long store/audio/stt test code.

## Run by hand

1. `cargo test -p audio --ignored` and `cargo test -p stt --ignored` on a Mac with permissions.
   Expect: pass as before; the closed-loop tests now use `tempfile::TempDir` for their output folders.
   Why skipped: ignored tests need real devices, permissions or a downloaded model. They compile and are listed as ignored in the normal run.

## Follow-up after Wave 3 Phase 3 (src-tauri is split)

Not touched here because Phase 3 is splitting `src-tauri` at the same time. These still use hand-rolled temp folders or tone code:

- `src-tauri/src/onboarding.rs` (test `meet-ai-ob-roundtrip-*` dir)
- `src-tauri/src/meetings.rs` (`meet-ai-move-*`, `meet-ai-state-*` helpers and many `remove_dir_all` cleanups)
- Check also `recording.rs`, `live_transcript.rs`, `live_transcript_e2e.rs`, `tickets.rs`, `engine.rs` for `temp_dir()` / `process::id()` patterns and sine generators.
- Swap them for `tempfile::TempDir` (add `tempfile.workspace = true` under `src-tauri` dev-dependencies) and `test-support` tone helpers.

## Not done here

- `crates/stt/src/replay.rs` voice-like signal (several mixed sines with an envelope) and the two-sine mixes in `audio` (`golden_bytes.rs`, `resample.rs`) stay local: they are not plain tones.
