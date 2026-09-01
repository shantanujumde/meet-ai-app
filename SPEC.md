# meet-ai — Production Spec v2 (lockable)

**Date:** 2026-09-01 · **Supersedes:** `Readme.md` (Discovery v1) · **Backed by:** `FINDINGS.md`

---

## Context

`Readme.md` locked 12 decisions before any code existed. Research against five comparable shipped projects (Muesli 1.1k★, pasrom/meeting-transcriber 151★, Meetily, Hyprnote/anarlog, open-granola) plus Granola's observed behaviour showed six of those decisions were contradicted by evidence, and v1 scope was roughly 3x what one person can vibe-code to a daily-usable state.

This spec is the reconciled version. Every change is traceable to evidence in `FINDINGS.md` §2 and to a user decision recorded in §1 below.

**The product, unchanged:** botless meeting recorder for developers. Meetings become merged PRs. Audio never leaves the machine.

**The architectural shift:** the app has **no AI layer**. It records, transcribes, stores markdown, and renders it. The user's own agent (Claude Code / Codex / Cursor) does all reasoning by reading and writing those files, driven by a self-contained prompt the app puts on the clipboard. This deletes prompt plumbing, output validation, retry logic, per-agent adapters, and API keys from the codebase entirely — and makes the app agent-agnostic forever.

---

## 1. Locked decisions

| # | Decision | v1 choice | Changed from Readme.md? |
|---|---|---|---|
| L1 | Platform | **macOS for v1; Windows next.** Seams built now (§8.2), port is additive | ✅ was mac+Windows simultaneously |
| L2 | OS floor | **macOS 14.4+**, Core Audio process tap only, no ScreenCaptureKit path | ✅ was macOS 13+ w/ fallback |
| L3 | Capture location | **In-process Rust** via `objc2-core-audio` | ✅ was Swift sidecar (TCC inheritance broken) |
| L4 | Transcription | Local `whisper-rs`, **utterance-level** via VAD | ✅ was true streaming |
| L5 | Speaker labels | Two channels: mic=`You`, tap=`Others` | — unchanged |
| L6 | Echo (no headphones) | **Detect + warn.** No dedupe code | ✅ explicit now |
| L7 | Storage | **Markdown = truth** + derived rebuildable SQLite FTS5 index | ✅ was markdown-only |
| L8 | Semantic search | **Deferred.** Schema leaves room for `sqlite-vec` | ✅ explicit now |
| L9 | AI layer | **None in-app.** Clipboard prompt → user's agent → agent writes files | ✅ was Claude Code CLI invocation |
| L10 | Agent instructions | **Self-contained in the copied prompt.** App writes nothing outside `~/Meetings/` | ✅ was bundled skill install |
| L11 | Trackers | Agent pushes via its **own** Jira/Linear/GitHub connections. App stores zero tokens | ✅ was 3 native sync engines |
| L12 | MCP server | **v1.1**, not v1 | ✅ was v1 |
| L13 | Calendar | **EventKit + Google OAuth + Microsoft Graph OAuth + ICS URL.** EventKit is the zero-auth default; OAuth covers users with no local mail client configured | ⚠️ partly — OAuth restored on top of EventKit |
| L14 | Start Work | **Copy prompt to clipboard** | — unchanged intent |
| L15 | Recording trigger | Auto-detect → notify → user confirms | — unchanged |
| L16 | Audio retention | **7 days**, then auto-delete. Configurable | ✅ explicit now |
| L17 | Distribution | **Personal for v1; public in v2.** Irreversible choices (bundle ID, updater key, entitlements) made correctly now — see §8.1 | ✅ explicit now |
| L18 | In-meeting UI | Notes pane + collapsible live transcript. Other panels post-meeting | ✅ was 5-panel workspace |

---

## 2. Tech stack — full granularity

### 2.1 Shell & frontend

