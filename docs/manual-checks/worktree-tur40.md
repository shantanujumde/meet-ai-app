# Manual checks: TUR-40 (faster CI: parallel jobs and better caching)

What did run: `actionlint` (with shellcheck) on both workflows; the
"Every just check command runs here" guard, locally, against the real
workflow (passes) and against a copy with one command removed and one turned
into a comment (fails, naming both); and the PR's own `check` runs, whose
times are in the PR body. What could not run from the worktree: a push to
main, and a release.

## 1. The first push to main after this PR merges

1. Merge the PR. Expected: on the merge commit `check` shows four jobs,
   `rust (macos-26)`, `rust fmt + windows seam (ubuntu)`, `js (ubuntu)` and
   `check`, all green.
2. `gh cache list --limit 20`. Expected: new entries named
   `v0-rust-macos-debug-…`, `v0-rust-windows-seam-…` and `meet-stt-…`, made
   by the main run. The old `v0-rust-check-…` entries are no longer written
   and age out after 7 days unused; they can be deleted by hand with
   `gh cache delete <id>`.
3. Expected: `release` still starts after `check` passes on main (its
   `workflow_run` trigger reads the workflow's result, and the workflow fails
   if any job fails).
4. Open a new PR after that. Expected: its `rust` job log shows
   `Cache restored from key: v0-rust-macos-debug-…` and `Restore the sidecar`
   is a hit, and the PR run creates no new cache entry (`gh cache list`).

## 2. The `check` status still gates merging

The `protect main` ruleset requires a status named `check`. The aggregate job
(`check-result`, display name `check`) is that status now.

1. On any PR, break one job on purpose (a clippy warning, a biome error).
   Expected: that job fails, `check` fails, and the PR cannot merge.

## 3. release.yml: `cache-on-failure: true` on the build cache

Cannot be proven without a release run, and the agent may not run releases.

1. On the next release, look at the `build` job's post `Swatinem/rust-cache`
   step. Expected: it saves `v0-rust-release-…`.
2. If a release `build` ever fails after compiling (signing, zip, upload),
   "Re-run failed jobs". Expected: the re-run restores that cache and its
   `just bundle-signed` step is clearly shorter than the first try.
   Signing, the trigger and permissions are unchanged by this PR.
