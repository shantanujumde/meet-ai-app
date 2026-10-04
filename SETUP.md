# meet-ai — Dependency Research & Initial Setup

**Verified:** 2026-09-01 against crates.io + npm registry live APIs. Not from memory.
**Companion to:** [`SPEC.md`](./SPEC.md) §2 (tech stack), §5 (phases), §9 (next steps).
**Note:** SPEC amendment **A2** added a Swift sidecar for Apple's STT — see §1.5 below for what that changes here.

---

## 0. This machine, right now

| Tool | Found | Needed | Action |
|---|---|---|---|
| macOS | **26.6.2** | 26.0+ | ✅ at or above floor (SPEC A8) |
| Xcode Command Line Tools | present (`/Library/Developer/CommandLineTools`) | CLT only | ✅ **full Xcode not required** — L2 drops ScreenCaptureKit, and `objc2` needs no Xcode project. `codesign` ships with CLT |
| Xcode (full) | **27.0** at `/Applications/Xcode.app` | 26+, **only to regenerate the app icon** | one-time setup in step 0.4, only on a machine that recompiles `src-tauri/icons/Assets.car`. Routine builds never touch it (SPEC A10) |
| Rust | ❌ **not installed** | 1.98.0 | install via rustup (step 0.1) |
| Node | v26.4.0 | ≥20.19 | ✅ works. Current LTS is v24.20.0 "Krypton" — switch only if a tool complains |
| pnpm | 11.9.0 | 11.25.0 | `pnpm self-update` |
| just | ❌ not installed | latest | `brew install just` |
| **`swiftc`** | **6.3.3**, target `arm64-apple-macosx26.0` | any | ✅ **verified present** — ships with CLT. A2's sidecar builds today, nothing to install |
| **`Speech.framework`** | present | macOS 26 | ✅ **verified** — `SpeechAnalyzer` / `SpeechTranscriber` available |
| ffmpeg | unverified | any | `brew install ffmpeg` — needed once, for the fixture WAVs (`just fixtures`) |

---

## 1. Research findings that change the spec

Five substantive changes came out of checking real registry data plus SPEC amendment A2. All are simplifications.

### 1.1 VAD: drop Silero + ONNX entirely → `earshot`

`voice_activity_detector` 0.2.1 (the Silero wrapper in the spec) pins **`ort =2.0.0-rc.10`** — an exact-version dependency on a *pre-release* ONNX runtime, while `ort` is now at rc.13. Exact pins on pre-releases are a dependency-resolution trap, and it drags a whole ONNX runtime + a bundled model into the build.

**`earshot` 1.2.2** (2026-08-19, 170k downloads) is a pure-Rust WebRTC VAD port whose only runtime dependency is `libm`.

| | `voice_activity_detector` (Silero) | **`earshot` (WebRTC)** |
|---|---|---|
| Runtime deps | `ort` (pinned pre-release), `ort-sys`, `ndarray`, `futures`, `pin-project`, `typed-builder` | **`libm`** |
| Bundled model | `silero_vad.onnx` ~2MB | none |
| Accuracy | better on noisy input | adequate for utterance boundaries |
| Build friction | ONNX runtime per-platform | none |

**Decision:** `earshot` for v1, behind a `Vad` trait. If Phase 1's silence test shows whisper hallucinating on noise, swap in Silero-via-`ort` directly (~60 lines, latest rc, no wrapper crate). This also removes `ort` — a pre-release dependency — from v1 completely, and drops one model from §2.4.

### 1.2 YAML: `serde_yaml` is deprecated → `yaml-rust2`

`serde_yaml`'s latest version is literally **`0.9.34+deprecated`**, untouched since March 2024. Not a foundation for the frontmatter layer.

Also relevant: `gray_matter` 0.3.2 already uses **`yaml-rust2`** under the hood, not serde_yaml.

**Decision:** use **`yaml-rust2` 0.12.0 directly** and drop `gray_matter`. Reason beyond deprecation — SPEC §2.3 requires frontmatter round-trips to *preserve unknown keys*. Deserializing into a struct loses them by definition. `yaml-rust2` gives an ordered `Yaml` value plus an emitter, so the flow is: parse → mutate the keys you own → emit. Unknown keys survive because they're never modelled. The `---` splitter is ~15 lines and removes a dependency.

### 1.3 Config: `json_comments` → `jsonc-parser`

`json_comments` 0.2.2 last shipped in **2023**. `jsonc-parser` 0.33.1 shipped July 2026, 9.8M downloads, and is what the VS Code-adjacent Rust ecosystem uses. Straight swap.

### 1.4 Newest is not always best for vibe-coding

Several dependencies shipped new majors recently. An LLM's training data skews to the *previous* major, so it will confidently write the old API. Where the new major buys nothing this project needs, take the well-trodden version instead.