| Layer | Choice | Version | Why / note |
|---|---|---|---|
| Desktop framework | Tauri | 2.x | ~45MB idle; runs beside Zoom. Native Swift was the alternative but forecloses Windows |
| Webview | WKWebView (system) | — | Comes with Tauri on macOS |
| UI library | React | 19 | Largest LLM training corpus → best vibe-coding target |
| Language | TypeScript | 5.6+ | `strict: true` |
| Bundler | Vite | 6 | Tauri default |
| Styling | Tailwind CSS | 4 | |
| Components | shadcn/ui | latest | Copy-in, no runtime dep, LLM knows it well |
| Icons | lucide-react | latest | |
| Client state | Zustand | 5 | Small; no server so no query layer needed |
| Routing | React Router | 7 (declarative) | 4 routes only |
| Long-list perf | `@tanstack/react-virtual` | 3 | Transcripts hit 1000+ lines |
| Markdown render | `react-markdown` + `remark-gfm` | latest | Renders `meeting.md`, tickets |
| Notes editor | plain `<textarea>` + preview toggle | — | Deliberately not CodeMirror. Notes are typed mid-meeting; nothing fancy is wanted |
| Toasts | `sonner` | latest | Permission + warning surfaces |
| TS lint/format | Biome | 2 | One tool replaces eslint+prettier |
| Frontend tests | Vitest + Testing Library | latest | Component + store tests only |

### 2.2 Tauri plugins

| Plugin | Used for |
|---|---|
| `tauri-plugin-clipboard-manager` | The entire L9/L14 flow |
| `tauri-plugin-notification` | "Meeting detected", "recording stopped" |
| `tauri-plugin-global-shortcut` | ⌘⇧R start/stop |
| `tauri-plugin-dialog` | Pick meetings root, pick repo path |
| `tauri-plugin-opener` | Reveal folder in Finder, open ticket in editor |
| `tauri-plugin-fs` | Scoped reads for the UI |
| `tauri-plugin-log` | Frontend logs into the same file as Rust |
| `tauri-plugin-single-instance` | Two recorders would fight over the tap |

### 2.3 Rust core — crate by crate

> **Exact pinned versions live in [`SETUP.md`](./SETUP.md) §2–3**, verified against crates.io and npm on 2026-09-01. This table names *what and why*; SETUP.md names *which version*. When they disagree, SETUP.md wins.

