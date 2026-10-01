# Manual checks: wave1-c-crates

Phase 4 (audio + stt crate internals) and the meeting-format constants.

## Run by hand

1. `just dev` (or a signed build), record 1 minute with mic + system audio, stop.
   Expect: `<meeting>/audio/mic.wav`, `system.wav`, `segments.json`, `transcript.md` and `notes.md` exist; live transcript lines appear.
   Why skipped: needs mic, system audio and a window.
2. Change the meetings folder in Settings.
   Expect: `.app/models` moves with it and downloaded models still show as installed.
   Why skipped: needs the running app.
3. `cargo test -p audio --ignored` and `cargo test -p stt --ignored` on a Mac with permissions.
   Why skipped: ignored tests need real devices or a downloaded model.

## Not done here

- Error types: `audio::Error` and `stt::Error` already use `thiserror`; left unchanged because src-tauri matches the variants.
- `cargo udeps` not installed; unused deps found by grep: audio `dirs`, `earshot`, `insta`; stt `insta`.
