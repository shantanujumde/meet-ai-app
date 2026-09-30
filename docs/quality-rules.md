# Quality rules

Every Claude run in this repo ends with the quality gate. When a run (or a
sub-agent) tries to stop, `.claude/hooks/quality-gate-hook.sh` checks the files
that run edited. If a check fails, the run is sent back to fix it, at most twice
in a row. After that, the run is allowed to stop, and a note says the gate is
still red.

You can run the same checks by hand:

```sh
scripts/quality-gate.sh                    # files git sees as changed or new
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

**Why only the changed files?** Several agent runs share one working tree
(see `.claude/hooks/tree-snapshot.sh`). A plain `git diff` would also pick up
files other runs are halfway through. So the hook reads the run's own
transcript and only checks files the run edited with the Edit, Write,
MultiEdit or NotebookEdit tools. Files changed through shell commands such as
`sed -i` are not seen. Run the gate by hand for those.

## Levels

- **ERROR** fails the gate. The run has to fix it.
- **WARN** is printed but does not fail. It points at old code that should be
  fixed when someone next works there.

Most rules only look at lines **added** compared to `HEAD` (the last commit).
Old code does not fail the gate, but new code does. A file git has never seen
counts as fully added.

## The rules

### R1: file too big (ERROR for new or growing files)

A source file has more than 600 lines of non-test code. For Rust, that means
the lines before the first `#[cfg(test)]`. For TypeScript, it is the whole
file, and `*.test.*` files are skipped.

- ERROR: a new file is over 600 lines, or a file got longer and is now over 600.
- WARN: a file was already over 600 before this change and did not grow.

**Why:** big files are hard to review, and agents edit them badly. They lose
track of what is where and keep adding to the pile. Keeping each file to one job
keeps diffs small.

**Fix:** move the new code into its own module and import it.

### R2: Tauri event name spelled out (WARN for now)

A string like `"recording://state"` appears outside
`src-tauri/src/events.rs`, `src/ipc/bindings.ts` or `src/ipc/client.ts`.
Real URLs (`http://`, `https://`, `file://` and so on) are ignored.

**Why:** the Rust side sends events and the TS side listens for them by name.
If one side renames an event, the other side goes quiet and nothing reports an
error. With a single list of names, a rename becomes a compile error.

**Fix:** use the constant from `events.rs` (Rust) or `bindings.ts` (TS).

**Flip to ERROR** once `src-tauri/src/events.rs` exists: set
`R2_LEVEL=error` at the top of `scripts/quality-rules.sh`.

### R3: meeting folder file name spelled out (WARN for now)

`"transcript.md"`, `"notes.md"`, `"segments.json"` or `"meeting.md"` appears in
Rust code outside `crates/meeting-format/`. Test code does not count.

**Why:** the layout of a meeting folder is a file format that users keep for
years. If three crates each spell the names out, one of them will drift. A
single owner means one place to change it.

**Fix:** use the constant from `crates/meeting-format`.

**Flip to ERROR** once `crates/meeting-format` exists: set `R3_LEVEL=error`.

### R4: new `.unwrap()` or `.expect(` in app code (ERROR)

A new `.unwrap()` or `.expect(` in non-test Rust under `crates/*/src/` or
`src-tauri/src/`. Test code (after the first `#[cfg(test)]`, `tests/`
folders), `src/bin/` tools and comment lines are skipped. Only added lines
count.

**Why:** an `unwrap` that fails crashes the whole app, often in the middle of a
recording. A returned error can be shown to the user and the recording saved.

**Fix:** return the error with `?`, or handle the `None`/`Err` case. If it truly
cannot fail, say why on the same line or the line above:

```rust
// quality: allow-unwrap the regex is a literal, checked by a unit test
let re = Regex::new(r"^\d+$").unwrap();
```

### R5: sync Tauri command that touches disk (ERROR for new commands)

A `#[tauri::command]` that is not `async` (and not `#[tauri::command(async)]`)
whose body calls into `meetings::`, `fs::`, `store::` or `std::fs`.

- ERROR: the `#[tauri::command]` line or the `fn` line was added in this change.
- WARN: an existing command in a changed file.

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
classes).

### R7: generated bindings out of date (ERROR, once it applies)

Runs only when `src/ipc/bindings.ts` and a `just bindings` recipe both exist,
and a Rust file or `bindings.ts` changed. The gate regenerates the file and
compares it with the current one. Then it puts the original back, so it never
edits your files behind your back.

**Why:** `bindings.ts` is the TypeScript view of the Rust commands. If it is
stale, the frontend calls commands with the wrong shape, and that only shows up
at runtime.

**Fix:** run `just bindings` and keep the result.

### R8: new CSS rule in `src/app.css` (ERROR)

An added line containing `{` in `src/app.css`.

**Why:** `app.css` is being emptied into Tailwind one component at a time.
Every new rule there is one more to migrate later. Once the file is empty, it
is deleted.

**Fix:** style the component with Tailwind utilities instead.

## Knobs

| Env var | Effect |
| --- | --- |
| `QUALITY_GATE_DISABLE=1` | skip the gate and the hook entirely |
| `QUALITY_GATE_SKIP_TESTS=1` | run lint, format, types and rules, and skip `vitest` and `cargo test` |
| `R1_MAX_LINES=800` | change the R1 limit for one run |

## Adding a rule

See the header of `scripts/quality-rules.sh`. In short: write a small
`rule_rN` function that takes one file path and calls `report`, add it to
`RULES`, start it at `warn` if the code base is not ready, and document it here
and in `.claude/skills/quality-gate/SKILL.md`.
