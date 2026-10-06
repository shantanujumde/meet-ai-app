# Manual checks: TUR-131 (Codex default model, model notes, model name on Default runs)

Codex is not installed on the Mac this ran on, so no real `codex debug
models` output or `config.toml` was used. The field names `display_name` and
`description` come from the ticket; verify them against a real Codex. The
headless parts are covered by tests:

- `crates/agent/src/codex/settings.rs`: the top-level `model` line is read;
  a `model` only under a `[table]`, other `model_*` keys, blank, unquoted or
  escaped values are ignored.
- `crates/agent/src/codex.rs`: `listed_models` keeps Codex's display name and
  description (trailing "." dropped); `exec_args` adds the config model as
  `--model` only on Default.
- `crates/agent/tests/codex_fake.rs`
  `a_default_run_passes_and_reports_the_model_in_codex_s_config`: the run
  passes it and `Reply.model` names it (so `analyzed_model` is filled).
- `crates/agent/src/models.rs`: `models.json` label and note win over Codex's.

## 1. Settings shows Codex's default

1. Put `model = "gpt-5.6-sol"` at the top of `~/.codex/config.toml` (before
   any `[table]`). Signed-in Codex. Settings → Notes → Codex.
2. Expected: the Model field reads "Default (Codex picks, currently
   gpt-5.6-sol)" or its label.
3. Remove the line and reopen Settings. Expected: "Default (Codex picks)".

Why skipped: needs a signed-in Codex and the running app.

## 2. Codex's model notes show

1. Signed-in Codex, Settings → Notes → Codex, open the model dropdown.
2. Expected: models that `models.json` does not label show Codex's own name
   and one-line description, without a trailing full stop.

Why skipped: needs a signed-in Codex and the running app.

## 3. A Default run records the model

1. With the config line from check 1 and Model on Default, record a short
   meeting and let notes run with Codex.
2. Expected: the meeting's frontmatter has `analyzed_model: gpt-5.6-sol`.
   Without the config line, `analyzed_model` stays empty.

Why skipped: needs a signed-in Codex, a recording and the running app.
