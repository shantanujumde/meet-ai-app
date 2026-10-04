# Manual checks: worktree-tur101

TUR-101: Settings → "When notes run" picks `agent.auto_run`: "Automatically
after the call" (true, default) or "Only when I click" (false). Saving writes
only `auto_run` (new `save_notes_auto_run` command), keeping the rest of
`agent`. In Manual mode the meeting view shows "Make notes now" as its main
state. Tested headless: Rust save and reload round trip on the config text
(`agent_setup::tests`), Stop starting no run with `auto_run` off (existing
`agent_run::tests::auto_run_off_or_no_agent_starts_nothing_and_on_writes_the_notes`),
and the picker and meeting view in vitest against the IPC mock.

## Run by hand

1. In a signed build, open Settings, pick "Only when I click", quit meet-ai and
   open it again.
   Expect: the picker still shows "Only when I click", and `config.jsonc` has
   `"auto_run": false` with harness, model and timeout unchanged.
   Why skipped: needs the running app.
2. With Manual on and a signed-in agent, record a short call and stop.
   Expect: no "Writing notes…"; the meeting shows "Notes run when you ask"
   with "Make notes now", and clicking it writes the notes.
   Why skipped: needs a mic, a signed build and a signed-in agent CLI.
3. Switch back to "Automatically after the call" and record again.
   Expect: notes start on their own after Stop, as before.
   Why skipped: same as 2.
4. In both modes, turn "Make notes for this meeting" off during a recording.
   Expect: nothing is sent at Stop and no start button shows; switched back
   on, "Make notes now" shows and works.
   Why skipped: same as 2.
