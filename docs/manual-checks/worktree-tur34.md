# Manual checks: TUR-34 (build and publish the release automatically)

None of these could run in the worktree. They need the owner's signing
keychain, repo admin rights, or a real GitHub Actions run, and the agent may
not run `security`, `codesign`, `gh secret`, `gh release` or `gh workflow`.

What did run locally: `actionlint` (with shellcheck) on
`.github/workflows/release.yml`; the release-please 17.6.0 updaters (the
version `release-please-action@v5` ships) on this repo's real `Cargo.toml`,
`package.json`, `src-tauri/tauri.conf.json` and `CHANGELOG.md`; the
notes, version-check and branch-parsing scripts from the workflow, with a
fake `gh`; `biome check` on the two release-please JSON files.

## 1. One-time setup (owner, on the Mac with the signing keychain)

Follow RELEASING.md, "Automatic release → One-time setup", steps 1–4:

1. Export the existing identity with `security export … -t identities -f
   pkcs12`. Run `scripts/signing/make-identity.sh --print` first. Expected
   leaf: `eafb73d29b2f35ca25c2f9fd193869fd880a7e0d`.
2. `gh secret set MACOS_SIGNING_P12_BASE64` and `MACOS_SIGNING_P12_PASSWORD`.
   Expected: `gh secret list` shows both.
3. Settings → Actions → General → turn on "Allow GitHub Actions to create
   and approve pull requests".
4. Optional, but needed if branch protection requires `check` on PRs: a
   `RELEASE_PLEASE_TOKEN` secret (a fine-grained PAT, Contents and Pull
   requests read/write). Without it the release PR shows no `check` status.
   I picked this design while Q1 was still open.

## 2. First run after this PR merges

1. Merge this PR. Expected: `check` passes on the merge commit, then a
   `release` run starts. Its `release-please` job opens
   `chore(main): release 0.4.0`, since `feat` commits have landed since
   v0.3.0. `release-pr-fixups` then pushes one commit to that PR, refreshing
   `Cargo.lock` and putting `tauri.conf.json`'s one-line arrays back. `build`
   and `publish` are skipped.
2. On the release PR, check the diff:
   - the version is `0.4.0` in `Cargo.toml`, `package.json`,
     `src-tauri/tauri.conf.json` and `.release-please-manifest.json`;
   - every workspace crate in `Cargo.lock` is `0.4.0`;
   - there is a new `## [0.4.0]` entry above `## [Unreleased]` in
     `CHANGELOG.md`, and the old text is untouched;
   - `check` passes, if `RELEASE_PLEASE_TOKEN` is set.
3. Merge a normal PR. Expected: the release PR gets updated, and no release is
   created.

## 3. The first automatic release

1. Merge the release PR. Expected: tag `v0.4.0`, and a `release` run in which
   `build` (macos-26) and then `publish` pass. In the `build` log, look for
   `leaf SHA-1 matches: eafb73d2…` and `signature OK: leaf eafb73d2…, version
   0.4.0`. The `Delete the temp keychain` step runs and prints `deleted …`.
2. Check the trust step's output. A `::warning::could not mark the cert
   trusted` line is fine as long as signing passed. If `just sign` fails with
   `CSSMERR_TP_NOT_TRUSTED` or "no identity found", the runner needs the cert
   trusted. In that case, change the trust step to
   `sudo security add-trusted-cert -d -r trustRoot -p codeSign -k /Library/Keychains/System.keychain`.
   This could not be tried without a runner.
3. Run `gh release view v0.4.0 --json name,assets,body`. Expected: the name
   is `meet-ai 0.4.0`, the assets are `meet-ai-0.4.0-macos-arm64.zip` and
   `.sha256`, and the body starts with `## Install` followed by the changelog
   notes.
4. On a Mac that had v0.3.0 installed with microphone and system-audio
   permission, follow the README install steps with the downloaded zip. Then
   do RELEASING.md step 9 (smoke test). Expected: `shasum -c` OK, the app
   opens, and recording works **without** a new permission prompt (same
   identity, so the TCC grants hold).

## 4. Failure paths (optional, on a throwaway fork or branch copy)

- Set `MACOS_SIGNING_P12_BASE64` to a different identity. Expected: `build`
  fails at the import step with "the .p12 holds leaf …, expected eafb73d2…",
  before the build starts, and the keychain is still deleted.
- Re-run failed jobs after a `build` failure. Expected: the same tag is
  built, and `publish` attaches the files with `--clobber` without adding
  `## Install` twice.
