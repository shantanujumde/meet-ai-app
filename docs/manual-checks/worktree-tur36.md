# tur36 manual checks

Windows and Linux could not be run from this Mac; GitHub Actions is the only
test bed for them (the `rust (windows)` and `rust (linux)` jobs in
`.github/workflows/check.yml`).

1. **The Windows installer opens to the meeting list.**
   Download the `meet-ai-windows-nsis` artifact from a green `check` run, run
   the `meet-ai_*_x64-setup.exe` on Windows 10/11, then start meet-ai.
   Expected: a normal (opaque, OS-framed) window opens on the meeting list.
   Recording controls stay usable only as far as the TUR-42 stubs allow: the
   permission check reports the system-audio track as not implemented on this
   platform yet, and nothing crashes.
   Skipped: needs a real Windows machine and a person to look at it.

2. **The Linux .deb opens to the meeting list.**
   Download `meet-ai-linux-deb`, `sudo apt install ./meet-ai_*_amd64.deb` on
   Ubuntu 24.04 (desktop), run `meet-ai`. Expected as in 1.
   Skipped: needs a Linux desktop session.

3. **The unsupported-recording copy reads well.** On either OS, open the
   record/permission UI. Expected: the stub message ("system-audio capture is
   not implemented on this platform yet") or equivalent, not a raw error. No new
   UI copy was added in TUR-36, because the TUR-42 stubs already return a
   readable message; if it reads badly in the running app, that is a follow-up.
   Skipped: needs the running app.

4. **macOS bundle unchanged.** `just bundle-signed` on a Mac still produces an
   app with the meet-stt sidecar and the vibrancy window. The per-OS Tauri
   files are only merged on Windows and Linux, so nothing should differ.
   Skipped: signing and running the app are out of bounds for this run.

Notes:
- `macOSPrivateApi` is left on in the merged Windows/Linux config on purpose:
  tauri-build fails the build when the `macos-private-api` Cargo feature on
  `tauri` (always on in `src-tauri/Cargo.toml`) does not match that key. It has
  no effect off macOS. A macOS-only feature does not build either: tauri-build
  checks every `tauri` entry, `[target.*]` tables included. Recorded in A13.
- Tests gated to macOS for TUR-50 (each file starts with
  `#![cfg(target_os = "macos")]`, R10's integration-test exemption, or asks
  the platform module):
  - `crates/audio/tests/golden_bytes.rs`: hashes of f32 synthesis taken on
    macOS; Linux and Windows libm round `sin` differently (each gives its own
    hash).
  - `crates/audio/src/resample.rs` `golden_output_bytes_at_48k_and_44k1`,
    `golden_downmix_stereo_bytes`: same reason, gated by
    `platform::F32_GOLDEN_HASHES`.
  - `crates/stt/tests/live.rs`, `crates/stt/tests/silence.rs`: the fixture
    WAVs are generated with macOS `say`.
- `src-tauri/src/sync/tests.rs`'s fake `claude`/`codex` scripts need
  `/bin/sh`; on Windows they compile now (through
  `platform::make_executable`) and TUR-54 makes them run.
- Tests that start a `/bin/sh` fake agent CLI (`agent::FakeHarness` or the
  `sync/tests.rs` scripts) start with `platform::skip_without_fake_cli!()`: off Unix it
  prints "skipped <module>: ..." to stderr and returns, so they pass without
  running on Windows (the CI log shows each skip); TUR-54
  makes them run there. 27 in `src-tauri/src`: 16 in `agent_run/tests.rs`,
  4 in `agent_setup/tests.rs`, 7 in `sync/tests.rs`.
