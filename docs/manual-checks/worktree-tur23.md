# Manual checks: TUR-23 (whisper error names the missing model)

The wording is covered headless by the tests in `crates/stt/src/registry.rs`
(both engines' error paths, and `Environment::discover_in` listing what is on
disk) and `src-tauri/src/engine.rs`
(`discovery_lists_installed_models_from_both_folders_for_the_error`, the app's
own discovery). The checks below need the running, signed app, so they were
not run here.

## 1. Whisper forced, a different model installed

1. Set `"transcription": { "engine": "whisper" }` (no `model`) in
   `~/Meetings/.app/config.jsonc`. Make sure only `small.en-q5_1` is in
   `~/Meetings/.app/models/`.
2. Start the app and start a recording.
3. Expected: live transcription fails with `config asked for the whisper
   engine but model large-v3-turbo-q5_0 is not downloaded (installed:
   small.en-q5_1)`. Settings' engine row shows the same sentence.

## 2. Whisper forced, nothing installed

1. Same config, empty `models/` folder.
2. Expected: `... model large-v3-turbo-q5_0 is not downloaded (none
   installed)`.

## 3. Auto, no Apple engine, a different model installed

1. `"engine": "auto"` on a build without the `meet-stt` sidecar (or a Mac
   that cannot run Apple's engine), only `small.en-q5_1` installed.
2. Expected: the "no speech engine is ready" message ends with `and whisper
   model large-v3-turbo-q5_0 is not downloaded (installed: small.en-q5_1) —
   download it below to continue`.

## Notes

The TS side had no copy of the old text (`grep` over `src/` for
"downloaded yet" / "no model is downloaded" found nothing), so no frontend
change was needed.