| Crate | Latest | **Use** | Why |
|---|---|---|---|
| `sha2` | 0.11.0 | **0.10.9** | Checksumming a download. 0.11 offers nothing here; 0.10 is the API every model knows |
| `reqwest` | 0.13.4 | **0.12.28** | Range-header resume + rustls identical in both. 0.12 has vastly more examples in the wild |
| `notify` | 9.0.0-rc.5 | **8.2.0** | 9.0 is a release candidate. Don't build the file watcher on an rc |

For the ones where latest *is* the right call, the mitigation is procedural — see §4 rule.

### 1.5 SPEC A2 — Swift sidecar demotes whisper to fallback

SPEC amendment A2 makes Apple `SpeechTranscriber` (macOS 26, 0 MB, ~2× faster than whisper large-v3-turbo, native streaming) the default STT engine via a Swift CLI at `sidecar/meet-stt`. Effects on this document:

| Item | Change |
|---|---|
| `whisper-rs` 0.16 | **Still pinned and still added** — it is the fallback engine for macOS < 26 and the Windows floor. Just no longer on the critical path |
| `earshot` 1.2.2 | Still needed, but only for the whisper path. macOS 26 uses `SpeechDetector` inside the sidecar |
| Model download manager | Moves off Phase 1's critical path. Lazy — nothing downloads on macOS 26 |
| New toolchain requirement | `swiftc`, which **ships with Command Line Tools already present on this machine**. No Xcode, no SwiftPM registry, no extra install |
| New build step | `just sidecar` → `swiftc -O sidecar/meet-stt/main.swift -o …/Contents/MacOS/meet-stt`, then sign with the same identity |
| 🆕 risk | `SpeechAnalyzer` shipped with macOS 26, so LLM training data is thin. Read Apple's docs and [`FluidInference/swift-scribe`](https://github.com/FluidInference/swift-scribe) first — same rule as §4 |
| Later, free | FluidAudio (pyannote diarization on the Neural Engine) drops into the same sidecar, un-deferring real N-speaker labels |

Windows, for the record: it has its own free on-device STT (Windows AI Speech Recognition) which is **WinRT and therefore callable directly from Rust via the `windows` crate — no sidecar needed there**.

---

## 2. Verified versions — Rust

Pin these exactly in `Cargo.toml` and commit `Cargo.lock`. 🆕 = new major with thin LLM training data; read docs.rs for that exact version before writing code.

### 2.1 Toolchain & shell

| Crate | Version | Notes |
|---|---|---|
| Rust toolchain | **1.98.0** (2026-08-18) | `rust-toolchain.toml`, pinned. Tauri MSRV is 1.77.2 |
| `tauri` | **2.11.5** | |
| `tauri-build` | **2.6.3** | build-dependency |
| `tauri-plugin-clipboard-manager` | **2.3.3** | L14 Start Work and the L9 copy-prompt fallback (SPEC A11) |
| `tauri-plugin-notification` | **2.4.0** | |
| `tauri-plugin-global-shortcut` | **2.3.2** | |
| `tauri-plugin-dialog` | **2.7.3** | |
| `tauri-plugin-opener` | **2.5.5** | |
| `tauri-plugin-fs` | **2.5.2** | |
| `tauri-plugin-log` | **2.9.1** | |
| `tauri-plugin-single-instance` | **2.4.4** | |
| `tauri-plugin-autostart` | **2.5.1** | Start at login (TUR-58); 2.6+ needs tauri 2.12 |
| `tauri-plugin-updater` | **2.11.0** | added now, `active: false` — §8.1 ⛔ seam |

### 2.2 Audio (🔴 the risky crate)

| Crate | Version | Notes |
|---|---|---|
| `objc2` | **0.6.4** | |
| `objc2-core-audio` | **0.3.2** | ⚠️ **The version skew vs `objc2` 0.6.4 is correct.** Framework crates version independently of `objc2`. Do not let an agent "fix" this |
| `objc2-core-audio-types` | **0.3.2** | |
| `objc2-core-foundation` | **0.3.2** | 🆕 not in the original list — the process tap's aggregate-device description is a `CFDictionary`, and this crate is where `objc2-core-audio` sources that type from. Already resolved transitively at 0.3.2 before this was made a direct dependency, so pinning it added no version churn |
| `objc2-foundation` | **0.3.2** | |
| `objc2-av-foundation` | **0.3.2** | 🆕 TUR-127 — SPEC §8.1 named `AVCaptureDevice.authorizationStatus(for: .audio)` as the mic's likely exemption from the process tap's "every `OSStatus` lies" problem, but left it unverified; a field denial proved `cpal`'s own return code cannot be trusted for that case, so `permission_check::check_mic` now asks this API first. `default-features = false`, only the `AVCaptureDevice`+`AVMediaFormat` features — the crate's ~170 default features are the whole framework surface, none of the rest needed |
| `block2` | **0.6.2** | needed for Core Audio tap IO callbacks |
| `cpal` | **0.18.2** 🆕 | mic capture. LLMs know 0.15 — the device/stream API changed |
| `rubato` | **5.0.0** 🆕 | 48k→16k resample. Went 0.16 → 5.0; almost no training data on 5.x |
| `ringbuf` | **0.5.1** 🆕 | lock-free audio→worker handoff |
| `hound` | **3.5.1** | WAV I/O. Stable since 2023, zero risk |
| `earshot` | **1.2.2** | VAD (§1.1). Only dep is `libm` |
| `wasapi` | **0.25.0** | 🆕 TUR-60, Windows only. MIT. Safe wrappers over WASAPI's `IAudioSessionManager2` session list, for "mic and speakers in use by another app". Pure Rust over the `windows` crate. Moved out of §2.7: SPEC A13 makes Windows a target now |
| `libpulse-binding` | **2.30.1** | 🆕 TUR-60, Linux only. MIT OR Apache-2.0 (we take MIT). The PulseAudio client API: source-output and sink-input lists. Links `libpulse.so` (`libpulse-dev` at build time); PipeWire desktops answer it through pipewire-pulse |
| `winreg` | see `Cargo.toml` | Windows only (TUR-51): the microphone consent registry keys |

