# Manual checks: TUR-66 updater manifest (latest.json)

Done here: `python3 scripts/release/test_updater_json.py` (3 tests pass);
`actionlint .github/workflows/release.yml` clean; an end-to-end dry run in a
scratch folder signed three dummy files with a throwaway key
(`tauri signer generate --ci`, never `~/.tauri`), ran `updater-json.py`, and
got a manifest with all six keys and the `.sig` contents as signatures.

Decision (Q1, answered A1: b): `createUpdaterArtifacts` is NOT set in
`tauri.conf.json`. With it on, `pnpm tauri build` fails without
`TAURI_SIGNING_PRIVATE_KEY` (check.yml bundle jobs, local `just build`), and
on macOS Tauri would tar the .app before `just sign` re-signs it. release.yml
makes and signs the updater files itself with `tauri signer sign`.

## 1. Add the secrets
- Run: RELEASING.md, One-time setup, step 5 (`gh secret set TAURI_SIGNING_PRIVATE_KEY`, `..._PASSWORD`).
- Expected: next release's build jobs pass "Updater archive and signature" / "Updater signature".
- Why skipped: needs the owner's private key; never touched here.

## 2. Release holds latest.json and signed files
- Run: after the next release, `curl -sL https://github.com/shantanujumde/meet-ai-app/releases/latest/download/latest.json`.
- Expected: `version` = release, keys `darwin-aarch64`, `windows-x86_64`, `linux-x86_64` (and `-app`/`-nsis`/`-appimage`), each URL downloads, `.sig` files attached for the .app.tar.gz, setup .exe and AppImage.
- Why skipped: needs a real release run.

## 3. One update install per OS (once)
- Run: build version N-1 locally with `plugins.updater.active: true`, launch, trigger an update check.
- Expected: finds release N, verifies the signature, installs, relaunches as N. On macOS the updated app still passes `codesign --verify --deep --strict` and keeps mic/system-audio permission (same leaf). Measure it: whether the AppImage update works when the AppImage sits in a read-only folder.
- Why skipped: needs the running app, signed builds and real Windows/Linux machines.
