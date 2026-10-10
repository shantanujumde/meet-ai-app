# TUR-168: agent runs share one harness, brief git times out, "Make notes now" waits, Test cancels

What was tested here: `cargo test -p meet-ai` on macOS with the fake CLI
(`test_support::FakeCli`) standing in for Claude Code, Codex and git, and
vitest on the touched screens. No real agent CLI, account or network share
was used. The checks below need those.

## Sign-in command on a signed-out CLI (skipped: needs real, signed-out CLIs)

- Run: a signed build. Sign Claude Code out (`claude auth logout`), record a
  short meeting with notes on Auto, let it stop.
- Expected: the meeting view says "Claude Code is not signed in. Open a
  terminal and run claude auth login, then press Retry." and shows
  `claude auth login` to type (no longer "type /login").
- Repeat with Codex installed only inside ChatGPT.app (no `codex` on the
  shell's PATH), signed out. Expected: the command shown is the full path,
  `/Applications/ChatGPT.app/Contents/Resources/codex login`, the same as
  Settings, Notes shows; pasted into Terminal it starts the sign-in.
- Press Sync on a task with the same signed-out Codex. Expected: "Couldn't
  send: Codex isn't signed in. Open a terminal, run `<that full path> login`,
  sign in, then press Retry."
- Why skipped: no signed-in or signed-out real CLI here; the logic is unit
  tested with fake paths.

## An agent reply that is not JSON vs. one in the wrong shape (skipped: needs a real CLI misbehaving)

- Hard to force with a real CLI. Expected if it happens: the meeting view
  says "Your agent's answer could not be read" (not JSON) or "Your agent's
  answer was not in the notes format" (wrong shape); Sync and Test show
  "The agent's answer could not be read" / "The agent's answer was missing
  parts".
- Why skipped: covered by fake-CLI tests in all three runners.

## Brief with a stalled repo (skipped: needs a stalled network share)

- Run: point `repos.default` (or a meeting's `repo:`) at a folder on an SMB
  or NFS share, then pull the network cable / pause the server so reads
  hang. Open a meeting's brief.
- Expected: the brief shows within about 5 seconds, without the "since last
  time" commits, and the log has a warn line "brief: git log took too long
  and was stopped". No `git` process is left running (`pgrep -fl "git -C"`).
- Why skipped: no network share here; a fake git that sleeps 30 s is stopped
  at the limit in `brief::tests::a_git_that_hangs_is_stopped_and_the_commits_left_out`.

## "Make notes now" right after Stop (skipped: needs the running app and a recording)

- Run: Settings, Notes, set notes to run only when asked. Record a meeting
  of a few minutes, press Stop, and press "Make notes now" on the meeting at
  once, while the transcript is still being finished.
- Expected: an error card "The transcript is still being finished. Its last
  lines are still being saved. Try again in a moment." and no notes run.
  A few seconds later "Make notes now" runs normally and the notes cover the
  meeting's last lines.
- Why skipped: no recording or speech engine here; the refusal is tested in
  `agent_run::tests::make_notes_now_is_refused_until_the_transcript_is_final`.

## Cancel on the agent Test (skipped: needs a real CLI)

- Run: Settings, Notes (and the onboarding agent step), pick Claude Code,
  press Test, then Cancel while "Testing…" shows.
- Expected: the Test stops at once, Test comes back, no error card, and no
  `claude` process is left (`pgrep -fl claude` shows none started by
  meet-ai). Cancel stops every Test going (a Test in onboarding and one in
  Settings at the same time would both stop).
- Why skipped: no real CLI here; `agent_setup::tests::cancel_stops_a_running_test`
  stops a fake CLI.

## Quit during "Send a test ticket" (skipped: needs a real CLI and tracker)

- Run: Settings, Tracker, press "Send a test ticket", then quit meet-ai
  (⌘Q) while it runs.
- Expected: no `claude` / `codex` process started by meet-ai is left after
  the app exits.
- Why skipped: no real CLI or tracker here; `sync::runs::tests::quitting_stops_tracker_checks_too`
  covers the shutdown.
