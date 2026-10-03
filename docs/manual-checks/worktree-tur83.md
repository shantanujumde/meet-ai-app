# Manual checks: TUR-83

`watcher::tests::a_noted_self_write_is_not_reported` failed on the macOS CI
runner because the watcher judged the 750 ms self-write window at the moment
the debouncer handed the burst over, which is already `WATCH_DEBOUNCE` (500 ms)
plus up to one tick (125 ms) after the event arrived. That left about 125 ms
for FSEvents latency and thread scheduling; a busy runner went past it and the
app's own `notes.md` write was reported as an outside change. The fix judges
each path at the time its event reached the debouncer (`DebouncedEvent::time`).

All checks for this are headless tests and ran here. These need things this
worktree cannot do.

## Run by hand

1. **CI, three runs in a row.** Re-run the `rust (macos-26)` job on this PR
   until it has passed three times in a row.
   Expect: `a_noted_self_write_is_not_reported` passes every time.
   Why skipped: needs GitHub Actions; the local Mac never reproduced the
   failure without an injected delay.

2. **Typing in notes on a loaded Mac.** In a signed build, open a meeting,
   start a heavy job (a full `cargo build`), then type steadily in Meeting
   notes for a minute.
   Expect: the cursor never jumps and no "changed outside the app" reload
   happens.
   Why skipped: needs the running app.

3. **An outside edit right after typing.** Stop typing, wait one second, then
   change `notes.md` in another editor and save.
   Expect: the app picks the change up.
   Why skipped: needs the running app.