### 2.3 Speech-to-text

| Crate | Version | Notes |
|---|---|---|
| `whisper-rs` | **0.16.0** | wraps `whisper-rs-sys` 0.15 |

Features confirmed present on 0.16.0: `metal`, `coreml`, `cuda`, `vulkan`, `hipblas`, `intel-sycl`, `openblas`, `openmp`, `tracing_backend`, `raw-api`.

- v1 build: `features = ["metal", "tracing_backend"]`
- **`coreml` is a free future speedup** — runs the encoder on the Neural Engine. Needs a generated CoreML model, so it's a post-Phase-1 optimisation, not a v1 task.
- `vulkan` / `cuda` are the declared-but-unused Windows seam from SPEC §8.2.

### 2.4 Store, config, prompts

| Crate | Version | Notes |
|---|---|---|
| `rusqlite` | **0.40.2** 🆕 | features `["bundled", "fts5"]`. LLMs know ~0.29–0.31 |
| `notify` | **8.2.0** | not 9.0-rc (§1.4) |
| `notify-debouncer-full` | **0.7.0** | |
| `yaml-rust2` | **0.12.0** | frontmatter parse + emit (§1.2) |
| `serde` | **1.0.229** | `derive` |
| `serde_json` | **1.0.151** | |
| `jsonc-parser` | **0.33.1** | config (§1.3) |
| `minijinja` | **2.24.0** | prompt templates |
| `jsonschema` | **0.58.4** | notes-schema check (SPEC A11). `default-features = false`: the defaults pull `reqwest` for remote `$ref`s |
| `dirs` | **6.0.0** | ⛔ every path via this — Windows seam |
| `chrono` | **0.4.45** | |

### 2.5 Calendar & auth (Phase 5)

| Crate | Version | Notes |
|---|---|---|
| `objc2-event-kit` | **0.3.2** | tier 1, zero-auth |
| `oauth2` | **5.0.0** | PKCE. Stable since Jan 2025 |
| `icalendar` | **0.17.13** | tier 4 ICS |
| `keyring` | **4.2.0** 🆕 | macOS Keychain → Windows Credential Manager free. LLMs know v2/v3 |
| `reqwest` | **0.12.28** | see §1.4. features `["json", "rustls-tls", "stream"]` |
| `sha2` | **0.10.9** | see §1.4 |
| `tokio-util` | **0.7.19** | download streaming |
| `base64` | **0.22.1** | TUR-44: reads the account label out of the id_token. The version `oauth2` already pulls |
| `chrono-tz` | **0.10.4** | TUR-47: a Windows zone name from Microsoft Graph, mapped to IANA, becomes a real offset. Pure Rust, tz database compiled in |
| `url` | **2.5.8** | TUR-86: Join links are parsed with the WHATWG parser browsers use, so the allow-list sees the host a browser opens. The version `oauth2` already pulls |

TUR-44 (SPEC A12) added `oauth2` (no default features in `crates/calendar`;
`reqwest-blocking` only in `src-tauri`, so the crate keeps no network client and
still cross-checks for Windows), `keyring` 4.2 (default `v1` feature: Keychain,
Windows Credential Manager, Secret Service through zbus, all pure Rust) and
`tauri-plugin-oauth` 2.1.0 (Rust API only, the JS plugin was never
registered). TUR-88 removed `tauri-plugin-oauth` again: it read each callback
with one 4048-byte `read`, so the loopback listener is now our own
(`src-tauri/src/calendar/loopback.rs`, std only). `icalendar` is not added: A12
dropped ICS.

#### Calendar sign-in (optional)

Sign-in with Google or Microsoft needs an OAuth client of your own; without one
the app still reads EventKit, and the sign-in commands answer
`calendar-not-configured` naming the missing key.

1. **Google.** In Google Cloud Console, create an OAuth client of type
   **Desktop app**, and enable the Google Calendar API. On the OAuth consent
   screen, set the publishing status to **In production**: in *Testing*,
   refresh tokens die after 7 days (SPEC §7). Unverified is fine; users see a
   one-time "Google hasn't verified this app" screen. Google gives a Desktop
   client a client secret that it treats as non-secret; it must still be sent,
   so copy it too.
