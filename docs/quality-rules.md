# Quality rules

Every Claude run in this repo ends with the quality gate. When a run (or a
sub-agent) tries to stop, `.claude/hooks/quality-gate-hook.sh` checks the files
that run changed. If a check fails, the run is sent back to fix it, at most
twice in a row. After that, the run is allowed to stop, and a note says the
gate is still red.

You can run the same checks by hand:

```sh
scripts/quality-gate.sh                    # everything this branch changed
scripts/quality-gate.sh src/App.tsx crates/store/src/lib.rs
scripts/quality-gate.sh --from-transcript <session.jsonl>
scripts/quality-rules.sh <files>           # only the repo rules below
```

## What the gate runs

Only on the changed files, all at the same time:

| Changed files | Checks |
| --- | --- |
| `.ts .tsx .js .json .css` | `biome check` on those files |
| `.ts .tsx` | `pnpm typecheck`, and `vitest related --run` (the tests that import those files) |
| `.rs` in `crates/<x>/` or `src-tauri/` | `cargo fmt --check`, `cargo clippy --all-targets -D warnings` and `cargo test`, only for the affected packages |
| any file | the repo rules R1–R8 below |

`crates/stt` tests and every `meet-ai` build need the Swift speech helper at
`target/meet-stt`. If it is missing, the gate builds it with `just sidecar`. If
that fails, the gate prints a `WARN` line and skips those checks.

### Which files count as "changed"

The rule is that a run is checked on the files **it** changed, not the whole
repo.

- The hook reads the run's own transcript and takes every file edited with
  the Edit, Write, MultiEdit or NotebookEdit tools. A sub-agent is checked
  against its own transcript only. If Claude Code does not pass one, that stop
  is skipped.
- A session with no edit calls and no Bash calls is read-only, such as a
  reviewer. It passes at once. It is never held to account for what the branch
  already had.
- Edits made through the Bash tool (`sed -i`, a heredoc) are not in that list.
  So the gate also adds every file the branch changed since it left `main`,
  plus untracked files. It does this only when all three are true:
  - the run is in a linked worktree (made with `git worktree add`, so its
    `git-dir` differs from `git-common-dir`)
  - it is on a branch other than `main`
  - it made at least one edit or Bash call

  That is the normal case: each run gets its own worktree (see CONTRIBUTING.md,
  "Agent runs and the working tree").

  Any Bash call counts, even a read-only one like `git diff`, because the gate
  does not try to guess what a shell command did. So a reviewer with a PR
  branch checked out can still be checked against that branch's files. To keep
  it from "fixing" code it never touched, the failure report:
  - tags each line that names such a file with
    `(from branch, not edited in this session)`
  - lists those files at the end
  - finishes with: "If you did not change these files in this session, say so
    and stop; do not edit them to satisfy the gate."

  The limit of 2 retries still applies. After it, the run is let through.
- In the primary checkout, only the transcript list is used, even on a
  feature branch, because other runs' files may be sitting there. Bash-tool
  edits are not caught there. Run the gate by hand with the file names.

### Time limit

A fresh worktree has no `target/` or `node_modules/`, so the first Rust check
can mean a full cold build. The checks together get `QUALITY_GATE_BUDGET_SECS`
(480 s by default), which is under the hook's 600 s timeout. Past that, the gate
kills the running checks and everything they started, so no stray cargo keeps
the build lock. It prints a `WARN` line saying the gate timed out and to run
`just check`, and it does **not** block. A timeout is never recorded as a pass, so the next stop tries again,
now with a warm build.

## Levels

- **ERROR** fails the gate. The run has to fix it.
- **WARN** is printed but does not fail. It points at old code that should be
  fixed when someone next works there.

Most rules only look at lines **added** compared to the *diff base*. The base is
where this branch left main: `git merge-base HEAD origin/main`, falling back to
`main`, then `HEAD`. So a line the run already committed still counts as new.
Agents usually commit before they stop, and diffing against `HEAD` alone would
let those lines through. On `main` itself the base is `HEAD`, so only
uncommitted lines count there. `QUALITY_BASE=<rev>` overrides the base.

A file the base does not have counts as fully added. This takes the place of
the allow-list file (`scripts/quality-baseline.txt`) the plan first proposed:
the base commit is the baseline, so there is no list to keep up to date.

## The rules

### R1: file too big (ERROR for new or growing files)

A source file has more than 600 lines of non-test code. For Rust, that means
the lines before the test module (a `#[cfg(test)]` followed by `mod ...`). For TypeScript, it is the whole
file, and `*.test.*` files are skipped.

- ERROR: a new file is over 600 lines, or a file got longer than it was at the
  base and is now over 600.
- WARN: a file was already over 600 at the base and did not grow.

**Why:** big files are hard to review, and agents edit them badly. They lose
track of what is where and keep adding to the pile. Keeping each file to one job
keeps diffs small.

**Fix:** move the new code into its own module and import it.

### R2: Tauri event name spelled out (WARN for now)

A string like `"recording://state"` appears outside `src-tauri/src/events.rs`
or `src/ipc/bindings.ts`. Real URLs (`http://`, `https://`, `file://` and so
on) are ignored.

**Why:** the Rust side sends events and the TS side listens for them by name.
If one side renames an event, the other side goes quiet and nothing reports an
error. With a single list of names, a rename becomes a compile error.

**Fix:** use the constant from `events.rs` (Rust) or `bindings.ts` (TS).

