# Manual checks: worktree-tur55

TUR-55: Windows and Linux file rules. `write_atomic` retries the rename on
Windows sharing/lock errors (10/20/40/80/160 ms) and then names the file in its
error; `SelfWrites` and the watcher root use `dunce::canonicalize`; the watcher
sets the hidden attribute on `<root>/.app` on Windows; running out of inotify
watches logs one warning with the `sysctl` fix.

The Windows-only tests (`meeting-format` platform/windows.rs: a held file
released after 60 ms, and a held file that never lets go; `store`
platform/windows.rs: `.app` is hidden) run in the `rust (windows)` CI job via
`cargo test --workspace`. Nothing below was run on a real Windows or Linux
machine.

## Run by hand

1. Linux: `sudo sysctl fs.inotify.max_user_watches=64`, make a meetings folder
   with 200 meeting subfolders, start meet-ai.
   Expect: one warning in the log containing `fs.inotify.max_user_watches`
   (and no repeat per folder). Restore the limit afterwards.
   Why skipped: needs a Linux machine and root; no UI surface exists for
   watcher errors, so the message is log-only (warn). Showing it in the window
   is a follow-up.
2. Windows: start meet-ai, open the meetings folder in Explorer with "Show
   hidden items" off.
   Expect: `.app` is not listed.
   Why skipped: needs a real Windows machine and the running app.
3. Windows: open `notes.md` of a meeting in an editor that locks it (or let
   Defender scan it), edit notes in the app.
   Expect: the save succeeds once the lock goes; if it never goes, the error
   names the file. No "changed outside the app" banner for the app's own save.
   Why skipped: needs a real Windows machine and the running app.
