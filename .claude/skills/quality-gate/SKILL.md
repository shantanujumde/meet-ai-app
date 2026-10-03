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
scripts/quality-gate.sh                            # everything this branch changed
scripts/quality-gate.sh <file>...                  # exactly these files
scripts/quality-gate.sh --from-transcript <jsonl>  # files a session edited (+ branch changes)
scripts/quality-gate.sh --list [...]               # just print the file list
scripts/quality-rules.sh <file>...                 # only the repo rules
scripts/quality-rules.sh --r10-tree                # R10 over every tracked .rs file (CI runs this)
```

In your own worktree branch, the no-argument form is the right one: it checks
every file the branch changed since it left `main`, plus untracked files. On
`main`, pass the file names, because other work may be sitting in that
checkout.

Exit 0 means pass (or timed out; see below). Exit 2 means fail, with a report
on stderr: each failing check and the end of its output (for cargo, the
error and panic lines first). Fix everything listed, then run it again.

What it runs, only for the changed files and all at once:

- `.ts/.tsx/.js/.json/.css`: `pnpm exec biome check <files>`
- `.ts/.tsx`: `pnpm typecheck` and `pnpm exec vitest related --run <files>`
- `.rs`: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
  `cargo test` for the affected packages (`crates/<x>` becomes the name in its
  `Cargo.toml`, `src-tauri` becomes `meet-ai`). The gate builds the
  `target/meet-stt` sidecar (`just sidecar`) if it is missing.
- The repo rules below.

All checks together get `QUALITY_GATE_BUDGET_SECS` (default 480 s). A cold
build in a fresh worktree can run past that. The gate then kills the checks,
prints a WARN, and does not block. Run `just check` by hand in that case.

Quick fixes for common failures: `pnpm exec biome check --write <files>` for
biome format and import-order errors, and `cargo fmt -p <pkg>` for rustfmt.

## Rules and how to fix them

Findings print as `path:line RULE LEVEL: message`. ERROR fails; WARN does not.

Most rules only look at lines added since the *diff base*. That base is
`git merge-base HEAD origin/main`, so lines you already committed on your
branch still count as new. On `main`, the base is `HEAD`.

| Rule | What it catches | Fix |
| --- | --- | --- |
| R1 | a file over 600 non-test lines that is new or grew (Rust: before the `#[cfg(test)] mod` test module) | move the new code into its own module |
| R2 | a `"name://event"` string outside `src-tauri/src/events.rs` and `src/ipc/bindings.ts` | use the event constant |
| R3 (warn) | `"transcript.md"`, `"notes.md"`, `"segments.json"`, `"meeting.md"` or `".app"` in Rust outside `crates/meeting-format` | use the meeting-format constant |
| R4 | a new `.unwrap()` / `.expect(` in non-test Rust under `crates/*/src` or `src-tauri/src` (strings and comments ignored) | return the error with `?`; if it truly cannot fail, add `// quality: allow-unwrap <reason>` on that line or the line above |
| R5 (warn) | a sync `#[tauri::command]` that calls `meetings::x(`, `store::x(`, `fs::x(` or `std::fs::` | make it `pub async fn`, and use `spawn_blocking` for heavy work |
| R6 | a new `style={{` in `src/**/*.tsx` | use Tailwind classes in `className`; for a runtime-only value, add `{/* quality: allow-style <reason> */}` on that line or the line above |
| R7 | `src/ipc/bindings.ts` is stale (checked once per run, when a command file, a type they send, `Cargo.toml` or `bindings.ts` changed). A WARN, not an ERROR, when `target/meet-stt-<host triple>` is missing next to the source, since `meet-ai` cannot build then | run `just bindings` and keep the result; for the WARN, `just sidecar` (and copy `target/meet-stt*` into `$CARGO_TARGET_DIR` if you set it) |
| R8 | a new selector rule (a line with `{` not starting with `@`) in `src/app.css` | style with Tailwind utilities in the component; for a real global override, add `/* quality: allow-css <reason> */` |
| R9 | an `Adapted from` comment (every line, not only added ones) that is not `Adapted from <host>/<owner>/<repo>/<path> @ <commit> (<SPDX>)`, pins a branch, has a licence off the allow-list (MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unlicense, MPL-2.0; `X OR Y` when one side is, `X AND Y` when both are), or has no `THIRD_PARTY_NOTICES.md` section for its repo. A change to the notices file re-checks every file with an `Adapted from` line | add or extend the project's section in `THIRD_PARTY_NOTICES.md` (for `X OR Y`, its `Licence:` line names the one we took); for a GPL source, delete the copy and write our own |
| R10 | a new `cfg` naming an OS (`target_os`, `target_family`, `target_vendor`, `unix`, `windows`; `cfg`, `cfg!`, `cfg_attr` conditions) in Rust under `crates/` or `src-tauri/src/`, outside `platform/`, `macos/`, `windows/`, `linux/`, `eventkit.rs`, `build.rs` and `crates/*/tests/`. Android/iOS-only conditions pass. CI also runs it over the whole tree (`--r10-tree`) | move the OS code into the crate's `src/platform/` and call `platform::...` |

R3 prints warnings until its phase lands. To make it fail, flip `R3_LEVEL` to
`error` at the top of `scripts/quality-rules.sh` (R2 already is).

## How to add a rule

1. In `scripts/quality-rules.sh`, write `rule_rN() { local f=$1; ... }`. Return
   early for files it does not apply to. For each problem, call
   `report error|warn RN "$f" <line> "message that says how to fix it"`.
2. Reuse the helpers:
   - `added_lines`: line numbers added since the diff base
   - `added_text FILE STOP [OPTOUT]`: the added lines themselves, skipping
     opted-out ones
   - `test_start`: where the `#[cfg(test)] mod` test module starts; `cfg_test_item_lines`: lines of single `#[cfg(test)]` items
   - `is_rust_test_file`, `is_ts_test_file`, `is_new_file`
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
- `QUALITY_GATE_BUDGET_SECS=<n>`: the time limit for all checks (default 480).
- `QUALITY_GATE_TRANSCRIPT_ONLY=1`: in transcript mode, do not add the
  branch's changed files.
- `QUALITY_BASE=<rev>`: diff against this commit instead of the merge base.
- `R1_MAX_LINES=<n>`: change the R1 limit for one run.

## How the hook behaves

- It runs on `Stop` and `SubagentStop` (`.claude/settings.json`, 600 s timeout).
- It takes the files from the run's own transcript: the ones edited with Edit,
  Write, MultiEdit or NotebookEdit. In its own linked worktree on a branch other than `main`, it also adds
  the files the branch changed, so Bash-tool edits are caught too. Failures in
  those files are tagged `(from branch, not edited in this session)`. If you
  did not change them, say so and stop; do not edit them to satisfy the gate.
- A session with no edit and no Bash calls (read-only, such as a reviewer)
  passes at once. The primary checkout only ever uses the transcript list.
- A sub-agent is checked against its own transcript only. A `SubagentStop`
  without `agent_transcript_path` is skipped.
- On a failure it exits 2 and sends the report back to the model. It blocks at
  most 2 times in a row per session (per sub-agent), then lets the run stop
  with a note.
- If nothing changed since the last pass, it exits right away. "Nothing" means:
  - the same files, contents, `HEAD` and diff base
  - the same gate scripts and knobs
  - the same sidecar state

  A timeout is never recorded as a pass.
- A problem inside the hook itself never blocks. It prints a warning and exits 0.