**Flip to ERROR** once `src-tauri/src/events.rs` exists: set
`R2_LEVEL=error` at the top of `scripts/quality-rules.sh`.

### R3: meeting folder name spelled out (WARN for now)

The exact string literal `"transcript.md"`, `"notes.md"`, `"segments.json"`,
`"meeting.md"` or `".app"` appears in Rust code outside
`crates/meeting-format/`. Test code does not count.

**Why:** the layout of a meeting folder is a file format that users keep for
years. If three crates each spell the names out, one of them will drift. A
single owner means one place to change it.

**Fix:** use the constant from `crates/meeting-format`.

**Flip to ERROR** once `crates/meeting-format` exists: set `R3_LEVEL=error`.

### R4: new `.unwrap()` or `.expect(` in app code (ERROR)

A new `.unwrap()` or `.expect(` in non-test Rust under `crates/*/src/` or
`src-tauri/src/`. These are skipped:

- test code: the `#[cfg(test)] mod ...` module and everything after it, single `#[cfg(test)]` items (a test-only fn or impl block), and `tests/` folders
- `src/bin/` tools
- text inside string literals and `//` comments

Only added lines count.

**Why:** an `unwrap` that fails crashes the whole app, often in the middle of a
recording. A returned error can be shown to the user and the recording saved.

**Fix:** return the error with `?`, or handle the `None`/`Err` case. If it truly
cannot fail, say why on the same line or the line above:

```rust
// quality: allow-unwrap the regex is a literal, checked by a unit test
let re = Regex::new(r"^\d+$").unwrap();
```

### R5: sync Tauri command that touches disk (WARN)

A `#[tauri::command]` that is not `async` (and not `#[tauri::command(async)]`)
whose body calls `meetings::something(`, `store::something(`,
`fs::something(` or anything in `std::fs::`. Only calls count. A type name
like `meetings::MeetingList` or a comment does not.

This rule is only a guess: it cannot see through helper functions. So it warns
and never fails. The message says "new" when the command or its `fn` line was
added.

**Why:** Tauri runs sync commands on the main thread. A slow disk (an external
drive, iCloud, a big folder) then freezes the whole window.

**Fix:** make it `pub async fn`, and put heavy work in
`tauri::async_runtime::spawn_blocking`.

### R6: new inline `style={{ }}` in React (ERROR)

A new `style={{` in `src/**/*.tsx` (test files skipped).

**Why:** styling is moving to Tailwind utility classes. Inline styles cannot use
the design tokens, do not support dark mode or hover states, and are hard to
find later.

**Fix:** use `className` with Tailwind utilities (and `cn()` for conditional
classes). For a value only known at runtime, such as a measured width, opt out
on the same line or the line above:
`{/* quality: allow-style <reason> */}` or `// quality: allow-style <reason>`.

### R7: generated bindings out of date

`src/ipc/bindings.ts` is generated from the Rust command list by
`just bindings` (a headless test, `export_bindings` in
`src-tauri/src/bindings.rs`). The rule regenerates it into a temp copy and
compares. It runs once per gate run, and only when a changed file can change the
output: command files, the types they send, `Cargo.toml`, or `bindings.ts`
itself. It never rewrites the working tree. The fix is `just bindings`, then
commit the file. CI runs the same check (`git diff --exit-code`).

**Why:** `bindings.ts` is the TypeScript view of the Rust commands. If it is
stale, the frontend calls commands with the wrong shape, and that only shows up
at runtime.

### R8: new CSS rule in `src/app.css` (ERROR)

An added selector line (a line with `{` that does not start with `@`) in
`src/app.css`. At-rules like `@media`, `@keyframes`, `@supports`, `@layer` and
`@theme` are wrappers, so they are not flagged themselves. A new selector
inside them is still flagged.

**Why:** `app.css` is being emptied into Tailwind one component at a time.
Every new rule there is one more to migrate later. Once the file is empty, it
is deleted.

**Fix:** style the component with Tailwind utilities instead. When a rule
really has to be global CSS (a third-party override), opt out on the same line
or the line above with `/* quality: allow-css <reason> */`.

## Knobs

| Env var | Effect |
| --- | --- |
| `QUALITY_GATE_DISABLE=1` | skip the gate and the hook entirely |
| `QUALITY_GATE_SKIP_TESTS=1` | run lint, format, types and rules, and skip `vitest` and `cargo test` |
| `QUALITY_GATE_BUDGET_SECS=480` | time limit for all checks together |
| `QUALITY_GATE_TRANSCRIPT_ONLY=1` | do not add the branch's changed files to the transcript list |
| `QUALITY_BASE=<rev>` | diff base for the rules |
| `R1_MAX_LINES=800` | change the R1 limit for one run |

## How the hook remembers things

It keeps small files in `${TMPDIR:-/tmp}/meet-ai-quality-gate/`:

- a retry counter per session (per sub-agent for `SubagentStop`)
- a "last pass" record, so a stop with nothing new is instant

The pass record covers:

- the file list and file contents
- `HEAD` and the diff base
- both gate scripts
- `QUALITY_GATE_SKIP_TESTS`
- whether the sidecar is built

Changing any of those runs the gate again. Files older than a day are deleted.

## Adding a rule

See the header of `scripts/quality-rules.sh`. In short: write a small
`rule_rN` function that takes one file path and calls `report`, add it to
`RULES`, start it at `warn` if the code base is not ready, and document it here
and in `.claude/skills/quality-gate/SKILL.md`.