| Concern | Crate | Notes / risk |
|---|---|---|
| Async runtime | `tokio` (rt-multi-thread, fs, process, sync) | Audio callbacks stay on their own OS threads, never in tokio |
| **System audio tap** | `objc2`, `objc2-core-audio`, `objc2-core-audio-types`, `objc2-foundation` | 🔴 **Hardest module.** Port structure from `insidegui/AudioCap` + sudara's tap gist |
| Mic capture | `cpal` 0.15 | Default input device, 48kHz f32 |
| Device-change events | `objc2-core-audio` property listener on `kAudioHardwarePropertyDefaultOutputDevice` / `…InputDevice` | AirPods hot-swap → close segment, reopen |
| Resampling | `rubato` 0.16 | 48k → 16k mono for whisper |
| Ring buffer | `ringbuf` 0.4 | Lock-free audio thread → worker |
| WAV I/O | `hound` 3.5 | Incremental write, header flushed every 5s (crash safety) |
| VAD | `earshot` 1.2.2 | Pure Rust WebRTC VAD, only dep is `libm`. **Replaces Silero+`ort`** — see SETUP.md §1.1. Behind a `Vad` trait so Silero can swap in |
| **STT** | `whisper-rs` 0.16, features `metal` + `tracing_backend` | 🟡 Build config per-arch is the documented gotcha. `coreml` feature is a free post-Phase-1 speedup; `vulkan`/`cuda` are the Windows seam |
| Model download | `reqwest` (rustls) + `sha2` + `tokio-util` | HTTP Range resume, checksum verify, atomic rename |
| Database | `rusqlite` 0.32, features `bundled`, `fts5` | Derived index only. WAL mode |
| Migrations | hand-written `PRAGMA user_version` steps | 3 tables; a framework is overkill |
| File watching | `notify` 7 + `notify-debouncer-full` | 500ms debounce; agent writes land as bursts |
| Frontmatter | `yaml-rust2` 0.12 | `serde_yaml` is deprecated; `gray_matter` is read-only. Parse to an ordered `Yaml` value, mutate owned keys, emit — unknown keys survive because they are never modelled. SETUP.md §1.2 |
| Config | `jsonc-parser` 0.33 + `serde_json` | `json_comments` last shipped 2023. SETUP.md §1.3 |
| Prompt templates | `minijinja` 2 | Templates live in `.app/prompts/*.md`, user-editable |
| Calendar — local | `objc2-event-kit` | 🟡 EventKit via objc2; read-only. Zero-auth default |
| Calendar — OAuth | `oauth2` 5.x + `tauri-plugin-oauth` (loopback listener) | 🟡 **PKCE, no client secret in the binary.** Google + Microsoft both |
| Calendar — ICS | `reqwest` + `icalendar` 0.16 | 🟢 Paste a private ICS URL; no auth at all. Universal fallback |
| Token storage | `keyring` 3.x (macOS Keychain) | Refresh tokens only. Never in `config.jsonc` |
| Meeting-app detection | `sysinfo` 0.32 | Process names: `zoom.us`, `Microsoft Teams`, `Webex`, `Slack`, `Discord` |
| Browser-meeting detection | **skipped in v1** | Google Meet in a tab is not detectable without a browser extension. Calendar + audio-activity covers it |
| Logging | `tracing`, `tracing-subscriber`, `tracing-appender` | Rolling file at `.app/logs/` |
| Errors | `thiserror` (libs), `anyhow` (top) | |
| Snapshot tests | `insta` | Transcript formatting, frontmatter round-trip |
| Task runner | `just` | `just dev`, `just rec`, `just sign` |

### 2.4 Models

| Model | File | Size | Source | Role |
|---|---|---|---|---|
| Whisper large-v3-turbo, q5_0 | `ggml-large-v3-turbo-q5_0.bin` | ~1.6GB | HF `ggerganov/whisper.cpp` | Default STT |
| Whisper small.en q5_1 | `ggml-small.en-q5_1.bin` | ~180MB | same | Fast tier / low battery |
| _(no VAD model)_ | — | — | — | `earshot` is pure Rust — nothing to download or bundle |

Downloaded on first run to `~/Meetings/.app/models/`. Resumable, checksummed, atomic rename. No Python anywhere.

### 2.4b Calendar providers (L13)

All four sit behind one trait — `CalendarProvider { list_events(range) -> Vec<Event> }` — so the UI and detection logic never branch on provider.

| Tier | Provider | Auth | Setup cost | Operational reality |
|---|---|---|---|---|
| 1 (default) | **EventKit** | One macOS permission prompt | 🟢 ~2 days | Reads *every* account already in Calendar.app — iCloud, Google, Exchange, Outlook, CalDAV. Zero OAuth. Fails only if the user never configured Calendar.app |
| 2 | **Microsoft Graph** (`Calendars.Read`, delegated) | OAuth + PKCE, loopback redirect | 🟡 ~2 days | No app review needed. Personal *and* work accounts. Refresh tokens persist |
| 3 | **Google Calendar** (`calendar.readonly`) | OAuth + PKCE, loopback redirect | 🟡 ~2 days + config chores | **Sensitive scope.** Two hard rules: (a) set the OAuth app to *In production* — leaving it in *Testing* makes refresh tokens die every **7 days**; (b) unverified apps show a one-time "Google hasn't verified this app" screen → Advanced → Go to app. 100-user cap while unverified, irrelevant at L17 |
| 4 | **ICS URL** | None | 🟢 ~0.5 day | Paste a private `.ics` link. Read-only, refresh on a timer. Covers Zoho, Fastmail, self-hosted, anything |

