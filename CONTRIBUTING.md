# Working on meet-ai

Everything below was run end to end on a clean checkout on 2026-09-27. If a step
here stops being true, fix this file in the same commit.

## What the documents are for

| File | Read it when |
|---|---|
| [`SPEC.md`](./SPEC.md) | Always, first. v2, locked decisions L1–L18, phase order and exit gates in §5. It wins over everything else. |
| [`RELEASING.md`](./RELEASING.md) | You are cutting a release: version bump, tag, signed app, GitHub release. |
| [`SETUP.md`](./SETUP.md) | You are adding or changing a dependency. It pins every version and says why. |
| [`PROBLEM.md`](./PROBLEM.md) | You want to know what the product is for. |
| [`FINDINGS.md`](./FINDINGS.md) | You want the research behind a dependency choice. |
| [`design-system/meet-ai/MASTER.md`](./design-system/meet-ai/MASTER.md) | You are writing UI. Start here rather than inventing a second visual language. |

`SETUP.md` is the source of truth for versions. Where this repo differs from it,
the difference is listed under [Where the repo differs from SETUP.md](#where-the-repo-differs-from-setupmd) — nothing drifts silently.

## Setup from a clean checkout

### 1. Prerequisites

```bash
# Rust, pinned by rust-toolchain.toml. rustup installs 1.98.0 on first use.
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

brew install just cmake
```

- **Xcode Command Line Tools** are enough. Full Xcode is not required — `swiftc`
  and `codesign` both ship with CLT.
- **cmake** is not in `SETUP.md`, but `whisper-rs` needs it to build whisper.cpp.
  Without it `cargo build` fails inside `crates/stt` on macOS.
- **Node** ≥ 20.19. **pnpm** installs itself: `package.json` pins
  `packageManager: pnpm@11.25.0`, so any recent pnpm switches to that version
  automatically inside this repo. Do not run `pnpm self-update` — it rewrites
  that pin to whatever is newest.

Verify:

```bash
rustc --version          # 1.98.0
node --version           # >= v20.19
pnpm --version           # 11.25.0  (run it inside the repo)
swiftc --version         # any
just --version           # any
sw_vers -productVersion  # >= 26.0
```

### 2. Install and check

```bash
git clone <repo> meet-ai && cd meet-ai
pnpm install
just check
```

`just check` is the one command. It runs, in order:

| Step | What it covers |
|---|---|
| `just check-windows` | `audio`, `calendar`, `stt`, `prompts`, `detect` still compile for Windows — the SPEC §8.2 seam guard. `store` and `modelfetch` are exempt; see "Where the repo differs from SETUP.md" |
| `cargo fmt --all --check` | Rust formatting |
| `cargo clippy --workspace --all-targets -- -D warnings` | Rust lint, warnings are errors |
| `cargo test --workspace` | Rust tests |
| `pnpm biome check .` | TypeScript/JSON lint + format |
| `pnpm typecheck` | `tsc --noEmit` for the app and for the build config |
| `pnpm vitest run` | Frontend tests |

`just fmt` fixes everything that is automatically fixable.

**`just check` does not run the whisper hallucination guard.** SPEC §5's Phase 1
gate — "silence produces no invented text" — has a whisper-specific test,
`whisper_writes_nothing_for_silence` in `crates/stt/tests/silence.rs`, behind
the `whisper-model-tests` feature. It is gated off `just check` on purpose: it
needs a 190 MB model on disk, and `just check` must stay something a fresh
clone can run with no network. That test is not optional, just not automatic —
run it yourself:

```bash
just model            # downloads + checksum-verifies small.en-q5_1, once
just check-whisper     # cargo test -p stt --features whisper-model-tests
```

Run `just check-whisper` whenever you touch `crates/stt/src/vad.rs`,
`crates/stt/src/whisper.rs`, the phrase blocklist, or the model catalog in
`crates/stt/src/model.rs` — those are the only things that can break this gate
— and again before any Phase 1 sign-off. Verified green against the real
`small.en-q5_1` model on 2026-09-28; see SPEC.md A9.

### 3. Run it

```bash
just dev     # Vite on :1420 + the Tauri window, hot reload
just build   # release .app at target/release/bundle/macos/meet-ai.app
just rec     # the Phase 0 capture CLI, no Tauri and no UI
```

### 4. Code signing — needed before any audio work

macOS TCC will not reliably register an unsigned app, so audio permission never
sticks. This is not optional, even for personal use.

Creating the certificate is scripted — there is no Keychain Access step. The
script asks for nothing: it puts the cert in its own keychain and sets trust in
the *user* domain, which needs no admin password (measured, TUR-10).

```bash
./spikes/phase0a-tcc/make-identity.sh   # ~7 s, no sudo, no GUI
security find-identity -v -p codesigning  # confirm "meet-ai Local Signing" appears
just bundle-signed                        # build + sign + verify
```

`just sign` defaults to that identity and to the keychain the script creates
(`~/Library/Keychains/meet-ai-signing.keychain-db`). Override either with
`SIGN_IDENTITY` / `SIGN_KEYCHAIN` for a Developer ID build.

**If `find-identity` reports `0 valid identities found`, check `$HOME` before
re-running the script.** `security` and `codesign` read the keychain search list
out of `$HOME`, and some automated runners redirect `$HOME` to a temp directory.
The certificate is fine; the lookup is pointed at an empty machine. Both sides
of this reproduce here:

```bash
$ HOME=/Users/<you> security find-identity -v -p codesigning
  1) EAFB73D2… "meet-ai Local Signing"
     1 valid identities found
$ HOME="$TMPDIR" security find-identity -v -p codesigning
     0 valid identities found
```

Re-running `make-identity.sh` under the redirected `$HOME` just creates a second
cert in a keychain nothing will search. Set `HOME` back instead. This applies to
any keychain-touching command, `just sign` included.

**Do not ad-hoc sign instead.** Under ad-hoc signing TCC keys the grant to the
executable's cdhash, so every rebuild silently drops audio permission and
`tccutil reset AudioCapture pro.saleschat.meetai` becomes a no-op. With this
identity the designated requirement is `identifier "pro.saleschat.meetai" and
certificate leaf = H"be3f…"` — bundle ID plus a stable cert — and the grant
survives rebuilds (FINDINGS §10.4).

**`just dev` is not a valid environment for testing audio permission.** TCC keys
on the signed bundle identity, and dev builds are unsigned at a different path.
Only `just bundle-signed` proves anything about permissions.

## Layout

```
crates/audio/      🔴 tap + mic + resample + wav. Has bin/meet-rec.
crates/stt/        🟡 SttEngine trait, sidecar driver, whisper fallback, transcript format
crates/modelfetch/ 🟡 whisper model download. Has bin/meet-stt-model. The only
                      crate in the speech path with an HTTP client — see below.
crates/store/      🟢 markdown, frontmatter, watcher, derived SQLite index
crates/prompts/    🟢 minijinja templates + assembly
crates/calendar/   🟡 CalendarProvider trait: eventkit | google | microsoft | ics
crates/detect/     🟢 process + audio-activity heuristics
sidecar/meet-stt/  🟡 Swift CLI: SpeechTranscriber. JSON lines on stdout.
src-tauri/         🟡 commands, events, tray, plugins, state machine
src/               🟢 React app
design-system/     the visual language. Imported by src/index.css, not copied.
```

Every crate compiles today and has at least one test. They are deliberately
near-empty: the traits and data contracts that other people's code has to fit
through are real, and the implementations are not written yet.

## Rules that are easy to break by accident

- **Read docs.rs for the exact pinned version** before writing against `cpal`
  0.18, `rubato` 5.0, `ringbuf` 0.5, `rusqlite` 0.40, `keyring` 4.2, `vite` 8,
  `react-router` 8, `tailwindcss` 4, `vitest` 4, `lucide-react` 1, `typescript`
  7. Model memory is a liability for all of these — it produces a
  plausible-looking API that silently does not exist.
- **`objc2` 0.6.4 alongside `objc2-*` 0.3.2 is correct.** Framework crates
  version independently. Never "align" them.
- **Tailwind v4 has no `tailwind.config.js`.** Config is CSS-first, in
  `src/index.css`. There is deliberately no such file in this repo.
- **`Cargo.lock` and `pnpm-lock.yaml` are committed and they matter.** See the
  Tauri note below.
- **Bundle id `pro.saleschat.meetai` is frozen.** macOS TCC keys audio
  permission to it; renaming it silently revokes consent on every existing
  install, with no migration path.
- **No AI calls, no API keys, no telemetry, ever.** L9/L10/L11. Nothing in this
  codebase may make an outbound request carrying meeting content, titles, or
  filenames.
- **Never commit a key, a recording, or a real transcript.**

## Agent runs and the working tree

Automated runs used to share this one checkout. Two runs editing the same files
at the same time cost us `crates/modelfetch/src/lib.rs`, which existed only in
the working tree and was gone by the time anyone looked (TUR-16). Two things now
stand between us and a repeat.

**Each run gets its own git worktree.** Worktrees are created under
`../.meet-ai-worktrees/`, outside this repo, branched from `main`. Inside one,
`git rev-parse --show-toplevel` resolves to the worktree — a run cannot reach
this checkout by walking up the directory tree, which is how it used to happen.
Note that worktrees do not carry `target/` or `node_modules/`, because both are
ignored: a fresh worktree pays a full cold build.

**The tree is snapshotted, not trusted.** `.claude/hooks/tree-snapshot.sh` commits
the entire working tree — tracked changes *and* untracked, non-ignored files — to
a ref under `refs/tree-snapshot/`. It runs at session start, at session end, and
before any Bash command that looks like it discards state (`clean`, `reset`,
`restore`, `stash`, a destructive `checkout`, `rm -r`). It writes through a
throwaway index, so it never touches your staging area or a single file on disk,
and it is safe to run mid-edit.

```bash
.claude/hooks/tree-snapshot.sh list          # snapshots, newest first
.claude/hooks/tree-snapshot.sh show <ref>    # what that snapshot changed
git checkout <ref> -- path/to/file           # get a file back
```

Snapshot refs live in the shared ref namespace, so a snapshot taken in one
worktree is recoverable from any of them. The newest 300 are kept.

A run that ends with untracked files under `crates/`, `src/`, `src-tauri/`,
`sidecar/` or `design-system/` gets a loud warning naming them, and the snapshot
holds a copy — but it is **not** blocked. Commit your own paths before you
finish; the snapshot is a safety net, not a substitute.

One hazard the net does not cover: `git clean -xdf` at the repo root deletes
`.paperclip/`, which is ignored and holds the live Paperclip instance database.
Snapshots skip ignored paths, so nothing brings that back. Do not run it here.

## Where the repo differs from SETUP.md

Five differences. Each one is a `SETUP.md` step that does not work as written.

1. **`rusqlite` features are `["bundled"]`, not `["bundled", "fts5"]`.**
   rusqlite 0.40.2 has no `fts5` feature, so asking for it fails to resolve. The
   `bundled` build already compiles SQLite with `SQLITE_ENABLE_FTS5`. The
   `fts5_is_available` test in `crates/store/src/lib.rs` proves that rather than
   trusting the claim.

2. **`just check-windows` covers everything except `store` and `modelfetch`.**
   Both exemptions have the same cause: a dependency whose build script
   compiles C **for the target**, which needs an MSVC toolchain no Mac has.
   Including either would make the check permanently red for a reason unrelated
   to our code, and a permanently-red guard is a guard nobody reads.

   - `store` — `rusqlite`'s `bundled` feature compiles `sqlite3.c`.
   - `modelfetch` — `reqwest`'s `rustls-tls` pulls `ring`, which compiles
     `curve25519.c`. The exact failure is
     `fatal error: 'assert.h' file not found` out of `cc-rs` with
     `--target=x86_64-pc-windows-msvc`. Switching rustls to `aws-lc-rs` is not
     an escape — it compiles C too. Put it back if rustls ever ships a usable
     pure-Rust crypto provider.

   `stt` *is* covered, and that is why `modelfetch` exists as a separate crate
   at all. The model downloader started out inside `crates/stt`, and it took the
   whole speech-to-text crate out of the guard with it (TUR-13). Splitting the
   ~200 lines of HTTP into their own crate means the exempt surface is one file
   with no `#[cfg(target_os)]` in it, instead of every line of transcription
   code. `whisper-rs` is gated to macOS in `stt`'s `Cargo.toml`, so nothing else
   in there compiles C for the target either.

   `audio` and `calendar` are covered too — `audio` is the crate the seam guard
   mainly exists for, and it was missing from the recipe until TUR-2 follow-up.

3. **`cmake` is a prerequisite.** `SETUP.md` step 0.2 omits it; `whisper-rs`
   does not build without it.

4. **The Tauri crates are pinned in `Cargo.lock`, below the versions Cargo would
   pick.** `tauri` 2.11.5 declares `tauri-runtime ^2.11.3`, `tauri-runtime-wry
   ^2.11.4` and `tauri-macros ^2.6.3`, but the 2.12/2.7 releases that satisfy
   those ranges are not actually source-compatible with 2.11.5. A fresh
   `cargo update` produces a tree that does not compile. The lockfile holds
   `tauri-runtime` 2.11.3, `tauri-runtime-wry` 2.11.4 and `tauri-macros` 2.6.3.
   **Do not run a blanket `cargo update`.** Update one crate at a time and run
   `just check`.

5. **`@types/node` 26.6.3 was added.** Not in `SETUP.md`, but `vite.config.ts`
   imports `node:url` and will not typecheck without it. It is confined to
   `tsconfig.node.json` — the app's `tsconfig.json` deliberately has no Node
   types, because code in `src/` runs in WKWebView where `node:fs` does not
   exist.

Two more things worth knowing:

- **`just sign` and `just bundle-signed` target `target/release/bundle/...`**,
  not `src-tauri/target/...` as `SETUP.md` step 6 says. Once the workspace
  `Cargo.toml` exists at the repo root, that is where cargo puts output.
- **The updater keypair from `SETUP.md` step 1.2 has been generated.** The public
  key is in `src-tauri/tauri.conf.json`. The private key was filed as a
  Paperclip secret, `meet-ai/tauri-updater/private-key`, and exists nowhere in
  this repo. The updater plugin is wired but `active: false`, per L17 and
  SPEC §8.1.
