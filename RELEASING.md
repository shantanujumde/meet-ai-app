# Releasing meet-ai

How a release gets from merged pull requests to a GitHub release with the
signed app attached.

Normally this is automatic: `.github/workflows/release.yml` (TUR-34) builds,
signs and publishes when you merge the release PR. The manual steps further
down are the fallback, for when the workflow cannot run. They were written
after v0.2.0 (2026-09-28) and every command in them was run for that release.
If a step stops being true, fix this file in the same commit.

Distribution is personal only (SPEC §8.1). The app is signed with the local
self-signed identity from `make-identity.sh`, not a Developer ID, and is not
notarized.

## Automatic release

### How it works

1. Every push to `main` runs `check` (`.github/workflows/check.yml`). When it
   passes, `release.yml` runs.
2. [release-please](https://github.com/googleapis/release-please) reads the
   Conventional Commit titles merged since the last `vX.Y.Z` tag. PRs are
   squash-merged, so the PR title is the commit title. It opens one release
   PR, titled `chore(main): release X.Y.Z`, or updates it if it is already
   open. That PR:
   - bumps the version in `Cargo.toml` (`[workspace.package]`),
     `package.json` and `.release-please-manifest.json`. The app reads its
     version from `package.json`, because `src-tauri/tauri.conf.json` has
     `"version": "../package.json"`. release-please would rewrite
     `tauri.conf.json` in a layout that `biome check` rejects, so the file
     points there instead of holding its own number;
   - bumps every workspace crate in `Cargo.lock`. Each one inherits the
     workspace version, so the lockfile has to change with it. The
     jsonpath `$.package[?(!@.source)]` picks out the workspace crates, so
     new crates are covered too;
   - adds a `## [X.Y.Z]` entry to `CHANGELOG.md`, above the text already
     there. The entry is built from the `feat` and `fix` titles (also `perf`
     and `revert`).

3. Merging any other PR only updates the release PR. Nothing is published.
4. Merging the release PR makes release-please tag the merge commit `vX.Y.Z`
   and create the GitHub release. Then, on a `macos-26` runner:
   - check that `check` passed on that commit;
   - import the signing identity from the repo secrets into a temporary
     keychain. The run stops here if its leaf SHA-1 is not `eafb73d2…`;
   - run `just bundle-signed`;
   - re-check the signature as in step 6 below (`codesign --verify --deep
     --strict`, same leaf, version `X.Y.Z`);
   - `ditto`-zip the app as `meet-ai-X.Y.Z-macos-arm64.zip`, write the
     `.sha256` file, and verify the unzipped copy again;
   - delete the temporary keychain, whether the run passed or failed.

   A last job attaches the zip and the checksum to the release. It renames
   the release `meet-ai X.Y.Z` and puts the step 8 Install section above the
   changelog notes.

How the next version is chosen (`release-please-config.json`): while the
version is below 1.0, `feat` bumps the minor number and `fix` bumps the patch
number. A breaking change (`feat!:`) also bumps only the minor number. To force
a version, put `Release-As: X.Y.Z` in the body of a commit on `main`.

Only one release run happens at a time (`concurrency: release`), and a run that
has started is never cancelled. The release jobs run only after a successful
`check` on a push to `main` in `shantanujumde/meet-ai-app`. Pull requests,
forks included, never reach them.

### One-time setup

These are done once by the owner, on the Mac that holds the signing keychain.

1. Export the **existing** identity. Do not make a new one: a new
   certificate resets every user's microphone and system-audio permission
   (`docs/findings.md` §10.6, §11).

   ```sh
   K=~/Library/Keychains/meet-ai-signing.keychain-db
   security unlock-keychain -p meetai "$K"
   scripts/signing/make-identity.sh --print          # leaf SHA-1: eafb73d2…
   P12_PASS="$(openssl rand -hex 24)"
   security export -k "$K" -t identities -f pkcs12 -P "$P12_PASS" -o /tmp/meet-ai-signing.p12
   ```

2. Store it and its password as repo secrets, then delete the file:

   ```sh
   base64 -i /tmp/meet-ai-signing.p12 | gh secret set MACOS_SIGNING_P12_BASE64
   printf '%s' "$P12_PASS" | gh secret set MACOS_SIGNING_P12_PASSWORD
   rm -P /tmp/meet-ai-signing.p12; unset P12_PASS
   ```

3. In the repo settings, under Actions → General → Workflow permissions, turn
   on "Allow GitHub Actions to create and approve pull requests". Without it,
   release-please cannot open the release PR.
4. Optional: add a `RELEASE_PLEASE_TOKEN` secret. Use a fine-grained personal
   access token for this repo only, with Contents and Pull requests set to
   read and write. GitHub does not start workflows for anything pushed with
   the built-in `GITHUB_TOKEN`, so without this token the release PR shows no
   `check` result. `check` still runs when the PR merges, and the build waits
   for it. If branch protection requires `check` on PRs, you need this token.
5. Add the updater signing key (TUR-66): `TAURI_SIGNING_PRIVATE_KEY` (the
   private key from the Paperclip secret `meet-ai/tauri-updater/private-key`,
   the pair of the `pubkey` in `src-tauri/tauri.conf.json`) and
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Without them the build jobs stop at
   "Updater archive and signature" / "Updater signature", naming the missing
   secret. Those steps sign `meet-ai-X.Y.Z-macos-arm64.app.tar.gz`, the
   Windows setup `.exe` and the AppImage (`.sig` beside each); `publish`
   writes `latest.json` with `scripts/release/updater-json.py`. The updater
   stays `active: false` until the owner turns it on.

### Cutting a release

1. Open the release PR (`chore(main): release X.Y.Z`). Read the version and
   the changelog entry. To change the wording, edit `CHANGELOG.md` on that
   branch. To change the version, see `Release-As` above.
2. Merge it.
3. About 20–30 minutes later (more with a cold cache), the release shows as
   Latest with both files attached:

   ```sh
   gh release view vX.Y.Z --json assets --jq '.assets[].name'
   ```

4. Smoke-test it (step 9 below). The workflow cannot test the app with real
   audio.

Clean up `CHANGELOG.md` in a normal PR when you get to it. release-please adds
each new entry above the old `## [Unreleased]` section. It does not move
anything out of that section, and it does not update the compare links at the
bottom.

### When the workflow fails

- **Before the release exists** (in `release-please`): fix the cause, then re-run the workflow from the
  Actions tab. A merge to `main` also starts a new run.
- **After the release exists** (in `build` or `publish`): the tag and the
  release are already there, but have no files attached. Fix the cause,
  then use **Re-run failed jobs** on the same run. That keeps the
  release-please output, so the same tag is built. A brand new run would not
  build it again, because release-please has already released it. If CI
  cannot build it at all, follow steps 6–8 below by hand for that tag. Skip
  steps 1–5: the version, the changelog, the PR and the tag already exist.
- **The leaf SHA-1 does not match**: the secret holds a different identity.
  Export the right one again (One-time setup, steps 1–2). Never rotate the
  identity to make the run pass.
- **`check has not passed on <sha>`**: release-please tags the release PR's
  merge commit, and `check` failed or has not run on that commit. Re-run
  `check` on it, then re-run the failed jobs.

## Manual fallback

Use this only when the workflow cannot run (Actions down, secrets missing, a
signing problem only a laptop can fix). Do not run steps 1–5 for a version
release-please has already tagged.

### Before you start: unlock the signing keychain

`just sign` needs the private key in `~/Library/Keychains/meet-ai-signing.keychain-db`.
macOS keeps that key encrypted until the keychain is unlocked. The password is
`meetai` (set in `scripts/signing/make-identity.sh`). It only guards this
throwaway local signing key, nothing else.

```sh
K=~/Library/Keychains/meet-ai-signing.keychain-db
security unlock-keychain -p meetai "$K"
security show-keychain-info "$K"   # should print "no-timeout"
scripts/signing/make-identity.sh --print   # leaf SHA-1: eafb73d2…
```

If `show-keychain-info` prints a timeout instead of `no-timeout`, turn auto-lock
off so builds stop failing overnight:

```sh
security set-keychain-settings "$K"
```

`make-identity.sh` creates the keychain with a 10-hour auto-lock
(`set-keychain-settings -lut 36000`), so run that line again after any
`--rotate`.

#### If macOS starts stacking "Keychain Not Found" dialogs

Symptom: repeated *"A keychain cannot be found to store 'Chrome'"* windows
piling up on the desktop. Nothing is wrong with the login keychain file — it has
fallen off the **user keychain search list**, so Chromium-based apps can no
longer reach their `… Safe Storage` item and re-ask on every attempt.

The cause is `security list-keychains -d user -s …`: `-s` *replaces* the list.
Run under a redirected `$HOME` (any sandboxed agent shell) it reads an empty
list back and writes the signing keychain in as the only entry. `make-identity.sh`
now refuses to write an empty list (TUR-10, FINDINGS §10.6), but a hand-run
`list-keychains -s` can still do it.

Click **Cancel**, not *Reset To Defaults*, then put the list back:

```sh
security list-keychains -d user -s \
  ~/Library/Keychains/login.keychain-db \
  ~/Library/Keychains/meet-ai-signing.keychain-db
security default-keychain -s ~/Library/Keychains/login.keychain-db
security list-keychains          # login + meet-ai-signing + System
```

Run it in a normal Terminal with your real `$HOME` — under a redirected one it
writes a different preference file and changes nothing.

### 1. Pick the version

[Semantic Versioning](https://semver.org/spec/v2.0.0.html), while the version is below 1.0:

- New features → bump the minor number (0.2.0 → 0.3.0).
- Fixes only → bump the patch number (0.2.0 → 0.2.1).

See what changed since the last tag:

```sh
git fetch --tags
git log --oneline "$(git describe --tags --abbrev=0)"..main
```

### 2. Bump the version and write the changelog

Work on a branch named `release/vX.Y.Z`, cut from the commits you want to ship.

Change the version in all of these files:

| File | Field |
|---|---|
| `Cargo.toml` | `[workspace.package] version` |
| `package.json` | `"version"`. This is what the built app reports: `src-tauri/tauri.conf.json` has `"version": "../package.json"` |
| `.release-please-manifest.json` | `"."`. Keep it in step, or the next automatic release starts from the wrong version |

Then refresh `Cargo.lock` so every workspace crate picks up the new number:

```sh
cargo metadata --format-version 1 > /dev/null
```

In `CHANGELOG.md`:

1. Add a `## [X.Y.Z] — YYYY-MM-DD` section right under `## [Unreleased]`, with
   `### Added` / `### Fixed` / `### Changed` as needed. Write it from the commit
   bodies, not only the subject lines.
2. Update the link lines at the bottom:

   ```
   [unreleased]: https://github.com/shantanujumde/meet-ai-app/compare/vX.Y.Z...HEAD
   [X.Y.Z]: https://github.com/shantanujumde/meet-ai-app/compare/vPREV...vX.Y.Z
   ```

### 3. Run the health gate

```sh
just check
```

It has to pass. CI (`.github/workflows/check.yml`) runs the same check on every
pull request, but run it locally first: a red CI run only tells you after the
push.

If it stops at `cargo fmt --check` or at a Biome `format` error, run `just fmt`,
look at the diff, and commit it on its own before the release commit. For
v0.3.0 that was one `if/else` in `meetings.rs` and line wrapping in the
Paperclip patch scripts added in 6548320 (PR #3, since deleted); Biome also
rewrote runs of spaces inside regexes to ` {12}`, which matches the same text.

### 4. Commit, open a PR, merge

```sh
git add CHANGELOG.md Cargo.lock Cargo.toml package.json .release-please-manifest.json
git commit -m "Release X.Y.Z"
git push -u origin release/vX.Y.Z
gh pr create --base main --head release/vX.Y.Z --title "Release X.Y.Z: <one-line summary>"
gh pr merge release/vX.Y.Z --merge
```

Use a merge commit (`--merge`), not squash, so each commit on the branch keeps its
own history on `main`.

### 5. Tag the merge commit

```sh
git checkout main && git pull
git tag -a vX.Y.Z -m "meet-ai X.Y.Z"
git push origin vX.Y.Z
```

### 6. Build and sign the app from the tag

```sh
git describe --tags --exact-match    # must print vX.Y.Z
just bundle-signed                   # build, sign, verify; ~3 min
```

`just bundle-signed` is `just build` then `just sign`. If the keychain could not
be unlocked yet — an agent shell may refuse to run `unlock-keychain` because the
command carries a password — run `just build` first, unlock the keychain
yourself, then run `just sign` on the finished build.

Check the result before shipping it:

```sh
APP=target/release/bundle/macos/meet-ai.app
codesign --verify --deep --strict --verbose=2 "$APP"
codesign -d -r- "$APP" 2>&1 | grep -o 'leaf = H"[0-9a-f]*"'      # eafb73d2…
/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$APP/Contents/Info.plist"   # X.Y.Z
```

### 7. Zip the app and make a checksum

Use `ditto`, not `zip`. It keeps the code signature intact when the file is
unzipped.

```sh
Z=meet-ai-X.Y.Z-macos-arm64.zip
(cd target/release/bundle/macos && ditto -c -k --sequesterRsrc --keepParent meet-ai.app "$OLDPWD/$Z")
shasum -a 256 "$Z" > "$Z.sha256"
```

### 8. Publish the GitHub release

The release notes are an Install section followed by the changelog section for
this version:

~~~sh
{ cat <<'EOF'
## Install

Download `meet-ai-X.Y.Z-macos-arm64.zip` (Apple silicon, macOS 26+), unzip, and move `meet-ai.app` to `/Applications`.

The app is signed with a self-signed local identity and is not notarized, so Gatekeeper blocks the first launch. Clear the quarantine flag once:

```sh
xattr -dr com.apple.quarantine /Applications/meet-ai.app
```

Verify the download against `meet-ai-X.Y.Z-macos-arm64.zip.sha256` with `shasum -a 256 -c`.

EOF
awk '/^## \[X.Y.Z\]/{f=1;next} /^## \[/{f=0} f' CHANGELOG.md; } > notes.md

gh release create vX.Y.Z --verify-tag --title "meet-ai X.Y.Z" \
  --notes-file notes.md "$Z" "$Z.sha256"
rm notes.md "$Z" "$Z.sha256"
~~~

Confirm it shows as Latest with both files attached:

```sh
gh release list
gh release view vX.Y.Z --json assets --jq '.assets[].name'
```

### 9. Smoke-test what you shipped

Install the zip from the release, not the local build, and make one recording.
This is the only check that runs the signed app with real audio.

```sh
cd "$(mktemp -d)"
gh release download vX.Y.Z
shasum -a 256 -c meet-ai-X.Y.Z-macos-arm64.zip.sha256
ditto -x -k meet-ai-X.Y.Z-macos-arm64.zip . && ditto meet-ai.app /Applications/meet-ai.app
xattr -dr com.apple.quarantine /Applications/meet-ai.app
open -a /Applications/meet-ai.app
```

Press ⌘⇧R, say a sentence, play some audio (`say "…"` works), press ⌘⇧R again.
Then check the newest folder in `~/Meetings`:

```sh
M=$(command ls -dt ~/Meetings/*-meeting | head -1)
cat "$M/audio/segments.json"                        # written on stop
cargo run -q -p audio --bin drift-check -- "$M/audio"   # PASS
target/meet-stt "$M/audio/system.wav"               # the audio you played
target/meet-stt "$M/audio/mic.wav"                  # what you said
```

FINDINGS §12 has the v0.3.0 run and what a pass looks like. Delete the test
meeting folder afterwards.

### When signing goes wrong

**`just sign` hangs, or fails with `errSecInternalComponent`.** The keychain is
locked. Unlock it (see the top of this file) and run `just sign` again. The app
is already built, so you don't need `just bundle-signed` again.

**`unlock-keychain` says the password is wrong.** The private key is lost. It
only ever lives inside that keychain. `~/.meet-ai/signing/cert.pem` is the
public half only and cannot sign. Make a new identity:

```sh
scripts/signing/make-identity.sh --rotate
security set-keychain-settings ~/Library/Keychains/meet-ai-signing.keychain-db
scripts/signing/make-identity.sh --print   # note the new leaf SHA-1
```

Rotating changes the certificate fingerprint. macOS keys microphone and
system-audio permission to it, so the app asks for both again. Clear the old
records and say so in the release notes:

```sh
tccutil reset AudioCapture pro.saleschat.meetai
tccutil reset Microphone   pro.saleschat.meetai
```

Then update the fingerprint wherever it is written down: this file,
`SETUP.md`, `CONTRIBUTING.md`, the header comment in
`scripts/signing/make-identity.sh`, and `EXPECT_LEAF` in
`scripts/signing/verify-tur10.sh`.

⛔ Never fall back to ad-hoc signing (`codesign -s -`). The permission grant
would then be tied to the binary itself, so every rebuild would drop it.
