# Working on meet-ai

Everything below was run end to end on a clean checkout on 2026-09-27. If a step
here stops being true, fix this file in the same commit.

## What the documents are for

| File | Read it when |
|---|---|
| [`SPEC.md`](./SPEC.md) | Always, first. v2, locked decisions L1–L18, phase order and exit gates in §5. It wins over everything else. |
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
sw_vers -productVersion  # >= 14.4
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
| `just check-windows` | `stt`, `prompts`, `detect` still compile for Windows — the SPEC §8.2 seam guard |
| `cargo fmt --all --check` | Rust formatting |
| `cargo clippy --workspace --all-targets -- -D warnings` | Rust lint, warnings are errors |
| `cargo test --workspace` | Rust tests |
| `pnpm biome check .` | TypeScript/JSON lint + format |
| `pnpm typecheck` | `tsc --noEmit` for the app and for the build config |
| `pnpm vitest run` | Frontend tests |

`just fmt` fixes everything that is automatically fixable.

### 3. Run it

```bash
just dev     # Vite on :1420 + the Tauri window, hot reload
just build   # release .app at target/release/bundle/macos/meet-ai.app
just rec     # the Phase 0 capture CLI, no Tauri and no UI
```

### 4. Code signing — needed before any audio work

macOS TCC will not reliably register an unsigned app, so audio permission never
sticks. This is not optional, even for personal use.

Create a self-signed code-signing certificate named `meet-ai-dev`:

> Keychain Access → Certificate Assistant → Create a Certificate…
> Name: `meet-ai-dev` · Identity Type: Self Signed Root · Type: Code Signing

```bash
security find-identity -v -p codesigning   # confirm meet-ai-dev appears
just bundle-signed                         # build + sign + verify
```

Use a different name by exporting `SIGN_IDENTITY`.

**`just dev` is not a valid environment for testing audio permission.** TCC keys
on the signed bundle identity, and dev builds are unsigned at a different path.
Only `just bundle-signed` proves anything about permissions.

## Layout

```
crates/audio/      🔴 tap + mic + resample + wav. Has bin/meet-rec.
crates/stt/        🟡 SttEngine trait, sidecar driver, whisper fallback, transcript format
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

## Where the repo differs from SETUP.md

Five differences. Each one is a `SETUP.md` step that does not work as written.

1. **`rusqlite` features are `["bundled"]`, not `["bundled", "fts5"]`.**
   rusqlite 0.40.2 has no `fts5` feature, so asking for it fails to resolve. The
   `bundled` build already compiles SQLite with `SQLITE_ENABLE_FTS5`. The
   `fts5_is_available` test in `crates/store/src/lib.rs` proves that rather than
   trusting the claim.

2. **`just check-windows` covers `stt`, `prompts` and `detect` — not `store`.**
   `rusqlite`'s `bundled` feature compiles `sqlite3.c` for the *target*, which
   needs an MSVC toolchain no Mac has. Including `store` would make the check
   permanently red for a reason unrelated to our code. `stt` *is* covered,
   because `whisper-rs` is gated to macOS in its `Cargo.toml`.

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