2. **Microsoft.** In Microsoft Entra, register an app for "Accounts in any
   organizational directory and personal Microsoft accounts", with no client
   secret, and turn on **Allow public client flows**. The portal rejects an
   `http://127.0.0.1` redirect, so add it in the app's **Manifest** under
   `replyUrlsWithType`:
   `{ "url": "http://127.0.0.1", "type": "InstalledClient" }`. Microsoft
   ignores the port when matching a loopback redirect, so the random port the
   app picks matches. Do not use `[::1]`: Entra does not support it.
3. Paste the ids into `~/Meetings/.app/config.jsonc`. No restart needed:

   ```jsonc
   "calendar": {
     "google": {
       "client_id": "123456789012-abc.apps.googleusercontent.com",
       "client_secret": "GOCSPX-…"
     },
     "microsoft": { "client_id": "00000000-0000-0000-0000-000000000000" }
   }
   ```

The refresh token is kept in the OS keystore under service
`pro.saleschat.meetai`, user `calendar-google` / `calendar-microsoft`. On Linux
that needs a running Secret Service (gnome-keyring or KWallet); without one,
sign-in works until the app quits. Real sign-in, restart and refresh on each OS
are in `docs/manual-checks/worktree-tur44.md`.

**Reading the Google calendar (TUR-48).** Add `"google"` to
`calendar.providers` (for example `["eventkit", "google"]`, or `["google"]`
on Linux or a Mac whose Calendar app has no Google account). It is read only
while signed in; until then it is skipped. The app reads the account's
**primary** calendar with `events.list` (`singleEvents=true`, so each
occurrence of a repeating meeting is its own event), under the read-only scope
`calendar.events.readonly`. Cancelled, declined and all-day events are
skipped. The first sign-in shows "Google hasn't verified this app": choose
**Advanced → Go to … (unsafe)** once; this is expected for a client of your
own (SPEC §7, accepted under L17). If today's meetings stop showing after
about a week, the consent screen is still in *Testing*: switch it to
**In production** and sign in again. A `403` naming `accessNotConfigured` in
the log means the Google Calendar API is not enabled in that Cloud project.
Real-account checks are in `docs/manual-checks/worktree-tur48.md`.

### 2.6 Runtime, logging, testing

| Crate | Version | Notes |
|---|---|---|
| `tokio` | **1.53.1** | `["rt-multi-thread","macros","fs","process","sync","time"]` |
| `sysinfo` | **0.39.6** | process detection |
| `tracing` | **0.1.44** | |
| `tracing-subscriber` | **0.3.23** | |
| `crash-handler` | **0.7.0** | MIT OR Apache-2.0. Writes a local crash file for native crashes (TUR-46). `tracing-appender` was dropped: tauri-plugin-log writes and rotates the log file |
| `thiserror` | **2.0.20** | libs |
| `anyhow` | **1.0.104** | binaries |
| `insta` | **1.48.0** | snapshot tests |

### 2.7 Recorded for later — do NOT add in v1

| Crate | Version today | For |
|---|---|---|
| `rmcp` | 3.2.0 | MCP server (L12, v1.1) |
| `sqlite-vec` | 0.1.9 | semantic search (L8) |
| `ort` | 2.0.0-rc.13 | Silero VAD upgrade, or Parakeet |
| `sherpa-rs` | 0.6.8 | N-speaker diarization |
| `parakeet-rs` | 0.3.7 | low-latency streaming STT |

**Dropped from SPEC §2.3 entirely:** `serde_yaml` (deprecated), `gray_matter` (redundant), `json_comments` (stale), `voice_activity_detector` + `ort` (§1.1).

---

## 3. Verified versions — frontend

