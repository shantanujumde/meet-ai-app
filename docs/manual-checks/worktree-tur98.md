# TUR-98 manual checks

## 1. Mica on Windows 11

- Run: launch a build on a real Windows 11 PC (`src-tauri/tauri.windows.conf.json` has `windowEffects: mica` with `transparent: false`).
- Expected: the window background shows the Mica material (tinted by the wallpaper), not a flat colour.
- Skipped: needs a real Windows 11 PC (manual check 1 of TUR-57).

## 2. `.app` hidden after first write on Windows

- Run: delete `.app` in the meetings folder, start meet-ai, close the window once (still-running notice) and look at the folder in File Explorer.
- Expected: `.app` is hidden. The `store` crate's Windows test covers `create_app_dir`; this checks the two callers (`lifecycle/notice.rs`, `sync/kept.rs`) on a real PC.
- Skipped: needs Windows.

## 3. Words off macOS

- Run: on Windows and Linux, deny recording access (no privacy pane denied) and open Settings, agent, with the CLI missing.
- Expected: "this PC" / "this computer" and "the Start menu" / "the app menu", no "Mac" or "Dock".
- Skipped: needs Windows and Linux.
