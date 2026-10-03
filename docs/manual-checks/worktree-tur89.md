# Manual checks: TUR-89 (repo rules and docs honesty)

Most of this ticket is rules, docs and comments, checked headless here:

- `cargo test -p store`: `retention::tests::a_locked_wav_is_skipped_and_deleted_on_the_next_run`
  (now one test for every OS, holding the WAV the way `platform::test_lock`
  says), `permission_denied_counts_as_locked_on_every_os`, and
  `platform::other::test_lock::no_raw_code_is_a_lock_violation`.
- `cargo clippy -p store --all-targets -- -D warnings`, `cargo fmt --all --check`.
- `crates/store/src/platform/windows.rs` type-checked for
  `x86_64-pc-windows-msvc` on its own, with its test module
  (`rustc --edition 2021 --target x86_64-pc-windows-msvc --test --emit=metadata`):
  clean apart from dead-code warnings, since nothing calls it in a standalone
  build.
- `scripts/quality-rules-selftest.sh` under macOS `/bin/bash` 3.2: every case
  passes, among them the new R9 allow-list, `OR`/`AND`/`WITH`, dropping a
  notices section, deleting the notices file, R10 `--r10-tree` (an old cfg
  fails, a known-debt line warns, a new cfg in a known-debt file fails) and
  R7's missing-sidecar WARN.
- `scripts/quality-rules.sh --r10-tree` on this branch: no ERROR, three WARNs
  for the `R10_DEBT` lines in `crates/agent` (TUR-54). On `origin/main`'s
  `crates/store` the same run reports the four store cfgs, so the step would
  have caught the TUR-45 / TUR-42 merge.
- `actionlint .github/workflows/check.yml`.

## 1. The store's Windows lock check and its test, on Windows

Not run: no Windows machine, and `cargo check --target x86_64-pc-windows-msvc
-p store` cannot run on a Mac (rusqlite's bundled SQLite compiles C for the
target, which needs MSVC). No Windows CI job exists until TUR-36 (#83) merges.

1. On Windows with MSVC: `cargo test -p store retention` and
   `cargo test -p store platform`.
2. Expected: `a_locked_wav_is_skipped_and_deleted_on_the_next_run` passes
   (`mic.wav` held open with no sharing is skipped, `system.wav` deleted, the
   next run deletes `mic.wav`), and
   `platform::windows::test_lock::sharing_and_lock_violations_are_lock_violations`
   passes.
3. Once #83 is on main, its `rust (windows)` job runs these on every PR; this
   section can then go.

## 2. The self-test and whole-tree R10 on the ubuntu runner

Not run here: the `rust-portable` job runs on ubuntu, whose `awk` is mawk;
this Mac has only BSD awk. The R9 licence check and the notices section match
are new awk programs.

1. On this PR, open the `rust fmt + windows seam (ubuntu)` job.
2. Expected: "Quality rules self-test" prints `N/N passed`, and
   "R10 over the whole tree" exits 0 with the three `R10_DEBT` WARN lines.

Seen on PR #96's first CI run: `78/78 passed`, and the whole-tree step passed
with exactly those three WARNs. Nothing left to check here.

## 3. The manual steps this ticket corrected

TUR-74 §1 step 4 and TUR-75's `engine="whisper"` log lines were corrected from
the code (`useAgentSetup.ts` `pick` returns early for the agent already
chosen; the log line comes from `tracing` through `tauri-plugin-log`, which
quotes string fields), not from a run of the app. Expected when someone next
runs those checks: the steps pass as written.