| Package | Version | Notes |
|---|---|---|
| `react` / `react-dom` | **19.2.8** | |
| `@types/react` | **19.2.18** | |
| `@types/react-dom` | **19.2.5** | |
| `typescript` | **7.0.2** 🆕 | The native port. `tsc --noEmit` is dramatically faster. Vite strips types itself, so TS is type-check-only — low blast radius. Fallback if a plugin breaks: `5.9.x` |
| `vite` | **8.2.2** 🆕 | LLMs know 5/6 |
| `@vitejs/plugin-react` | **6.1.1** | |
| `tailwindcss` + `@tailwindcss/vite` | **4.3.3** 🆕 | v4 config is CSS-first — no `tailwind.config.js`. LLMs default to v3 syntax |
| `zustand` | **5.0.15** | |
| `react-router` | **8.3.1** 🆕 | LLMs know v6. Declarative mode, 4 routes |
| `@tanstack/react-virtual` | **3.14.10** | |
| `react-markdown` | **10.1.0** | |
| `remark-gfm` | **4.0.1** | |
| `sonner` | **2.0.8** | |
| `lucide-react` | **1.39.0** 🆕 | v1. LLMs know `0.4xx` |
| `clsx` / `tailwind-merge` / `class-variance-authority` | **2.1.1** / **3.6.0** / **0.7.1** | shadcn/ui prerequisites |
| `@biomejs/biome` | **2.5.11** | replaces eslint + prettier |
| `vitest` | **4.1.11** 🆕 | LLMs know 1/2 |
| `@testing-library/react` | **16.3.3** | |
| `@testing-library/jest-dom` | **7.0.1** | |
| `@testing-library/user-event` | **14.6.7** | devDependency. Real key presses in tests: jsdom alone does not move a radio group on the arrow keys (TUR-73) |
| `jsdom` | **30.0.1** | |
| `@tauri-apps/api` | **2.11.1** | |
| `@tauri-apps/cli` | **2.11.4** | devDependency |
| `@tauri-apps/plugin-*` | match the Rust plugin versions in §2.1 | clipboard-manager 2.3.3, notification 2.4.0, global-shortcut 2.3.2, dialog 2.7.3, opener 2.5.5, fs 2.5.2, log 2.9.1, updater 2.11.0 |
| `pnpm` | **11.25.0** | |

**JS/Rust plugin versions must move together.** A mismatched `@tauri-apps/plugin-x` and `tauri-plugin-x` fails at runtime with an unhelpful IPC error.

---

## 4. The 🆕 rule

Eleven dependencies are new majors where model memory is a liability: `cpal`, `rubato`, `ringbuf`, `rusqlite`, `keyring`, `vite`, `react-router`, `tailwindcss`, `vitest`, `lucide-react`, `typescript`.

Plus one framework with the same problem for a different reason: **Apple `SpeechAnalyzer` / `SpeechTranscriber` shipped with macOS 26**, so there is very little of it in any training set. Treat it identically — Apple's docs and [`swift-scribe`](https://github.com/FluidInference/swift-scribe) before code.

**Rule for every one of them: read the docs.rs / official docs page for the exact pinned version before writing the first line against it. Never write from memory.** The failure mode is not a compile error — it's a plausible-looking API that silently doesn't exist, followed by an agent "fixing" the version number to match its memory.

Put this in `CLAUDE.md` at the repo root, plus the two specific traps:
- `objc2` 0.6.4 alongside `objc2-*` 0.3.2 is **correct** — never "align" them.
- Tailwind v4 has **no** `tailwind.config.js` — config lives in CSS.

---

## 5. Setup steps

### Step 0 — prerequisites

```bash
# 0.1 Rust (not currently installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# 0.2 tooling
brew install just
pnpm self-update                      # 11.9.0 -> 11.25.0

# 0.3 verify
rustc --version                        # expect 1.98.0
xcode-select -p                        # CLT path is fine; full Xcode not needed
sw_vers -productVersion                # 26.6.2 — at or above the 26.0 floor

# 0.4 ONLY if you regenerate the app icon (SPEC A10). Needs Xcode 26+ installed.
#     Full paths because xcode-select stays on CLT — do not `xcode-select -s`.
#     `just icon-car` points at Xcode by itself through DEVELOPER_DIR.
#     Opening Xcode.app once and accepting its prompts does the same two steps.
sudo /Applications/Xcode.app/Contents/Developer/usr/bin/xcodebuild -license accept
sudo /Applications/Xcode.app/Contents/Developer/usr/bin/xcodebuild -runFirstLaunch
# --find alone is not enough: it resolves before -runFirstLaunch, and actool
# then fails with "A required plugin failed to load".
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun actool --version   # prints a plist
```

### Step 1 — ⛔ irreversibles, before any feature code

```bash
cd ~/apps/meet-ai

# 1.1 Apache-2.0 license — REPLACES the MIT LICENSE already committed.
#     Sole author, so the relicense is yours to make; MIT stays in prior commits.
curl -sL https://www.apache.org/licenses/LICENSE-2.0.txt -o LICENSE
cat > NOTICE <<'EOF'
meet-ai
Copyright 2026 Shantanu Jumde

This product includes software developed as part of the meet-ai project.
EOF

# 1.2 Updater keypair. Public key goes in tauri.conf.json; PRIVATE key to your
#     password manager and NOWHERE else. Without this, v1 installs can never
#     auto-update to v2.
pnpm dlx @tauri-apps/cli@2.11.4 signer generate -w ~/.tauri/meet-ai.key
#     -> copy the printed PUBLIC key for step 3.4

# 1.3 .gitignore before the first commit
cat > .gitignore <<'EOF'
target/
node_modules/
dist/
*.key
.DS_Store
EOF

# NOTE: the repo already has commits — do NOT run `git init`.
git add -A && git commit -m "chore: relicense to Apache-2.0, add NOTICE and gitignore"
```

**Bundle identifier is decided now and never changes: `pro.saleschat.meetai`.** macOS TCC keys audio permission to it; renaming later silently revokes consent on every existing install with no migration path.

### Step 2 — scaffold the Tauri app