**Security notes:** desktop OAuth uses PKCE with no client secret (Google and Microsoft both treat desktop client secrets as non-secret; PKCE is the required flow). Redirect is `http://127.0.0.1:<random>/callback` via `tauri-plugin-oauth`, never a custom URL scheme. Refresh tokens go to the macOS Keychain via `keyring`; access tokens stay in memory only. Scopes are read-only — the app never writes to a calendar.

### 2.5 AI / agent layer

| Piece | Implementation |
|---|---|
| In-app inference | **None** |
| API keys stored | **None** |
| Prompt assembly | `minijinja` over `.app/prompts/{wrap,start_work,push_ticket}.md` |
| Delivery | Clipboard (`tauri-plugin-clipboard-manager`) |
| Instructions | **Inline in the prompt** — full output contract every time (L10) |
| Agent writes results | Direct filesystem writes into the meeting folder |
| App notices results | `notify` watcher → index update → UI re-render |
| Tracker push | Prompt names the user's configured tracker; the agent uses its own Linear/Jira/GitHub connection |
| Ticket ID allocation | Prompt instructs: scan `~/Meetings/*/tickets/TICK-*.md`, take max+1, zero-pad to 4 |

### 2.6 Build, sign, verify

| Concern | Choice |
|---|---|
| Package manager | pnpm 9 |
| Rust toolchain | stable, pinned via `rust-toolchain.toml` |
| Target | `aarch64-apple-darwin` only (v1) |
| Info.plist keys | `NSMicrophoneUsageDescription`, **`NSAudioCaptureUsageDescription`** (the tap permission key), `LSMinimumSystemVersion = 14.4` |
| Entitlements | `com.apple.security.device.audio-input` |
| Signing | **Local self-signed identity + hardened runtime.** Required for TCC to register the app at all — this is not optional even for personal use |
| Notarization / auto-update / CI | **Out of scope** (L17) |
| Rust checks | `cargo clippy -D warnings`, `cargo fmt`, `cargo test` |
| Audio test harness | Fixture WAVs in `crates/audio/fixtures/` — every 🔴 module testable with **no live meeting** |

---

## 3. Data contracts

### 3.1 Folder layout

```
~/Meetings/
  2026-09-01-1430-standup/
    meeting.md            # frontmatter + Summary/Decisions/Actions/Questions  (agent writes)
    transcript.md         # app writes, append-only during meeting
    notes.md              # user types, app writes
    audio/
      mic.wav             # deleted after retention_days
      system.wav
      segments.json       # clock alignment record (see 3.4)
    tickets/
      TICK-0001.md
  .app/
    config.jsonc
    index.db              # DERIVED — safe to delete, rebuilt on launch
    models/
    prompts/{wrap,start_work,push_ticket}.md
    logs/meet-ai.log
```

**Invariant:** nothing exists only in `index.db`. Delete it → full rescan restores every feature.

### 3.2 `meeting.md`

```yaml
---
id: 2026-09-01-1430-standup
title: Platform Standup
date: 2026-09-01T14:30:00+05:30
duration_sec: 2714
attendees: [Shantanu, Priya, Dev]        # from EventKit when available
calendar_event_id: "ABC123"              # optional
repo: ~/apps/api                          # optional link
analyzed_by: claude-code                  # agent stamps this
analyzed_at: 2026-09-01T15:32:00+05:30
---

## Summary
## Decisions
## Action Items
## Open Questions
```

Sections are fixed headings — the parser locates by heading, so the agent must not rename them. Stated in the prompt.

### 3.3 `TICK-NNNN.md`

