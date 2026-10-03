# Manual checks: TUR-74 (agent model: the CLI's own default, and every model it offers)

These need a signed-in `claude` or `codex` and the running app, so they were
not run here: no agent CLI was started in this run, and no `--help` or model
list was captured from a real CLI. The headless parts are covered by tests:

- `crates/agent/src/models.rs`: `models.json` parses, Sonnet and Haiku come
  first, labels and notes, the 10-minute list cache (never keeps an empty
  list).
- `crates/agent/src/claude.rs`: `settings_model_in` reads `model` from a fake
  config folder and tolerates a missing, invalid or odd file without writing
  it; an "issue with the selected model" envelope becomes a clear error.
- `crates/agent/tests/claude.rs` `a_notes_run_without_a_model_lets_the_cli_pick`
  and `crates/agent/tests/codex_fake.rs`
  `a_notes_run_passes_the_model_only_when_one_is_set`: no model, no `--model`.
- `src-tauri/src/config/agent_tests.rs`: a fresh config writes
  `"model": null`; blank and `null` read as no model; a stored `"opus"` is kept.
- `src-tauri/src/agent_setup/tests.rs`: Default saves null over a stored opus,
  the Test run gets the picked model (and none for Default), a failed Test
  names the model it tried, Codex falls back to `models.json`.
- `src/ui/agent/ModelField.test.tsx`: buttons, dropdown, free text, a stored
  opus shown picked, "currently Sonnet", Codex's buttons.

## 1. Test with Default writes notes (Claude Code)

1. Fresh install (no `~/Meetings/.app/config.jsonc`). Onboarding → agent step,
   or Settings → Notes → Claude Code.
2. Expected: the Model field is blank with "Default (Claude Code picks)" (or
   "…, currently Sonnet" if `~/.claude/settings.json` has `"model": "sonnet"`),
   and the Default button is pressed.
3. Press **Test**. Expected: "It works. Claude Code wrote these notes…".
   `ps -ax -o args | grep "claude -p"` during the run shows no `--model`.
4. Open `config.jsonc`. Expected: `"model": null` (a fresh config writes it;
   do not pick Claude Code again first, picking the agent already chosen
   writes nothing). On an install whose config still stores `"opus"`, press
   **Default** first: that saves `null`.

## 2. Test with Haiku writes notes

1. Press the **Haiku** button, then **Test**.
2. Expected: notes come back, faster than Default. `ps` shows `--model haiku`.
   `config.jsonc` has `"model": "haiku"`.

## 3. A bad model name fails at Test with a clear message

1. Type `claude-nope-1` in the Model field, press Enter, then **Test**.
2. Expected: an error saying Claude Code cannot use this model (the name is
   wrong, or the account has no access), ending in `(model: claude-nope-1)`.
   Why it matters: the wording is matched on Claude Code's error text
   ("issue with the selected model", "may not exist or you may not have
   access", `not_found_error`), which was never captured from a real run. If
   the real text differs, the run still fails as "the agent CLI failed (exit
   code 1): Claude Code reported an error (…) (model: claude-nope-1)"; add the
   real phrase to `MODEL_HINTS` in `crates/agent/src/claude.rs`.

## 4. Every full model id in the dropdown works

`claude-sonnet-5-5`, `claude-opus-5-5`, `claude-haiku-4-5` and
`claude-fable-5-1` come from the ticket, not from a real CLI. Pick each in the
**More models** dropdown and press **Test**. Expected: notes each time. Remove
any that fail from `crates/agent/models.json`.

## 5. Codex: its own list, Default, and the fallback

1. With Codex signed in, pick Codex.
2. Expected: buttons Default plus the first two models `codex debug models`
   lists (on 0.152.1: `gpt-5.6-sol`, `gpt-5.6-terra`), the rest in the
   dropdown. Placeholder "Default (Codex picks)". Test with Default writes notes
   and `ps` shows no `--model`.
3. Close and reopen Settings within 10 minutes. Expected: `codex debug models`
   does not run again (`ps`).
4. Sign Codex out (`codex logout`). Expected: the list from `models.json`
   (`gpt-5.6-sol`, `gpt-5.6-terra`, `gpt-5.6-luna`, `gpt-5.5`, `gpt-5.2`).

## 6. An existing config keeps its model

1. Before updating, have `"model": "opus"` in `config.jsonc`.
2. After updating, open Settings. Expected: the field says `opus`, the
   dropdown shows "Opus: most capable, slowest", no button pressed. The notes
   run still passes `--model opus`. Pressing **Default** writes `"model": null`.

## Follow-ups

- **Model discovery for Claude Code.** No non-interactive way to list Claude
  Code's models was found in the repo's recorded `--help` output or SPEC A11's
  "Measured" sections, and this run did not start the CLI. Check `claude
  --help` on a current version for a models subcommand; until then the list
  is `crates/agent/models.json`.
- **Codex's current default.** "currently …" is shown only for Claude Code
  (from `~/.claude/settings.json`). Codex keeps its default in
  `~/.codex/config.toml` (`model`); reading it would need a TOML parser and
  was left out. The notes run ignores that file anyway
  (`--ignore-user-config`), so Codex's built-in default applies to notes.
- **Codex notes per model.** `models.json` has no one-line notes for the Codex
  models: none were recorded. Add them when known.
- `analyzed_model` in `meeting.md` is empty for a run on Default, since
  meet-ai does not know which model the CLI picked (as it already was for
  Codex on its own default).