```bash
pnpm create tauri-app@latest meet-ai-app -- --template react-ts --manager pnpm
# then flatten into the repo root (or keep as src/ + src-tauri/ per SPEC §4)
```

Then set the identifier in `src-tauri/tauri.conf.json`:

```jsonc
{
  "productName": "meet-ai",
  "identifier": "pro.saleschat.meetai",
  "bundle": {
    "macOS": { "minimumSystemVersion": "26.0" }
  }
}
```

### Step 3 — Rust workspace

```bash
# 3.1 workspace root Cargo.toml
cat > Cargo.toml <<'EOF'
[workspace]
members = ["crates/*", "src-tauri"]
resolver = "3"

[workspace.package]
edition = "2024"
rust-version = "1.98.0"
license = "Apache-2.0"
EOF

cat > rust-toolchain.toml <<'EOF'
[toolchain]
channel = "1.98.0"
components = ["rustfmt", "clippy"]
targets = ["aarch64-apple-darwin"]
EOF

# 3.2 crates per SPEC §4
for c in audio stt store prompts calendar detect; do cargo new --lib "crates/$c"; done
cargo new --bin crates/audio/../../crates/audio 2>/dev/null || true   # meet-rec bin added manually
```

Add `crates/audio/src/bin/meet-rec.rs` — Phase 0 needs no Tauri and no UI.

```bash
# 3.3 dependencies, exact verified versions
cd crates/audio && cargo add \
  objc2@0.6.4 objc2-core-audio@0.3.2 objc2-core-audio-types@0.3.2 \
  objc2-foundation@0.3.2 block2@0.6.2 \
  cpal@0.18.2 rubato@5.0.0 ringbuf@0.5.1 hound@3.5.1 earshot@1.2.2 \
  thiserror@2.0.20 tracing@0.1.44 dirs@6.0.0 serde@1.0.229 --features serde/derive
cd ../stt && cargo add whisper-rs@0.16.0 --features metal,tracing_backend   # fallback engine
cargo add hound@3.5.1 earshot@1.2.2 thiserror@2.0.20 tracing@0.1.44
cd ../store && cargo add rusqlite@0.40.2 --features bundled,fts5
cargo add yaml-rust2@0.12.0 notify@8.2.0 notify-debouncer-full@0.7.0 \
  jsonc-parser@0.33.1 serde@1.0.229 serde_json@1.0.151 chrono@0.4.45 \
  dirs@6.0.0 thiserror@2.0.20 tracing@0.1.44
cd ../prompts && cargo add minijinja@2.24.0 serde@1.0.229 serde_json@1.0.151
cd ../detect && cargo add sysinfo@0.39.6 serde@1.0.229 serde_json@1.0.151
cd ../..
```

`crates/calendar` deps (`objc2-event-kit`, `oauth2`, `icalendar`, `keyring`, `reqwest`, `sha2`) are added at Phase 5 — no point compiling them for two months.

```bash
# 3.4 src-tauri: plugins + updater seam
cd src-tauri && cargo add \
  tauri@2.11.5 tauri-plugin-clipboard-manager@2.3.3 tauri-plugin-notification@2.4.0 \
  tauri-plugin-global-shortcut@2.3.2 tauri-plugin-dialog@2.7.3 tauri-plugin-opener@2.5.5 \
  tauri-plugin-fs@2.5.2 tauri-plugin-log@2.9.1 tauri-plugin-single-instance@2.4.4 \
  tauri-plugin-updater@2.11.0 \
  tokio@1.53.1 --features tokio/rt-multi-thread,tokio/macros,tokio/fs,tokio/process,tokio/sync,tokio/time
cargo add anyhow@1.0.104 tracing@0.1.44 tracing-subscriber@0.3.23 crash-handler@0.7.0
cargo add --build tauri-build@2.6.3
cd ..
```

Then paste the public key from step 1.2 into `tauri.conf.json`:

```jsonc
"plugins": {
  "updater": {
    "active": false,
    "pubkey": "PASTE_PUBLIC_KEY_FROM_STEP_1.2",
    "endpoints": ["https://example.invalid/meet-ai/{{target}}/{{current_version}}"]
  }
}
```

### Step 4 — frontend deps

```bash
pnpm add react@19.2.8 react-dom@19.2.8 zustand@5.0.15 react-router@8.3.1 \
  @tanstack/react-virtual@3.14.10 react-markdown@10.1.0 remark-gfm@4.0.1 \
  sonner@2.0.8 lucide-react@1.39.0 clsx@2.1.1 tailwind-merge@3.6.0 \
  class-variance-authority@0.7.1 \
  @tauri-apps/api@2.11.1 @tauri-apps/plugin-clipboard-manager@2.3.3 \
  @tauri-apps/plugin-notification@2.4.0 @tauri-apps/plugin-global-shortcut@2.3.2 \
  @tauri-apps/plugin-dialog@2.7.3 @tauri-apps/plugin-opener@2.5.5 \
  @tauri-apps/plugin-fs@2.5.2 @tauri-apps/plugin-log@2.9.1 \
  @tauri-apps/plugin-updater@2.11.0

pnpm add -D typescript@7.0.2 vite@8.2.2 @vitejs/plugin-react@6.1.1 \
  tailwindcss@4.3.3 @tailwindcss/vite@4.3.3 @types/react@19.2.18 \
  @types/react-dom@19.2.5 @biomejs/biome@2.5.11 vitest@4.1.11 \
  @testing-library/react@16.3.3 @testing-library/jest-dom@7.0.1 jsdom@30.0.1 \
  @tauri-apps/cli@2.11.4
```

