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
| any file | the repo rules R1–R10 below |

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

### R2: Tauri event name spelled out (ERROR)

A string like `"recording://state"` appears outside `src-tauri/src/events.rs`
or `src/ipc/bindings.ts`. Real URLs (`http://`, `https://`, `file://` and so
on) are ignored.

**Why:** the Rust side sends events and the TS side listens for them by name.
If one side renames an event, the other side goes quiet and nothing reports an
error. With a single list of names, a rename becomes a compile error.

**Fix:** use the constant from `events.rs` (Rust) or `bindings.ts` (TS).

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

`meet-ai` does not build without the speech sidecar next to the source
(`target/meet-stt-<host triple>`, made by `just sidecar`), whatever
`CARGO_TARGET_DIR` says. A worktree that shares a build cache often has it only
there. Then R7 prints a `WARN` that it skipped the check, as the gate does for
`stt`, instead of failing: the bindings were not compared, not found stale. Run
`just sidecar` (and copy `target/meet-stt*` into `$CARGO_TARGET_DIR` if you set
it), then the gate again.

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

### R9: adapted code without a notice (ERROR)

A comment (`//`, `#`, `/* */`, `<!-- -->`) with `Adapted from` in a source,
script, workflow, CMake or CSS file (`.rs .ts .tsx .js .swift .sh .yml .cmake
.css .html` and similar) that:

- is not in the form
  `Adapted from <host>/<owner>/<repo>/<path> @ <commit> (<SPDX>)`, where
  `<host>/<owner>/<repo>` is for example `github.com/insidegui/AudioCap`
  (`https://` in front is accepted)
- pins a branch or tag instead of a commit hash (7 to 40 hex digits)
- has no SPDX id, or one that is not on the COPY allow-list in
  CONTRIBUTING.md: `MIT`, `Apache-2.0`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`,
  `Zlib`, `Unlicense`, `MPL-2.0` (case does not matter). Anything else fails,
  including a licence nobody thought of. `X OR Y` is a choice, so it passes
  when one side is on the list (`MIT OR GPL-3.0-only` passes; we take it under
  MIT), and then the section's `Licence:` line must name the licence we took
  (`Licence: MIT (chosen from MIT OR GPL-3.0-only)`). `X AND Y` binds both, so
  each must be on the list. `X WITH <exception>` is judged by `X`.
- has no section in `THIRD_PARTY_NOTICES.md` whose `URL:` line names that
  repository. The match ignores case and must be the whole repository, so
  `github.com/acme/lib` does not match a notice for `github.com/acme/library`.
  A source listed under `## To confirm` does not count.

Unlike most rules, R9 reads every line of the file, not only the added ones: a
copy needs its notice for as long as it is in the repo. For the same reason,
when `THIRD_PARTY_NOTICES.md` itself is among the changed files, R9 also checks
every file `git grep -l 'Adapted from'` finds, so dropping a section (or the
whole file) fails the copy that needed it. CI passes deleted files in too
(`--diff-filter=ACMRD`) so a deleted notices file triggers this; the rules
skip any other path that no longer exists. `R9_NOTICES=<file>`
points it at another notices file. The rule's own script and self-test are
skipped, because they spell the format out as examples.

**Why:** our Apache-2.0 licence lets us ship MIT, BSD or Apache code from other
projects only if we keep their copyright and licence notices. GPL-family code
cannot go into an Apache-2.0 app at all. A notice that is missing is a licence
breach the day we ship, and nobody notices it in review.

**Fix:** add or extend the project's section in `THIRD_PARTY_NOTICES.md`
(project, URL, licence, copyright line, commit, files taken; the top of that
file shows the shape). For a GPL source, delete the copied code and write our
own. The full rules are in CONTRIBUTING.md, "Code from other projects".

**Self-test:** `scripts/quality-rules-selftest.sh` runs R9 on sample files in a
throwaway git repo: an `Adapted from` line with no notice must fail, one with a
notice must pass, and so on for each case above, including every licence on
the allow-list, ones that are not, `OR`/`AND`/`WITH`, and dropping a section
from (or deleting) the notices file. CI runs it in the
`rust-portable` job.

### R10: OS-specific cfg outside a platform module (ERROR)

A new `cfg` on an operating system in Rust under `crates/` or `src-tauri/src/`,
outside the places OS code is allowed to live. The rule reads every
`#[cfg(...)]`, `#![cfg(...)]`, `cfg!(...)` and `#[cfg_attr(...)]` whose
condition names `target_os`, `target_family`, `target_vendor`, `unix` or
`windows`, anywhere in it: `cfg(target_os = "macos")`,
`cfg(not(target_os = "macos"))`, `cfg(all(test, unix))`, `cfg!(windows)`, and
conditions split over several lines. For `cfg_attr` only the condition counts,
so `cfg_attr(not(debug_assertions), windows_subsystem = "windows")` passes.
Comments and string literals are skipped. Only a `cfg` with an added line
counts.