```yaml
---
id: TICK-0001
title: Move sessions to Redis
meeting: 2026-09-01-1430-standup
status: open                  # open | in_progress | done | dropped
assignee: Shantanu
estimate: 2d                  # U4 data collection starts in v1
estimated_on: 2026-09-01
transcript_ref: "00:14:22"    # powers Start Work context
synced_to: null               # linear | jira | github
external_id: null
external_url: null
---

Body / acceptance notes.
```

### 3.4 `transcript.md` — strict, parseable

```
[00:00:04] Others: Morning everyone, let's start with the API work.
[00:00:11] You: Sessions are still in memory, that's the blocker.
```

Regex: `^\[(\d{2}:\d{2}:\d{2})\] (You|Others): (.*)$`. Deliberately plain — readable in Obsidian *and* trivially parseable.

`segments.json` records clock truth:

```json
{"segments":[{"idx":0,"start_host_ns":123456789,"mic_rate":48000,"sys_rate":48000,
              "mic_frames":130713600,"sys_frames":130713600,"reason":"start"},
             {"idx":1,"start_host_ns":...,"reason":"default_output_device_changed"}]}
```

Timestamps derive from `segment.start_host_ns + frame_index / rate` — never from wall-clock at write time. This is the mitigation for the 🔴 clock-drift risk.

### 3.5 `config.jsonc`

```jsonc
{
  "$schema": "./config.schema.json",
  "meetings_root": "~/Meetings",
  "transcription": {
    "model": "large-v3-turbo-q5_0",   // or "small.en-q5_1"
    "language": "en",
    "live": true                       // false = transcribe on stop only
  },
  "audio": {
    "retention_days": 7,               // 0 = delete immediately, -1 = keep forever
    "warn_no_headphones": true
  },
  "calendar": {
    "providers": ["eventkit"],        // + "google", "microsoft", "ics"
    "ics_urls": [],
    "refresh_minutes": 15
  },
  "detection": {
    "calendar": true,
    "processes": true,
    "audio_activity": true,
    "min_attendees": 2
  },
  "tickets": { "tracker": "linear" },  // named in the copied prompt; app never calls it
  "repos": { "default": "~/apps/api" }
}
```

### 3.6 `index.db` (derived)

```sql
meetings(id PK, title, date, duration_sec, path, has_analysis, mtime)
tickets(id PK, meeting_id, title, status, assignee, estimate, synced_to, path, mtime)
transcript_fts USING fts5(meeting_id UNINDEXED, ts UNINDEXED, speaker UNINDEXED, text)
-- v1.1: transcript_vec USING vec0(embedding float[768])   <- the seam for L8
```

---

## 4. Module layout

```
meet-ai/
  crates/
    audio/        # 🔴 tap + mic + resample + wav + device events. Has a CLI bin.
    stt/          # 🟡 VAD segmentation + whisper-rs + transcript formatting
    store/        # 🟢 markdown read/write, frontmatter, index, watcher
    prompts/      # 🟢 minijinja templates + assembly
    calendar/     # 🟡 CalendarProvider trait: eventkit | google | microsoft | ics
    detect/        # 🟢 sysinfo processes + audio-activity heuristic
  src-tauri/      # 🟡 commands, events, tray, plugins, state machine
  src/            # 🟢 React app
```

Every 🔴/🟡 crate is independently runnable and fixture-testable. `crates/audio` ships `bin/meet-rec` — Phase 0 needs no Tauri and no UI.

`AudioSource` trait is defined in `crates/audio` from day one even though only macOS implements it — that is what makes the Windows port additive.

---

## 5. Phased build, with exit gates

Miss a gate → stop, don't stack work on a broken layer.

