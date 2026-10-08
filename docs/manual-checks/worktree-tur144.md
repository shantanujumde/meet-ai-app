# Manual checks: tur144

TUR-144: when the call app hangs up while recording, a 10-second countdown
card asks before stopping (SPEC A31). Its manual checks are in
[`worktree-tur141.md`](./worktree-tur141.md), section "TUR-144: the call-end
countdown", next to the parent ticket's overall check, as the ticket asked.

Tested headless on a Mac:

- `cargo test -p detect call_end` (28 tests): the 5 s wait, short mic drops,
  the countdown's zero, Stop now, Keep recording, cancel when back on the
  mic, stop by hand, the app followed (prompt's app, else first call app or
  browser), no call app, an OS that cannot list the apps, the card's line.
- `cargo test -p meet-ai --lib detection::call_end`: the countdown thread
  with a fake card and clock, the mic list as the rules see it, and which
  popup answers name an app.
- `pnpm vitest run src/ui/NotificationSettings.test.tsx`: the new switch
  writes `callEnd`.
