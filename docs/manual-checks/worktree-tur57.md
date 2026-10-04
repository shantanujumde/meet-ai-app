# Manual checks: TUR-57 (window look, fonts and wording per OS)

Done here, headless on a Mac: `pnpm vitest run` (osText words per OS, `data-os`
attribute), `pnpm typecheck`, `pnpm biome check`. Nothing below was run: each
needs a real machine and the running app, which this worktree cannot open.

## 1. Windows 11 window
Run: install the NSIS build from the PR's artifacts (or `pnpm tauri build` on a
Windows 11 machine), launch meet-ai.
Expect: a native title bar with minimise/maximise/close, Mica tint in the title
bar, no 78 px empty gap left of the in-app "meet-ai" row, solid sidebar, UI text
in Segoe UI Variable, transcript/code text in Cascadia Mono or Consolas.
Screenshot it into this folder.
Why skipped: needs a real Windows 11 machine.

## 2. Windows 10 window
Same as 1 on Windows 10. Expect: Mica is ignored, the window is a solid
opaque background with normal decorations, nothing see-through.
Why skipped: needs a real Windows 10 machine.

## 3. Linux window (GNOME and one other desktop)
Run: install the .deb or AppImage, launch.
Expect: native decorations from the desktop, opaque background, no left gap,
text in Cantarell / Noto Sans / Ubuntu (whichever the distro ships), mono in
DejaVu Sans Mono.
Why skipped: needs a real Linux desktop.

## 4. Wording
On each OS: open a meeting (button "Show in File Explorer" / "Show in file
manager"), onboarding folder step, the Record button tooltip with audio denied
("Fix audio permission in Settings first"), Settings > Notifications and the
Today pane ("Open Settings"), agent details ("works in PowerShell" on Windows).
Why skipped: needs the running app on each OS.

## 5. macOS unchanged
Run a signed build on macOS 26, compare with main: traffic lights, 78 px inset,
vibrancy and SF fonts are the same.
Why skipped: needs a signed build and the running app.

## Notes
- The in-app titlebar row stays on Windows/Linux under the native title bar
  (it holds the record control); only its traffic-light inset is removed.
- `windowEffects: mica` is set with `transparent: false`; the content column is
  opaque, so Mica shows only where the native title bar paints. Check 1 confirms.
