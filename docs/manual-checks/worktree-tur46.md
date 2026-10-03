# Manual checks: TUR-46 (logs in `.app/logs` with rotation, plus local crash files)

These need the running app (a signed build for the full flow), so they were
not run here. The headless parts are covered by `src-tauri/src/logs/tests.rs`
(the crash file's contents and name, two crashes in one second, pruning to
`MAX_CRASH_FILES = 5`, a real panic through the installed hook, the native
writer with a stand-in exception) and `crates/meeting-format/src/layout.rs`
(`logs_dir`).

What was run by hand on this Mac (macOS 26, arm64), outside the app: a
throwaway binary built from `src-tauri/src/logs/crash.rs` and `platform/`,
attaching the same handler, then crashing three ways. Each left one file:

```text
segv  -> crash-<ts>-native.log  exception: EXC_BAD_ACCESS (kind 1), code 0x1, subcode 0x0
abort -> crash-<ts>-native.log  exception: EXC_SOFTWARE (kind 5), code 0x10003, subcode 0x6
panic -> crash-<ts>.log         message: forced panic, location, backtrace
```

## Where the log goes

- After onboarding: `<meetings root>/.app/logs/meet-ai.log` (default
  `~/Meetings/.app/logs/meet-ai.log`).
- Before onboarding is finished (first run): the OS log folder, as before:
  `~/Library/Logs/pro.saleschat.meetai/meet-ai.log` on macOS,
  `%LOCALAPPDATA%\pro.saleschat.meetai\logs` on Windows,
  `~/.local/share/pro.saleschat.meetai/logs` on Linux. The folder is chosen
  once at startup, so the first launch **after** onboarding switches to
  `.app/logs`. The same rule applies after moving the meetings folder: the next
  launch writes under the new root.

## 1. The log lands in `.app/logs` and Settings opens it

1. Build and run the signed app (`just bundle-signed`, then open it) with
   onboarding already done. Quit and relaunch once if onboarding was finished
   in this session.
2. Expected: `~/Meetings/.app/logs/meet-ai.log` exists and has a line
   "logs and crash files go here" naming that folder.
3. Settings → Files → **Open logs folder**. Expected: Finder opens
   `~/Meetings/.app/logs`.
4. In a debug build (`just dev`) the same lines also print in the terminal and
   in the webview console; in a release build they do not.

## 2. First run before onboarding

1. With onboarding reset (Settings → Show setup again, then quit), relaunch.
2. Expected: the log is written to `~/Library/Logs/pro.saleschat.meetai/` and
   **Open logs folder** opens that folder. Finish onboarding, quit, relaunch:
   now `~/Meetings/.app/logs/`.

## 3. The log never grows past the cap

1. `LOG_MAX_BYTES` is 1 000 000 with `RotationStrategy::KeepSome(1)`. Leave
   the app running with a recording and a notes run until the log has rotated
   (or temporarily lower `LOG_MAX_BYTES` in a dev build).
2. Expected: `meet-ai.log` never exceeds ~1 MB, and there is at most one
   `meet-ai_<date>.log` next to it. The rotation itself is tauri-plugin-log
   2.9.1's (it rotates before a write would cross the cap), so there is no
   headless test of it here.

## 4. A forced native crash in the real app leaves a file

1. With the signed app running: `kill -ABRT $(pgrep -x meet-ai)`.
2. Expected: `~/Meetings/.app/logs/crash-<UTC yyyymmdd-hhmmss>-native.log`
   with `version`, `os`, `time` and `exception: EXC_SOFTWARE (kind 5), code
   0x10003, subcode 0x6`. macOS's own crash report still appears in
   `~/Library/Logs/DiagnosticReports` (the handler does not swallow it).
3. Relaunch with more than five crash files in the folder. Expected: only the
   five newest remain (native crashes are pruned at the next launch, not while
   crashing).

## 5. Windows and Linux

Not available here. CI runs the panic tests (`logs::tests`, including
`a_real_panic_leaves_a_crash_file`) on macOS today; Windows and Linux once
TUR-36 adds those test jobs. Until then the panic file on those OSes is
unverified. A real native crash on Windows (expected `exception code
0xc0000005` for an access violation) and Linux (`signal 11, code …`) is
unverified too.

## 6. Known gap: a crash before `setup`

The panic hook and the native handler are installed at the start of Tauri's
`setup`, so a panic or crash while the plugins initialise (before `setup`
runs) leaves no crash file. The OS's own crash report still covers it. The
ticket does not ask for more.