| Phase | Build | Exit gate |
|---|---|---|
| **0. Capture CLI** ~2wk 🔴 | `meet-rec` writes `mic.wav` + `system.wav` + `segments.json`. Permission prompt, device-change handling, incremental writes | **45-min real Zoom call: both files intact, drift < 200ms end-to-end, survives an AirPods switch mid-call, survives `kill -9`** |
| **1. Transcribe** ~1wk 🟡 | Model download, VAD segmentation, whisper-rs, `transcript.md` | Phase-0 call reads accurately. Speakers correctly split. Silence produces no invented text |
| **2. App shell** ~2wk 🟢 | Tauri + React: meeting list, transcript view, notes pane, live utterances, tray, ⌘⇧R | You choose it over Notes for a real meeting |
| **3. Store + index** ~1wk 🟢 | Markdown read/write, watcher, SQLite FTS5, search box, ticket UI, manual ticket create | Delete `index.db` → everything still works after rescan |
| **4. Agent loop** ~1wk 🟢 | `[Wrap up]` + `[Start Work]` + `[Push ticket]` prompt buttons; prompt templates | 5 consecutive meetings → usable tickets appear in UI with zero hand-repair |
| **5a. Detection + local calendar** ~1.5wk 🟡 | `CalendarProvider` trait + **EventKit**, auto-title, 1-min reminder, process detect, confirm-to-start, **U5 pre-meeting brief with `git log`** | You open the app before meetings without being prompted |
| **5b. Cloud calendars** ~1wk 🟡 | PKCE loopback flow, Keychain tokens, **Microsoft Graph first** (no review), then **Google** (production-unverified), then ICS URL | Fresh Mac with Calendar.app untouched still shows today's meetings |
| **6. Polish** ~1wk 🟢 | Retention job, headphone warning, config + JSON schema, 3 hooks (`on_transcript_ready`, `on_analysis_complete`, `on_meeting_end`), logs | Two weeks of daily use, no manual file surgery |

**≈10.5 weeks.** Deferred by design: MCP server (L12), semantic search (L8), Windows, N-speaker diarization (`sherpa-rs`), U2 drift detection, U4 calibration UI, U6 ownership map, notarization.

**Kill criteria:**
- Phase 0 drift unsolved after 2 weeks → rewrite as native Swift, macOS-only, drop Windows from the roadmap permanently.
- Phase 4 tickets need hand-repair every time → this is a transcriber, not a PR pipeline. Reposition before building more.
- You stop using it for 2 weeks → the missing piece is the daily habit (Phase 5), not more features.

---

## 6. Verification

**Phase 0 (the one that matters):**
```
just rec                        # starts meet-rec, prints permission state
# join a real 45-min Zoom call, swap to AirPods mid-way, then kill -9 the process
ffprobe audio/mic.wav audio/system.wav      # both present, non-truncated
cargo run -p audio --bin drift-check -- audio/    # asserts < 200ms, reads segments.json
```

**Phase 1:** `cargo test -p stt` — fixture WAVs incl. a 30s pure-silence file that must yield **zero** transcript lines (the whisper-hallucination guard).

**Phase 3:** `rm ~/Meetings/.app/index.db && just dev` → all meetings, tickets and search return identically. This test *is* the L7 invariant.

**Phase 4:** click `[Wrap up]`, paste into Claude Code, confirm `meeting.md` + `tickets/` appear in the UI within ~2s of the agent writing them (watcher latency).

**Phase 5a:** `cargo test -p calendar` against fixture events; manual check that a Google account already added to Calendar.app shows up with **no** OAuth.

**Phase 5b:** with Calendar.app deliberately empty, connect Microsoft then Google via the OAuth buttons — today's events appear. Then confirm token persistence: quit the app, wait a day, relaunch, events still load without re-login (this is the check that catches a "Testing"-status OAuth app).

**Continuous:** `just check` = `cargo clippy -D warnings && cargo test && pnpm biome check && pnpm vitest run`.

---

## 7. Open risks