Tailwind v4 — **no config file**. In `src/index.css`:

```css
@import "tailwindcss";
```

### Step 5 — macOS permissions & signing

`src-tauri/Info.plist`:

```xml
<key>NSMicrophoneUsageDescription</key>
<string>meet-ai records your microphone so your own voice appears in meeting transcripts.</string>
<key>NSAudioCaptureUsageDescription</key>
<string>meet-ai records this Mac's audio so other participants appear in meeting transcripts.</string>
<key>LSMinimumSystemVersion</key>
<string>26.0</string>
```

`src-tauri/entitlements.plist`:

```xml
<key>com.apple.security.device.audio-input</key><true/>
```

Self-signed identity — **not optional, and it is a one-time step.** Under ad-hoc
signing (`codesign -s -`) the designated requirement *is* the binary's cdhash, so
TCC keys the grant to the executable **path** and every rebuild can drop it.
With an identity the requirement becomes bundle ID + certificate, TCC keys the
grant to `pro.saleschat.meetai`, and `tccutil reset` starts working. Both halves
measured in docs/findings.md §10.3–10.4.

```bash
scripts/signing/make-identity.sh          # ~7s, no admin password needed
scripts/signing/make-identity.sh --print  # leaf SHA-1: eafb73d2…
```

The script generates the cert, puts it in its own keychain
(`~/Library/Keychains/meet-ai-signing.keychain-db`, never your login keychain),
and marks it trusted for code signing in the **user** trust domain. That last
part is why there is no password prompt: `security add-trusted-cert` only needs
an admin password with `-d`, which writes to the system keychain. We don't need
that.

**Run it as often as you like — it is idempotent.** If the identity already
exists it reuses it and exits. That matters more than it sounds: the leaf SHA-1
it prints is what TCC keys every grant to, so minting a replacement silently
voids every permission the machine has already given you. `--rotate` is the
deliberate override and warns first.

Use `--print` to read the fingerprint back, and check a built bundle against it:

```bash
codesign -d -r- build/meet-ai.app
# designated => identifier "pro.saleschat.meetai" and certificate leaf = H"eafb73d2…"
```

Verify with `security find-identity -v -p codesigning ~/Library/Keychains/meet-ai-signing.keychain-db`
— pass the keychain explicitly. Without it the command reads the user's keychain
search list, which is empty under a sandboxed `$HOME`, and reports **"0 valid
identities"** for an identity that is present and working.

After that, signing needs no environment at all — `build.sh` defaults to this
identity and finds the keychain itself:

```bash
spikes/phase0a-tcc/build.sh    # or: just bundle-signed
```

⛔ Never fall back to `codesign -s -`. A bundle signed ad-hoc invalidates every
TCC result you get from it, including the Phase 0a gate — and it is not
recoverable: an ad-hoc build makes TCC record the grant against the executable's
absolute **path**, and path-keyed records cannot be removed by `tccutil reset`
(it resolves its argument as a bundle ID and returns `-10814`). Deleting the
build directory does not clear them either. Only System Settings by hand, or a
`TCC.db` write behind Full Disk Access, will. Two such records are already stuck
on the development machine. `build.sh` now exits 1 rather than ad-hoc sign;
`ALLOW_ADHOC=1` overrides it if you genuinely need to. Details in docs/findings.md §10.6.

### Step 6 — `justfile` (incl. the ⛔ Windows cross-check)

```make
set shell := ["bash", "-uc"]
SIGN_IDENTITY := env_var_or_default("SIGN_IDENTITY", "meet-ai-dev")

sidecar:
    swiftc -O sidecar/meet-stt/main.swift -o target/meet-stt
    codesign --force --options runtime -s "{{SIGN_IDENTITY}}" target/meet-stt

dev: sidecar
    pnpm tauri dev
rec:            ; cargo run -p audio --bin meet-rec
stt FILE:       ; ./target/meet-stt {{FILE}}
build:          ; pnpm tauri build

# Windows seam guard — platform-agnostic crates must compile for Windows from day 1.
# Fails the moment a mac assumption leaks out of crates/audio or crates/calendar.
check-windows:
    rustup target add x86_64-pc-windows-msvc
    cargo check --target x86_64-pc-windows-msvc -p store -p stt -p prompts -p detect

check: check-windows
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo test
    pnpm biome check .
    pnpm vitest run

sign:
    codesign --force --deep --options runtime \
      --entitlements src-tauri/entitlements.plist \
      -s "{{SIGN_IDENTITY}}" "src-tauri/target/release/bundle/macos/meet-ai.app"

# The ONLY valid environment for the Phase 0a TCC spike. `just dev` proves nothing:
# TCC keys on the signed bundle identity, and dev builds are unsigned at another path.
bundle-signed: sidecar build sign
    codesign --verify --verbose=2 "src-tauri/target/release/bundle/macos/meet-ai.app"

# One-time fixture generation (crates/audio/fixtures/), needs ffmpeg.
fixtures:
    mkdir -p crates/audio/fixtures
    ffmpeg -f lavfi -i anullsrc=r=16000:cl=mono -t 30 -c:a pcm_s16le \
      crates/audio/fixtures/silence-30s.wav
```

