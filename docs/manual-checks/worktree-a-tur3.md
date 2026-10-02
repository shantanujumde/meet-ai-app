# Manual checks: TUR-3, `agent` and `tickets` in config.jsonc

Everything in this ticket runs headless; the round trips are unit tests in
`src-tauri/src/config/agent_tests.rs`. One check needs a real editor, so it was
not run:

1. **Editor autocomplete.** Nothing in the app writes the new sections yet
   (TUR-9 adds the IPC commands), so to try it, copy
   `src-tauri/config.schema.json` to `~/Meetings/.app/` and open
   `~/Meetings/.app/config.jsonc` in VS Code with
   `"$schema": "./config.schema.json"` at the top. Type `"agent": { "harness": `.
   **Expected:** the editor offers `claude-code`, `codex`, `none`, and shows a
   warning for `"codx"`. Unknown keys are not flagged. **Skipped because:** it
   needs a GUI editor.

Decisions (approved by Shann):

- The schema lives in the repo at `src-tauri/config.schema.json`. It is built
  into the binary with `include_str!` and written next to `config.jsonc` each
  time the app saves the `agent` or `tickets` section. There is no separate
  write at startup.
- No IPC commands yet; TUR-9 (Setup screens) adds them. Until then the new
  functions carry `#[allow(dead_code)]` / `#[allow(unused_imports)]`.
- `ConfigError` is new, in `src-tauri/src/config`. It maps to `UiError` domain
  `app`, kinds `unknown-harness`, `invalid-config` and `io`.
- `transcription` reading is unchanged: it still logs and falls back to the
  defaults. Only `agent` and `tickets` return errors.

Other notes:

- `src-tauri/Cargo.toml`: one existing line changed. `jsonc-parser` gained the
  `cst` feature (the parser's comment-keeping syntax tree, used for writes).
  `Cargo.lock` is unchanged.
- `agent.timeout_sec: 0` is rejected as `invalid-config`, to match the
  schema's `minimum: 1`.
- A fresh worktree needs `just sidecar` before `cargo check --workspace`
  (tauri-build looks for `target/meet-stt-<triple>`).