| Risk | Standing mitigation |
|---|---|
| 🔴 Core Audio tap setup is officially poorly documented | Port from `insidegui/AudioCap`; isolated crate; fixture harness; Phase-0 gate before anything is built on top |
| 🔴 Two-stream clock drift, silent failure at ~30min | `segments.json` + `drift-check` binary + explicit 200ms gate |
| 🟡 whisper-rs Metal build config | Pin crate + toolchain; document the working `build.rs` env once it works |
| 🟡 TCC won't register an unsigned app | Self-signed identity + hardened runtime from Phase 0, not retrofitted |
| 🟡 First-run 1.6GB download | Resumable + checksummed; small.en offered as the fast path |
| 🟡 Google refresh tokens dying weekly | OAuth app must be set to *In production* (unverified is fine at L17), **not** *Testing*. Phase-5b gate explicitly tests this |
| 🟡 "Google hasn't verified this app" screen | Accepted at L17 — one-time click-through, documented in setup notes. Only becomes real work if L17 changes to public release |
| 🟢 Agent writes malformed markdown | Parser tolerates missing sections and preserves unknown frontmatter keys; UI shows a "needs attention" badge rather than failing |
| 🟢 Doubled transcript without headphones | Detect + warn (L6). Explicitly not fixed in code |

---

## 8. Extension seams

Both v2 targets — public release and Windows — are additive **only if** the items below are done in v1. Total cost ≈3 days. Items marked ⛔ are irreversible after the first install goes out.

### 8.1 Public release (v2)

| Seam | Do in v1 | Why it can't wait |
|---|---|---|
| ⛔ **Bundle identifier** | Fix it now: `pro.saleschat.meetai`. Never change it | macOS TCC keys permissions to the bundle ID. Renaming later silently revokes mic + audio-capture consent for every existing install, with no way to migrate it |
| ⛔ **Updater keypair** | `tauri signer generate` now; put the **public** key in `tauri.conf.json`, include `tauri-plugin-updater`, point `endpoints` at a placeholder URL, ship with `active: false` | The pubkey is compiled in. A build without it can never verify a later update — v1 users would have to find and reinstall manually. Private key goes to a password manager, never the repo |
| **Signing indirection** | `just sign` reads `$SIGN_IDENTITY` (defaults to the self-signed cert) | v2 becomes `SIGN_IDENTITY="Developer ID Application: …" just sign` — one env var, no code change |
| **Entitlements + Info.plist** | Already final (§2.6): hardened runtime, `com.apple.security.device.audio-input`, `NSAudioCaptureUsageDescription` | Same files feed the notarized build. Notarization then adds only a CI step, not a rewrite |
| **OAuth client IDs in config, not code** | `.app/config.jsonc` → `calendar.google.client_id`, `calendar.microsoft.client_id` | v2 swaps in a *verified* Google production client without a rebuild |
| **Onboarding as its own route** | `/onboarding` exists in v1 with 3 steps (permission → model download → meetings folder), even if plain | Public v1 users hit permissions cold. Growing an existing route is cheap; retrofitting a flow into a running app is not |
| **License + repo hygiene** | `LICENSE` = Apache-2.0 (matches open-granola), `SECURITY.md`, `CHANGELOG.md` from commit one | Adding a license after external contributions arrive is a legal mess |
| **No telemetry, ever** | Keep it absent, and say so in the README | It's the product's core claim. Adding it later would break the promise |
| Deliberately **not** done in v1 | Notarization CI, hosted privacy policy, Google scope verification, crash reporting, i18n | All are pure v2 add-ons that touch no v1 architecture |

### 8.2 Windows (v1.1)