`check-windows` deliberately scopes to the four OS-agnostic crates rather than the whole workspace — `cargo check` on `src-tauri` for a Windows target pulls webview shims that add noise without adding signal. Extend it when `crates/audio/src/windows.rs` lands.

### Step 7 — repo `CLAUDE.md`

```md
# meet-ai

Spec: SPEC.md (locked). Versions: SETUP.md. Evidence: docs/findings.md.

## Hard rules
- Read docs.rs for the EXACT pinned version before writing against: cpal 0.18,
  rubato 5.0, ringbuf 0.5, rusqlite 0.40, keyring 4.2, vite 8, react-router 8,
  tailwind 4, vitest 4, lucide-react 1, typescript 7. Do not write from memory.
- `objc2` 0.6.4 with `objc2-*` 0.3.2 is CORRECT. Never align these versions.
- SpeechAnalyzer/SpeechTranscriber (macOS 26) is new: read Apple docs + swift-scribe first.
- sidecar/meet-stt is Swift, built with swiftc from CLT. Contract is JSON lines on
  stdout. Keep it dumb: WAV in, transcript lines out. No app logic inside it.
- STT engines live behind the SttEngine trait. Never call an engine directly.
- Tailwind v4 has no tailwind.config.js. Config is CSS-first.
- OS-specific code ONLY in crates/audio/src/macos/ and crates/calendar/src/eventkit.rs.
- All paths via `dirs` + `PathBuf::join`. Never `format!("{}/...")`, never a literal `~`.
- Bundle id `pro.saleschat.meetai` is frozen. Changing it revokes users' audio permission.
- No AI calls, no API keys, no telemetry in this codebase. Ever. Only crates/agent
  may hand a transcript to the user's chosen agent CLI (claude / codex), as a
  local child process, when notes are on for that meeting (SPEC A11).
- markdown is the source of truth; index.db is derived and must be safe to delete.
- transcript.md: ONE utterance = ONE line. Collapse \n\r\t and whitespace runs to a
  single space. Never write empty text. Append-only, never rewrite a line.
- Only FINALIZED text is persisted. Volatile/partial results go to the UI event
  channel only and never touch disk. All engines write via one TranscriptSink.
- The watcher must suppress self-writes (path -> Instant, 750ms) or it will reload
  notes.md under the user's cursor. Agent writes are NOT suppressed (on the
  background path the app writes meeting.md and tickets itself, so those are
  self-writes).

## Commands
just dev | just rec | just check | just sign
```

### Step 8 — verify the setup

```bash
just check                    # must pass green, including check-windows
just dev                      # window opens
cargo run -p audio --bin meet-rec --  --list-devices

# Phase 0a gate — signed bundle only. Never test TCC under `just dev`.
just fixtures
just bundle-signed
tccutil reset AudioCapture pro.saleschat.meetai
open src-tauri/target/release/bundle/macos/meet-ai.app
```

Pass = the prompt appears, names **meet-ai** (not the helper), and non-silent samples arrive.
⚠️ Prompt appears but samples are silent = **fail**, not pass.

⚠️ If System Settings shows more than one row named **meet-ai** (Microphone or
"System Audio Recording Only"), do not guess which one to toggle — the extras
are almost always permanent path-keyed rows left over from pre-identity ad-hoc
builds (or, if you built `spikes/phase0a-tcc` instead of the real app, a second
real bundle ID, `pro.saleschat.meetai.tap-probe`). `tccutil reset` above only
ever reaches `pro.saleschat.meetai`. Full recipe and root cause: docs/findings.md §11.

Then commit and start Phase 0.

---

## 6. What Step 8 does not prove

The setup is green when the workspace compiles and the window opens. That says nothing about the 🔴 risk. Phase 0's real gate is unchanged from SPEC §5:

> 45-minute real Zoom call. `mic.wav` + `system.wav` both intact. Drift < 200ms end-to-end. Survives an AirPods switch mid-call. Survives `kill -9`.

First thing to write in `crates/audio` is the permission check plus `--list-devices`, so you find out on day one whether `NSAudioCaptureUsageDescription` + the self-signed identity actually produce a working permission prompt. That single question is the project's biggest unknown — front-load it.
