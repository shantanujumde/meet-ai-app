# Manual checks: TUR-167 (hooks hardening)

TUR-167: hooks no longer rewrite `$HOME` in `sh -c` commands (Windows only,
whole `$HOME` token only), `hooks.timeout_secs` is cut to the
`agent.timeout_sec` ceiling (`u32::MAX` s) and the runner never adds a timeout
to a clock, hooks run through `agent::process::run_capped` (both pipes read at
once, `Interrupted` retried, tree killed after exit or timeout), a running hook
holds the folder gate, and on Windows a meeting folder holding `%` reaches
`cmd` as `"%MEETAI_MEETING_DIR%"` instead of as text `cmd` would expand.

Tested headless on macOS: `cargo test -p agent --lib process` (new
`capped::*` tests), `cargo test -p meet-ai --lib hooks` (23 tests, including
`$HOMEBREW_PREFIX` untouched, `'$HOME'` literal under `sh`, a huge timeout,
a backgrounded grandchild killed and not hanging the run, the gate held while
a hook runs and a hook refused during a move), clippy for both crates,
`cargo check --target x86_64-pc-windows-msvc -p agent --tests`.

Not run here:

1. Windows, `cmd` and `%` (CI's `rust (windows)` job runs
   `hooks::platform::windows_tests`). By hand: rename a meeting folder to
   `x%COMSPEC%`, set `"on_meeting_end": "echo %1 > C:\\hooks\\out.txt & rem"`,
   end that meeting. Expect: `out.txt` has the folder path with `%COMSPEC%`
   as written, not `C:\Windows\system32\cmd.exe`. Skipped: no Windows here;
   `cargo check -p meet-ai` for Windows fails on `ring`'s C build on this Mac.
2. Running app, folder gate: set `"on_meeting_end": "sleep 20 #"`, record a
   short meeting, stop it, and within 20 s try Settings, change meetings
   folder. Expect: refused with the `folder-busy` message; it works after the
   hook ends. The message still lists "a recording, notes, a model download"
   and not hooks (`folder_move.rs` is not this task's file). Skipped: needs
   the running, signed app.
3. Running app, huge timeout: `"timeout_secs": 99999999999` with a hook that
   exits 3. Expect "Hook failed: on_meeting_end (exited with status 3)" in the
   meeting view (before TUR-167 the hook thread panicked and nothing showed).
   Skipped: needs the running app.

Seen while testing: `process::tests::cli::cancel_still_works_while_waiting_for_output`
(not changed here) failed once with "no pid ... after 10 s" while other
workers were building, and passed on the rerun. Load-related, not this change.
