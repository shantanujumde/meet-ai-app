# Manual checks: tur39 (TUR-39, Windows and Linux release builds)

`.github/workflows/release.yml`: new `build-other` matrix job (Windows NSIS,
ubuntu-22.04 .deb, ubuntu-24.04 AppImage), `publish` attaches every
`release-files*` artifact and writes one install block per OS.
`release-please-config.json`: `src-tauri/tauri.conf.json` `$.version` in
`extra-files`.

Checked here: `actionlint .github/workflows/release.yml` passes, the config is
valid JSON. Nothing below could run on this Mac.

## Run by hand

1. **One release produces four files (Wave H).** Merge a release PR.
   Expect: the GitHub release holds `meet-ai-X.Y.Z-macos-arm64.zip`,
   `-windows-x64-setup.exe`, `-linux-amd64.deb`, `-linux-x86_64.AppImage`,
   each with a `.sha256`, and the notes show the macOS, Windows and Linux
   blocks, the "preview: can't record yet" line on Windows and Linux, and the
   SmartScreen "More info → Run anyway" note.
   Why skipped: needs a real release run.
2. **tauri.conf.json follows the release.** On the next release PR, check
   that release-please changed `src-tauri/tauri.conf.json` `"version"` from
   `"../package.json"` to the new number, and the built apps report it.
   Why skipped: needs release-please to run on main.
3. **AppImage without a webkit pin.** If the ubuntu-24.04 AppImage build
   fails or the AppImage shows a blank window, pin
   `libwebkit2gtk-4.1-dev=2.44.0-2` (and its siblings, as Handy does) in the
   apt step and write the reason there. Measure it; not pinned now.
   Why skipped: needs the CI run and a Linux desktop.
4. **Installers start.** Run the `.exe` on Windows 10/11 (SmartScreen
   warning, then the app opens), `sudo apt install ./…deb` on Ubuntu 22.04,
   and the AppImage on Ubuntu 24.04 and one non-Debian distro.
   Why skipped: needs real Windows and Linux machines.
5. **The AppImage has no libpipewire/libspa.** The workflow checks the
   AppDir if Tauri leaves it; if it logs "no AppDir left to inspect", run
   `./meet-ai-*.AppImage --appimage-extract` and confirm no `libpipewire*` or
   `libspa*` under `squashfs-root`.
   Why skipped: needs the built AppImage.