| Seam | Do in v1 | Payoff |
|---|---|---|
| **`AudioSource` trait** | Defined in `crates/audio` from Phase 0, macOS the only impl | Windows = one new `windows.rs` implementing the trait with the `wasapi` crate's loopback (per-process supported, no permission prompt) |
| ⛔ **Confine `#[cfg]`** | OS-specific code allowed **only** in `crates/audio/src/macos/` and `crates/calendar/src/eventkit.rs`. Nowhere else | Prevents the slow leak of mac assumptions into `store`, `stt`, and `src-tauri` — the thing that turns a port into a rewrite |
| **`stub` AudioSource + cross-check** | A no-op impl behind a cargo feature, and `just check` runs `cargo check --target x86_64-pc-windows-msvc --features stub-audio` from day one | The whole workspace type-checks for Windows on every commit. A leak fails CI the day it's introduced, not in month four |
| **Paths via `dirs` + `PathBuf`** | `dirs::home_dir()`, `PathBuf::join`. Zero `format!("{}/…")`, zero literal `~` | Windows paths just work |
| **whisper-rs accel behind features** | `metal` on macOS; leave `vulkan` / `cuda` feature stubs declared and unused | Windows GPU tiering is a feature flag, not a refactor |
| **Process names in data, not code** | Detection list lives in `crates/detect/processes.json` | Windows names (`Zoom.exe`, `ms-teams.exe`) are a data edit |
| **Keychain via `keyring`** | Already chosen (§2.3) | Maps to Windows Credential Manager with no code change |
| **UI font stack** | Full fallback chain, not bare `-apple-system` | No mystery-font bug on Windows |
| **Free payoff already banked** | Calendar tiers 2–4 (Graph / Google / ICS) from Phase 5b are cross-platform | Windows calendar support arrives ~done; only EventKit is mac-only |

**Revised total: ≈11 weeks** (10.5 + ~3 days of seam work spread across phases).

---

## 9. Next steps

**Status: LOCKED.** Changes to §1 decisions require a dated amendment at the bottom of this file, not an edit in place.

1. ~~Write this spec to `SPEC.md`; mark `Readme.md` as superseded discovery.~~ ✅ done
2. **Do the ⛔ irreversible items before any feature code** — bundle ID `pro.saleschat.meetai`, `tauri signer generate` keypair, `LICENSE` (Apache-2.0), entitlements + Info.plist keys. ~30 minutes now, unfixable later.
3. Scaffold the cargo workspace + Tauri app, with the `stub-audio` Windows cross-check wired into `just check` from the first commit.
4. Start **Phase 0** in `crates/audio` — `meet-rec` CLI, no UI. Gate: 45-min real Zoom call, drift < 200ms, survives AirPods swap and `kill -9`.

---

## Amendments

### A1 — 2026-09-01 · Dependency research (see [`SETUP.md`](./SETUP.md))

Live registry check produced four substantive changes, all simplifications. No §1 decision is affected.

1. **VAD: Silero+`ort` → `earshot` 1.2.2.** `voice_activity_detector` 0.2.1 pins `ort =2.0.0-rc.10` — an exact pin on a pre-release ONNX runtime. `earshot` is pure Rust with `libm` as its only dependency. Removes `ort`, `ort-sys`, `ndarray` and the 2MB `silero_vad.onnx` from v1. Silero stays available behind a `Vad` trait if Phase 1's silence test shows hallucination on noise.
2. **Frontmatter: `serde_yaml` + `gray_matter` → `yaml-rust2` 0.12.** `serde_yaml`'s latest release is literally `0.9.34+deprecated` (Mar 2024). `yaml-rust2` also satisfies the preserve-unknown-keys requirement properly, since nothing is deserialized into a struct.
3. **Config: `json_comments` → `jsonc-parser` 0.33.1.** The former last shipped in 2023.
4. **Three deliberate non-latest pins** where a new major buys nothing this project needs and costs LLM familiarity: `sha2` 0.10.9 (not 0.11), `reqwest` 0.12.28 (not 0.13), `notify` 8.2.0 (9.0 is an rc).

Also recorded: this machine has **no Rust toolchain installed** and no `just`; full Xcode is **not** required (Command Line Tools suffice, since L2 drops ScreenCaptureKit). Toolchain pinned to Rust **1.98.0**.
