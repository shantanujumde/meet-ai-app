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
  no effect off macOS.
