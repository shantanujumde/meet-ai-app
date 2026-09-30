---
name: quality-gate
description: Run the repo quality gate on changed files; use at end of a task or when asked to check code quality / run /quality-gate
---

# Quality gate

The same checks the Stop / SubagentStop hook runs at the end of every agent run
(`.claude/hooks/quality-gate-hook.sh`), available on demand. The full reasons
behind each rule are in `docs/quality-rules.md`.

## How to run

From the repo root:

```sh
scripts/quality-gate.sh <file>...                  # the files you changed (best)
scripts/quality-gate.sh --from-transcript <jsonl>  # files a session edited
scripts/quality-gate.sh                            # everything git sees as changed
scripts/quality-gate.sh --list [...]               # just print the file list
scripts/quality-rules.sh <file>...                 # only the repo rules
```

Pass the files you edited when you can. Other agents share this working tree,
so the no-argument mode also checks their half-done files.

Exit 0 means pass. Exit 2 means fail, with a report on stderr: each failing
check and the last 40 lines of its output. Fix everything listed, then run it
again.

What it runs, only for the changed files and all at once:

- `.ts/.tsx/.js/.json/.css`: `pnpm exec biome check <files>`
- `.ts/.tsx`: `pnpm typecheck` and `pnpm exec vitest related --run <files>`
- `.rs`: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
  `cargo test` for the affected packages (`crates/<x>` becomes the name in its
  `Cargo.toml`, `src-tauri` becomes `meet-ai`). The gate builds the
  `target/meet-stt` sidecar (`just sidecar`) if it is missing.
- The repo rules below.

Quick fixes for common failures: `pnpm exec biome check --write <files>` for
biome format and import-order errors, and `cargo fmt -p <pkg>` for rustfmt.

## Rules and how to fix them

Findings print as `path:line RULE LEVEL: message`. ERROR fails; WARN does not.
Most rules only look at lines added since `HEAD`.

| Rule | What it catches | Fix |
| --- | --- | --- |
| R1 | a file over 600 non-test lines that is new or grew (Rust: before the first `#[cfg(test)]`) | move the new code into its own module |
| R2 (warn) | a `"name://event"` string outside `src-tauri/src/events.rs`, `src/ipc/bindings.ts` and `src/ipc/client.ts` | use the event constant |
| R3 (warn) | `"transcript.md"`, `"notes.md"`, `"segments.json"` or `"meeting.md"` in Rust outside `crates/meeting-format` | use the meeting-format constant |
| R4 | a new `.unwrap()` / `.expect(` in non-test Rust under `crates/*/src` or `src-tauri/src` | return the error with `?`; if it truly cannot fail, add `// quality: allow-unwrap <reason>` on that line or the line above |
| R5 | a sync `#[tauri::command]` that calls `meetings::`, `fs::`, `store::` or `std::fs` (ERROR when new, WARN when old) | make it `pub async fn`, and use `spawn_blocking` for heavy work |
| R6 | a new `style={{` in `src/**/*.tsx` | use Tailwind classes in `className` |
| R7 | `src/ipc/bindings.ts` is stale (once `just bindings` exists) | run `just bindings` and keep the result |
| R8 | a new CSS rule (a line with `{`) in `src/app.css` | style with Tailwind utilities in the component |

R2 and R3 print warnings until their phase lands. To make them fail, flip
`R2_LEVEL` / `R3_LEVEL` to `error` at the top of `scripts/quality-rules.sh`.

## How to add a rule

1. In `scripts/quality-rules.sh`, write `rule_rN() { local f=$1; ... }`. Return
   early for files it does not apply to. For each problem, call
   `report error|warn RN "$f" <line> "message that says how to fix it"`.
2. Reuse the helpers: `added_lines` (line numbers added since HEAD),
   `added_text`, `test_start` (first `#[cfg(test)]`), `is_rust_test_file`,
   `is_ts_test_file`, `is_new_file`.
3. Add it to `RULES` (per file) or `RUN_ONCE` (once per run).
4. If existing code is not ready yet, give it a `RN_LEVEL=warn` variable and
   flip it to `error` later.
5. Keep the script compatible with macOS bash 3.2: no associative arrays and no
   `mapfile`. Run `shellcheck scripts/*.sh`.
6. Document it in `docs/quality-rules.md` (with the reason for it) and in the
   table above.

## Env knobs

- `QUALITY_GATE_DISABLE=1`: turn the gate and the hook off.
- `QUALITY_GATE_SKIP_TESTS=1`: skip `vitest` and `cargo test`, and keep lint,
  format, types and rules.
- `R1_MAX_LINES=<n>`: change the R1 limit for one run.

## How the hook behaves

- It runs on `Stop` and `SubagentStop` (`.claude/settings.json`, 600 s timeout).
- It reads the run's own transcript (the sub-agent's for `SubagentStop`) and
  checks only files edited with Edit, Write, MultiEdit or NotebookEdit.
- On a failure it exits 2 and sends the report back to the model. It blocks at
  most 2 times in a row per session (per sub-agent), then lets the run stop
  with a note.
- If nothing changed since the last pass, it exits right away.
- A problem inside the hook itself never blocks. It prints a warning and exits 0.
