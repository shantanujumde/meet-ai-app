# Manual checks: TUR-19 (two flaky live_transcript tests)

Nothing in this ticket needs a person. It changes two tests and their fake
engine only (`src-tauri/src/live_transcript/tests.rs`, `fakes.rs`); no app
code changed, so there is nothing to see in the running app.

## What was run here, headless

- `cargo test -p meet-ai --lib a_guess_that_is_never_settled` and
  `cargo test -p meet-ai --lib an_engine_that_comes_back_after_stop_gave_up`,
  50 runs in a row each: idle, and again while a cold
  `cargo build --workspace` (separate target dir) ran the whole time. All
  passed. Before the fix the first one failed 6 of 20 runs on this Mac.
- Mutation check, to show the tests still prove what they did: removing the
  board's generation guard fails the second test; withdrawing a guess after
  3 s instead of 6 s fails the first ("withdrawn too eagerly"); never
  withdrawing fails it too (timeout). Each mutation was reverted.

## Note for whoever owns it

`docs/manual-checks/worktree-c-tur9.md` still lists
`a_guess_that_is_never_settled_or_withdrawn_does_not_stay_on_screen` as a
flaky test outside its ticket. That note is out of date once this merges; it
was left alone because the file belongs to TUR-9.