Allowed paths:

- `**/platform/**`, `**/macos/**`, `**/windows/**`, `**/linux/**`
- `**/eventkit.rs` (`crates/calendar`, named by SPEC §8.2)
- `**/build.rs` (better: read `CARGO_CFG_TARGET_OS`, as `crates/stt/build.rs`
  does)
- integration tests, `crates/*/tests/**`, so an OS-only test file can gate
  itself with `#![cfg(...)]`. A `tests.rs` inside `src/` is not one of these.

Not OS cfgs, so never flagged: `cfg(test)`, `cfg(feature = ...)`,
`cfg(debug_assertions)`, and a condition whose only OS names are
`target_os = "android"` or `"ios"`. That last one is Tauri's desktop-vs-mobile
plugin gate (`src-tauri/src/lib.rs`, `notify.rs`), not a port.

The known-debt list is empty since TUR-54, which moved the agent tests onto
`test_support::FakeCli`, a Rust fake CLI that runs on every OS. The list,
`R10_DEBT` in `scripts/quality-rules.sh`, is keyed on the path and the cfg
line's trimmed text (not its line number, so an edit above it does not break
the build). Each entry lets through only the first hit with that
text in its file, so a new cfg in the same files still fails, even one spelled
the same. Nothing else in the tree has an OS cfg outside the
allowed paths (`crates/store` keeps its one, Windows' lock-violation codes, in
`src/platform/` since TUR-89).

**Whole tree in CI.** R10 normally counts only added lines, so two branches
that each pass alone can still add up to an OS cfg in shared code when both
merge (TUR-45 and TUR-42 did, in `crates/store`). So the `rust-portable` CI job
also runs `scripts/quality-rules.sh --r10-tree`: R10 alone, over every file
`git ls-files '*.rs'` lists and every line, with the same allowed paths. Any
hit fails the job; the `R10_DEBT` lines print a `WARN`. Run it by hand the same
way.

**Why:** SPEC §8.2 keeps OS code in one place per crate so that a Windows or
Linux port adds files instead of editing every module. A `cfg` dropped into
shared code is how that drifts: each one is small, and together they turn a
port into surgery.

**Fix:** put the OS code in the crate's `src/platform/` module, one file per OS
(`macos.rs`, `windows.rs`, `linux.rs`, or folders) behind the same functions,
and call `platform::...` from the shared code. `crates/audio/src/platform/` is
the full example: `mod.rs` is the only file that picks an OS, and the Windows
and Linux files are explicit stubs. A test that only makes sense on one OS
can ask the platform module (a constant such as `platform::DEVICE_ACTIVITY`)
instead of using `cfg!`.

**Self-test:** the R10 cases in `scripts/quality-rules-selftest.sh`: a stray
`cfg(target_os = ...)` in a crate must fail and the same line under
`src/platform/` must pass, plus every form and allowed path above, and for
`--r10-tree`: an old cfg fails, a known-debt line warns, and a new cfg in a
known-debt file fails.

### R11: em dash in user-facing text (ERROR)

Fails on an em dash (`—`) anywhere in a changed file's code, outside
comments. The whole file is checked, not only the added lines, so one that
slipped in earlier is caught the next time someone edits the file.

Files checked:

- non-test `.ts` / `.tsx` under `src/` (not `src/ipc/bindings.ts`, which is
  generated, not `*.d.ts`, and not `*.test.*`, `*.spec.*` or `src/test/`)
- `.rs` under `src-tauri/src/`, up to the test module (`TEST_START_AWK`);
  Rust test files (`is_rust_test_file`, which includes `*/e2e.rs`) are skipped. Its strings reach the
  window (errors), the menu bar and notifications.

Comments are skipped: `//` to the end of the line (but not the `//` in
`https://`), and `/* ... */` across lines, which also covers JSX `{/* */}`.
The check is a few lines of awk, so it is not a parser: a `/*` inside a string
starts a "comment" too. Crates under `crates/` are not checked, since most of
their text is logs and CLI output; the few strings there that reach the window
were reworded by hand in TUR-92.

**Why:** TUR-92 rewrote the app's text to read like a person talking. Em dashes
were the most common sign of the stiff, written-up style it replaced.

**Fix:** use a full stop, comma, colon or brackets. Number ranges use "to"
("2 to 3 minutes"), not an en dash.

**Self-test:** the R11 cases in `scripts/quality-rules-selftest.sh`.

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
