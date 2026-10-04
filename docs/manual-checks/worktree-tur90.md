# Manual checks: TUR-90 part 1 (audit cleanup)

Checked headless here: `cargo test -p meet-ai -p agent -p stt -p store`,
clippy and fmt for those crates, `cargo check --target x86_64-pc-windows-msvc
-p stt -p agent`, `pnpm vitest run src/ui/agent src/ui/engine`,
`pnpm typecheck`, and the quality gate. The new tests:
`config::app_section::tests::two_saves_at_once_both_land` (fails without the
write lock), `saving_over_a_bad_section_is_refused_and_the_file_kept`, the
three calendar "skip the bad value" tests, `logs::tests::a_panic_*`,
`tray::tests::only_tray_ids_are_the_menu_bar_s_own`,
`claude::tests::the_envelope_names_the_model_that_did_the_work`, and the
vitest cases for the stale agent save, None → Claude Code, and Whisper before
the model list loads.

## 1. Capture a real Claude Code result envelope

Not run: no signed-in `claude` here, and tests never call agent CLIs.
`SAMPLE_ENVELOPE` in `crates/agent/src/claude.rs` is shaped after the Agent
SDK's documented `SDKResultMessage` (`modelUsage` keyed by model id), not
captured.

1. With Claude Code signed in and `agent.model` blank (Default):
   `echo 'Reply with {"ok":true}' | claude -p --output-format json --json-schema '{"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"]}'`
2. Expected: the JSON has a `modelUsage` object whose keys are model ids, the
   main model with the most `outputTokens`.
3. Replace `SAMPLE_ENVELOPE` with that output (strip the session id and any
   text), and check the test still passes. Then run notes on a meeting with
   Default picked: `meeting.md` gets a non-empty `analyzed_model`.

## 2. The running app

Not run: needs the signed app.

- Menu bar → Open meet-ai brings the window back with its Dock icon (now the
  same path as a Dock click). Clicking app-menu items (Edit → Copy) logs no
  "unknown menu-bar item" warning.
- Settings: flip "Show in Dock" and "Show next meeting in the menu bar"
  quickly one after the other; both stick after a restart.
- Move the meetings folder, then force a panic in a debug build: the crash
  file lands in `~/Library/Logs/pro.saleschat.meetai`, and the old meetings
  root is not recreated.
