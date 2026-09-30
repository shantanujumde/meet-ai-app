# meet-ai — Production Spec v2 (lockable)

**Date:** 2026-09-01 · **Supersedes:** `Readme.md` (Discovery v1) · **Backed by:** `FINDINGS.md` · **Why it exists:** [`PROBLEM.md`](./PROBLEM.md)

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
| L2 | OS floor | **macOS 26+** (Core Audio process tap only, no ScreenCaptureKit path). The Apple STT engine ships with the OS, so mac has no sub-26 tier to fall back from | ⚠️ amended — was 13+, then 14.4+, now **26+**, see **A8** |
| L3 | Capture location | **RESOLVED: in-process Rust** (`crates/audio`, `objc2-core-audio`). The Phase 0a spike passed, but for a reason that removed §5's argument for the sidecar — see A6 | ⚠️ amended — see A2, **A6** |
| L4 | Transcription | **Apple `SpeechTranscriber` via Swift sidecar** on macOS 26+, with **native long-form streaming** (volatile → finalized). `whisper-rs` + `earshot` VAD chunking is the fallback engine | ⚠️ amended — better *and* less code, see A2 |
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
| File watching | `notify` 8 + `notify-debouncer-full` | 500ms debounce; agent writes land as bursts. **Must implement self-write suppression** — see §4 |
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
| **Apple `SpeechTranscriber`** | — | **0 MB** | ships with macOS 26 | **Default on mac.** Nothing to download |
| Whisper large-v3-turbo, q5_0 | `ggml-large-v3-turbo-q5_0.bin` | ~1.6GB | HF `ggerganov/whisper.cpp` | **Windows** engine, and the manual opt-out on mac. No longer a mac version fallback (A8) |
| Whisper small.en q5_1 | `ggml-small.en-q5_1.bin` | ~180MB | same | Fast tier / low-end Windows |
| _(no VAD model)_ | — | — | — | `earshot` is pure Rust; on mac `SpeechDetector` replaces it |

**On mac the first-run download is zero bytes** — every supported mac is macOS 26+ (L2). Whisper models are fetched lazily, only when the user deliberately picks the fallback engine. Downloads go to `~/Meetings/.app/models/`: resumable, checksummed, atomic rename. No Python anywhere.

### 2.5 STT engines (L4)

One trait, four implementations. The app never branches on platform outside the engine registry.

```rust
trait SttEngine {
    fn kind(&self) -> EngineKind;
    fn transcribe(&self, wav: &Path, sink: &mut dyn TranscriptSink) -> Result<()>;
    fn supports_streaming(&self) -> bool;
}
```

| # | Engine | How it's called | Available | Model download | Notes |
|---|---|---|---|---|---|
| 1 | **Apple `SpeechTranscriber`** | Swift sidecar `meet-stt` | macOS 26+ | **0 MB — ships with the OS** | Default on mac. ~2× faster than whisper large-v3-turbo, tops on-device accuracy. **Native long-form streaming**, so no VAD chunking needed |
| 2 | **`whisper-rs`** | in-process Rust | everywhere | 1.6GB (or 180MB small.en) | Universal floor. Needs `earshot` VAD + utterance chunking. On mac this is now only a **manual override**, never a version fallback (A8) |
| 3 | **Windows AI Speech Recognition** | `windows` crate (WinRT), in-process — **no sidecar needed** | Windows 11 | preinstalled on Copilot+ NPU; on-demand on CPU-only | Windows default at port time. ⚠️ APIs still **preview** — verify then; engine 2 is the safety net |
| 4 | **Cloud (BYOK)** | HTTPS | everywhere | none | Opt-in only, never load-bearing. Needs Opus encode + chunking (OpenAI caps at 25MB/file vs ~350MB for a 1-hour WAV) |

**Defaults:** mac → 1, always (macOS 26 is the floor, L2). Windows 11 → 3, falling back to 2. Cloud is never a default.

**What reaches disk.** The engines have different output shapes — Apple emits volatile → finalized results, whisper emits completed chunks. One rule reconciles them:

- **Only finalized text is persisted.** Volatile/partial results are pushed to the frontend over a Tauri event channel and live in UI memory only. They never touch the filesystem.
- Every engine normalizes through **one `TranscriptSink`**, which owns the §3.4 line contract, the whitespace collapse, and the append. Engines emit structured utterances; they never format and never write.
- Therefore, by design: **the live transcript pane and `transcript.md` are not identical mid-meeting.** The pane shows a volatile tail; the file holds only settled text. This is intentional, not a bug to fix.

### 2.6 The Swift sidecar (`sidecar/meet-stt`)

A single Swift CLI binary inside the app bundle at `Contents/MacOS/meet-stt`. Reads WAV paths, emits JSON lines on stdout. Built with `swiftc` — **Command Line Tools are sufficient, full Xcode is not required.**

| Responsibility | Framework | Phase |
|---|---|---|
| Transcription | `Speech` (`SpeechAnalyzer` / `SpeechTranscriber`) | 1 |
| VAD when needed | `SpeechDetector` | 1 |
| Speaker diarization (N speakers) | FluidAudio (pyannote on the Neural Engine) | post-v1 — un-defers L5/U6 |
| System audio capture | Core Audio process tap | **only if Phase 0a passes** |

**Why a sidecar is safe here, unlike the one L3 originally rejected:** transcription reads a file off disk. No microphone, no audio-capture permission, nothing for TCC to attribute. The TCC risk applies *only* to the capture responsibility, which is exactly what Phase 0a tests.

**Testing it.** A bare `swiftc` build has no XCTest target, and adding SwiftPM just to get one is disproportionate. Test the sidecar **from Rust** instead: `crates/stt/tests/sidecar.rs` spawns `target/meet-stt` against the §6 fixture WAVs and asserts the JSON-lines output. That tests the real contract — process boundary and JSON shape — rather than Swift internals, and keeps one test suite behind one `just check`.

**Not available via sidecar:** App Intents / Shortcuts must be a real Xcode target inside the bundle, which Tauri cannot build. Accepted loss. Spotlight indexing needs no code at all — the markdown corpus on disk is already indexed.

### 2.7 Calendar providers (L13)

All four sit behind one trait — `CalendarProvider { list_events(range) -> Vec<Event> }` — so the UI and detection logic never branch on provider.

| Tier | Provider | Auth | Setup cost | Operational reality |
|---|---|---|---|---|
| 1 (default) | **EventKit** | One macOS permission prompt | 🟢 ~2 days | Reads *every* account already in Calendar.app — iCloud, Google, Exchange, Outlook, CalDAV. Zero OAuth. Fails only if the user never configured Calendar.app |
| 2 | **Microsoft Graph** (`Calendars.Read`, delegated) | OAuth + PKCE, loopback redirect | 🟡 ~2 days | No app review needed. Personal *and* work accounts. Refresh tokens persist |
| 3 | **Google Calendar** (`calendar.readonly`) | OAuth + PKCE, loopback redirect | 🟡 ~2 days + config chores | **Sensitive scope.** Two hard rules: (a) set the OAuth app to *In production* — leaving it in *Testing* makes refresh tokens die every **7 days**; (b) unverified apps show a one-time "Google hasn't verified this app" screen → Advanced → Go to app. 100-user cap while unverified, irrelevant at L17 |
| 4 | **ICS URL** | None | 🟢 ~0.5 day | Paste a private `.ics` link. Read-only, refresh on a timer. Covers Zoho, Fastmail, self-hosted, anything |

**Security notes:** desktop OAuth uses PKCE with no client secret (Google and Microsoft both treat desktop client secrets as non-secret; PKCE is the required flow). Redirect is `http://127.0.0.1:<random>/callback` via `tauri-plugin-oauth`, never a custom URL scheme. Refresh tokens go to the macOS Keychain via `keyring`; access tokens stay in memory only. Scopes are read-only — the app never writes to a calendar.

### 2.8 AI / agent layer

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

### 2.9 Build, sign, verify

| Concern | Choice |
|---|---|
| Package manager | pnpm 9 |
| Rust toolchain | stable, pinned via `rust-toolchain.toml` |
| Target | `aarch64-apple-darwin` only (v1) |
| Swift sidecar | `swiftc` from Command Line Tools — **full Xcode not required**. Built by `just sidecar`, signed with the same identity, embedded at `Contents/MacOS/meet-stt` |
| Info.plist keys | `NSMicrophoneUsageDescription`, **`NSAudioCaptureUsageDescription`** (the tap permission key), `LSMinimumSystemVersion = 26.0` (A8), `CFBundleIconName` (A10) |
| App icon | "m." in white on the Dusk gradient tile, coral to violet (design-system/meet-ai/brand/README.md). Icon Composer source `design-system/meet-ai/brand/meet-ai.icon` (made by `render.sh`), compiled by `actool` into `src-tauri/icons/Assets.car`, which is **committed**. The bundle carries `Assets.car` in `Contents/Resources`; `icon.icns` still ships as the fallback. Xcode 26+ is needed only to regenerate `Assets.car` — routine builds stay CLT-only (A10) |
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

**A partly written meeting is called "Interrupted"** (TUR-97). A meeting folder is in one of three states, and the list says which:

| State | UI | What is on disk |
|---|---|---|
| Finished | nothing extra | Stopped on purpose: every WAV header declares exactly the bytes on disk *and* exactly the frames `segments.json` gives that channel (A5 §3's equality on graceful stop). Also any folder with no WAVs at all — nothing to judge by, e.g. after retention (L16) |
| **Interrupted** | label in the list; on open, one line: *"Recording stopped unexpectedly. Audio up to HH:MM:SS was saved."* | Anything else: no `segments.json` beside a WAV, a header behind the samples or behind the segments, or a header that cannot be read. `kill -9`, force quit, a crash, a dead battery |
| Recording | "● Recording" | The meeting this app is writing right now. Its files look interrupted until it stops, so it is never labelled that way |

An interrupted meeting opens exactly like a finished one — transcript, notes, audio, no error screen and no repair step. The word was picked because it says what happened and nothing more: not *failed* or *corrupt* (what was kept is good), not *partial* (nothing else is coming), not *recovered* (the user did nothing), and not finished. The time quoted is the header's (A5: header frames are the only true duration). Classification reads two 44-byte headers and `segments.json`, never the samples.

One silent fix-up runs at launch, before the record shortcut exists: a WAV whose header declares fewer bytes than are on disk **and** that has no `segments.json` beside it — every recording killed on v0.3.0, which never checkpointed and so left headers declaring 0 bytes over minutes of audio — gets its two header size fields raised to cover the whole frames already on disk. Nothing else is written: no sample is moved or truncated, a size is never lowered, a second run finds nothing to do, and the meeting stays Interrupted. A recording that has checkpointed is left as the recorder wrote it; its header is at most one checkpoint behind, and the samples past it were never in any `segments.json`.

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

**Line contract — binding, because L7 makes this file the source of truth.** These rules must hold before the first transcript is ever written; changing them later means migrating every recorded meeting.

| Rule | Detail |
|---|---|
| **One utterance = exactly one line** | A 40-second monologue is one long line. Editors soft-wrap it; the parser never has to reassemble anything |
| **Whitespace is collapsed** | `\n`, `\r`, `\t` and runs of spaces in recognized text all become a single space before writing |
| **No escaping** | The prefix is fixed-width and anchored, so `]` or `:` inside speech is safe. `(.*)$` takes the rest of the line verbatim |
| **Empty text is never written** | Whitespace-only results are dropped. This is the last line of defence for the whisper-hallucination guard |
| **Timestamps are utterance *start*** | Derived from `segments.json` (`start_host_ns + frame/rate`), never wall-clock at write time |
| **Append-only** | A line, once written, is never rewritten or reordered. See §2.5 on what is allowed to reach disk |

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
  sidecar/
    meet-stt/     # 🟡 Swift CLI: SpeechTranscriber, SpeechDetector, (FluidAudio later)
  src-tauri/      # 🟡 commands, events, tray, plugins, state machine
  src/            # 🟢 React app
```

**`crates/store` — watcher self-write suppression (required).** The app writes `notes.md` while the user is typing in it. Without suppression the watcher fires, the app reloads the file, and the cursor jumps mid-sentence.

- Keep a short-lived map of paths this process wrote (`path → Instant`). Drop `notify` events matching an entry newer than **750ms**.
- Second line of defence: compare a content hash before re-rendering, so an echo that slips through is a no-op.
- **Agent writes must not be suppressed.** `meeting.md` and `tickets/*.md` arrive as bursts from an external process; the debouncer already coalesces those. Suppression applies only to paths *this* process wrote.

Every 🔴/🟡 crate is independently runnable and fixture-testable. `crates/audio` ships `bin/meet-rec` — Phase 0 needs no Tauri and no UI.

`AudioSource` trait is defined in `crates/audio` from day one even though only macOS implements it — that is what makes the Windows port additive.

---

## 5. Phased build, with exit gates

Miss a gate → stop, don't stack work on a broken layer.

| Phase | Build | Exit gate |
|---|---|---|
| **0a. TCC spike** ~2d ⚡ | Swift CLI with `NSAudioCaptureUsageDescription` embedded via `-sectcreate __TEXT __info_plist`, signed with the same identity, in `Contents/MacOS/`, launched as a child of the app. **Precondition: `pnpm tauri build` + `just sign` must work first — `tauri dev` is NOT a valid test environment for TCC** (unsigned binary, different path, meaningless result) | **Does the permission prompt appear, name *meet-ai* rather than the helper, and does non-silent audio actually flow?** ⚠️ Prompt appears but samples are silent = **fail**, not pass — that is precisely the documented Tauri sidecar failure. Pass → capture lives in the sidecar, 🔴 drops to 🟡, ~1 week saved. Fail → in-process Rust exactly as L3 originally specced. Either way the project's biggest unknown is answered on day 2 |
| **0. Capture CLI** ~1–2wk 🔴 | `meet-rec` writes `mic.wav` + `system.wav` + `segments.json`. Permission prompt, device-change handling, incremental writes. Language decided by 0a | **45-min real Zoom call: both files intact, drift < 200ms end-to-end, survives an AirPods switch mid-call, survives `kill -9`** |
| **1. Transcribe** ~1wk 🟡 | `SttEngine` trait + `sidecar/meet-stt` (Apple `SpeechTranscriber`, native streaming) + `whisper-rs` fallback + lazy model download + `transcript.md` | Phase-0 call reads accurately on **both** engines. Speakers correctly split. Silence produces no invented text. Engine switch is a config change only |
| **2. App shell** ~2wk 🟢 | Tauri + React: meeting list, transcript view, notes pane, live transcript (native streaming on macOS 26, chunked on the whisper path), tray, ⌘⇧R, `/onboarding` incl. **permission-denied path** | You choose it over Notes for a real meeting |
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

**Fixtures** — `crates/audio/fixtures/`, generated once with `ffmpeg` (`just fixtures`). Every 🔴 module is testable against these with no live meeting:

| Fixture | Asserts |
|---|---|
| `silence-30s.wav` | **Zero** transcript lines, on both engines. The hallucination guard |
| `two-speaker-60s.wav` | Speaker split and timestamp sanity |
| `device-switch.wav` + `segments.json` | Multi-segment clock math, without needing a live AirPods swap |

**Phase 0a** — the precondition is a *signed release bundle*; `tauri dev` proves nothing here.
```
just bundle-signed                                    # tauri build + codesign + verify
tccutil reset AudioCapture pro.saleschat.meetai       # between every attempt
open src-tauri/target/release/bundle/macos/meet-ai.app
```
Pass = prompt appears, names **meet-ai** (not the helper), and non-silent samples arrive. ⚠️ Prompt appears but samples are silent = **fail**.

⚠️ **The `tccutil` line only works on a bundle signed with a real identity.** Under ad-hoc signing TCC keys the record to the executable *path*, and the reset is a silent no-op — measured both ways in FINDINGS §10.4. `just bundle-signed` must therefore use `$SIGN_IDENTITY`, never `codesign -s -`.

⛔ **Do not "copy the bundle to a new path" to force a fresh prompt.** An earlier revision of this line recommended exactly that; it is a one-way door. Each new path makes TCC create a **path-keyed** record, and those are permanent — `tccutil reset` resolves its argument as a bundle ID and returns `-10814` for a path, and deleting the directory leaves the record behind (measured, FINDINGS §10.6). Two are already stuck on the dev machine. Sign with the identity and reset by bundle ID; that is the only repeatable baseline. The identity's leaf SHA-1 is what every grant is keyed to, so it must not be re-minted between gate runs either — `spikes/phase0a-tcc/make-identity.sh` is idempotent for that reason, and `verify-tur10.sh` aborts if the leaf has moved.

**Phase 1:** `cargo test -p stt` against fixture WAVs, run **once per engine**, incl. a 30s pure-silence file that must yield **zero** transcript lines (the whisper-hallucination guard — Apple's engine should pass it trivially, whisper should only pass it with VAD gating).

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
| 🟡 whisper-rs Metal build config | Pin crate + toolchain; document the working `build.rs` env once it works. Now off the critical path — it is the fallback engine, not the default |
| 🟡 `SpeechAnalyzer` shipped with macOS 26 → thin LLM training data | Read Apple's docs and `FluidInference/swift-scribe` before writing the sidecar. Same rule as SETUP.md §4 |
| 🟡 Swift adds a second language to the build | Confined to one CLI binary with a JSON-lines contract. `swiftc` ships with CLT, so no Xcode dependency |
| 🟡 Windows AI Speech APIs are in preview | Verify at port time; `whisper-rs` small.en is the guaranteed floor |
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
| **Entitlements + Info.plist** | Already final (§2.9): hardened runtime, `com.apple.security.device.audio-input`, `NSAudioCaptureUsageDescription` | Same files feed the notarized build. Notarization then adds only a CI step, not a rewrite |
| **OAuth client IDs in config, not code** | `.app/config.jsonc` → `calendar.google.client_id`, `calendar.microsoft.client_id` | v2 swaps in a *verified* Google production client without a rebuild |
| **Onboarding as its own route** | `/onboarding` exists in v1 with 3 steps (permission → model download → meetings folder), even if plain. **Must handle denial**: one sentence on what breaks, a **Retry** button, and **Open System Settings** deep-linking to Privacy & Security via `x-apple.systempreferences:` — *verify the exact audio-capture anchor at implementation time*, falling back to the pane root. Recording controls stay visibly disabled while permission is absent, rather than failing at click time. ⚠️ **Detect denial with a positive control, not a return code and not an RMS floor** — both are proven not to work (FINDINGS §10.1–10.2, and confirmed against a real user **Don't Allow** in §10.7): on denial every `OSStatus` is `noErr` and the tap delivers bit-exact zeros, which is indistinguishable from a granted capture of a silent Mac. Play a ~200 ms known tone from meet-ai's own process, confirm it comes back through the tap, and only then call permission granted. Note also that `AudioDeviceCreateIOProcIDWithBlock` blocks for exactly as long as the consent dialog stands open (1 255 ms measured against a 1 250 ms dialog, versus 4.6 ms when no prompt is shown), so onboarding must not treat a slow return as a failure — it is the user thinking. The **microphone** half is probably exempt — `AVCaptureDevice.authorizationStatus(for: .audio)` has a real `.denied` case — but that is documented, not measured; verify it before trusting it | Public v1 users hit permissions cold. Growing an existing route is cheap; retrofitting a flow into a running app is not |
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
| **`SttEngine` trait** | Defined in `crates/stt` at Phase 1, with 2 impls (Apple sidecar, whisper) | Windows adds a third impl calling Windows AI Speech via the `windows` crate — **in-process, no sidecar needed**, since WinRT is directly callable from Rust |
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

### A10 — 2026-09-30 · The app icon ships as an Icon Composer `.icon`; Xcode is needed only to regenerate it (amends §2.9; narrows A1's toolchain note; TUR-35, TUR-85)

**Decision:** the app icon's source is `design-system/meet-ai/brand/meet-ai.icon`, made by `render.sh`. `actool` compiles it into `src-tauri/icons/Assets.car`, and that file is **committed** — the same call as `icon.icns`, which `.gitignore` already records as a committed `render.sh` output. The bundle gets `Assets.car` in `Contents/Resources` and `CFBundleIconName` in `Info.plist`. The `.icns` still ships as the fallback; it costs nothing to keep.

**No post-bundle step.** The TUR-35 plan assumed Tauri has no asset-catalog support, so `Assets.car` would be copied in after `tauri build` and the bundle re-signed. That was out of date: tauri-bundler 2.9 copies any `.car` listed in `bundle.icon` to `Contents/Resources/Assets.car` itself, next to the `.icns`. `CFBundleIconName` is set explicitly in `src-tauri/Info.plist` rather than left to Tauri's `assetutil` lookup, which only warns when it fails. `just sign` already seals the whole bundle after `tauri build`, so the catalog is covered with no new signing step; `sign` now refuses to seal a bundle whose `Assets.car` or `CFBundleIconName` is missing.

**Found on the way:** `src-tauri/Info.plist` still said `LSMinimumSystemVersion 14.4`, and that file overrides `tauri.conf.json`, so shipped bundles said 14.4 despite A8. Corrected to 26.0.

**What this buys, stated plainly.** Platform alignment, not a visible fix. On macOS 26 a legacy `.icns` means the system picks the icon container for us; a `.icon` is the supported way to control it. Today's icon is already correct on 26 at every size measured, so a signed bundle that looks the same as before is the expected result, not a failure. It does **not** buy per-size art: a `.icon` is one 1024 canvas the system renders every size from, so `meet-ai-appicon-16-fullcolor.svg` still reaches only the favicon and `.ico`. And there is no legacy or hybrid icon to build, because A8 put the floor at macOS 26.

**The toolchain note, narrowed — an addendum to A1, not a reversal.** *Xcode 26 or later is required only to regenerate `src-tauri/icons/Assets.car` from the `.icon` source; routine builds still need only Command Line Tools.* That holds because `Assets.car` is a portable binary that is committed, so a CLT-only machine builds and ships it untouched. §2.6, §2.9 and §7 all say the sidecar needs no Xcode; that stays true, since `swiftc` is still the only Swift involved. The compile step (`just icon-car`) points at Xcode for itself through `DEVELOPER_DIR`; nobody switches `xcode-select` globally for it. A machine that regenerates the icon needs a one-time Xcode setup (license and first launch, by full path since `xcode-select` stays on CLT); the commands are in `SETUP.md` step 0.4.

**Verified 2026-09-30** (TUR-86, `design-system/meet-ai/brand/proofs/tur86/RESULTS.md`): on a real signed bundle macOS draws `Assets.car`, not the `.icns`; the `.icns` fallback is pixel-identical to the old bundle; `codesign --verify --deep --strict` passes. Same at 11 of 13 sizes, better at 512@2x and 1024, where the `.icns` bundle was drawn inside a light system plate. Worse at none.

### A9 — 2026-09-28 · Whisper half of the silence-hallucination guard verified on real hardware (closes TUR-67; amends nothing in §1)

`whisper_writes_nothing_for_silence` (`crates/stt/tests/silence.rs`) has existed
since Phase 1 landed but sat behind the `whisper-model-tests` feature, which
`just check` deliberately never turns on. Nobody had run it against a real
model. That is a real gap: §5's Phase 1 exit gate names "silence produces no
invented text" for **both** engines, and only the Apple half and the pure-VAD
half (`apple_engine_writes_nothing_for_silence`,
`vad_finds_no_speech_in_either_silence_fixture`) had ever gone green.

**Run today**, on the arm64 dev machine, against the real, checksum-verified
`ggml-small.en-q5_1.bin` (SPEC §2.4): `just model` then `just check-whisper`.
Result: **118 tests passed, 0 failed**, including
`whisper_writes_nothing_for_silence` against both `silence-30s.wav` and
`room-tone-30s.wav`, and `whisper_streams_nothing_over_thirty_quiet_seconds`
(the live-session path). No hallucinated line on either fixture — the existing
three-layer guard (A4: VAD gating, `no_speech_thold`, the shape-plus-phrase
blocklist) holds against the real model, not just in theory. **No change was
needed to the VAD gating or thresholds** — this amendment records a
verification, not a fix.

**How this stays a guard and not a one-time check.** The test itself was
already a standing regression test, not a script someone ran once — that part
was never the gap. The gap was that it is feature-gated (correctly — `just
check` must not need 190 MB and a network call) and nothing made anyone
actually flip that feature on. There is still no CI in this repo, so the
repeatable mechanism is a documented manual gate, not an automated one:
`just check-whisper` is now named explicitly in `CONTRIBUTING.md`'s check
section, with the trigger conditions that make it non-optional — any change to
`crates/stt/src/vad.rs`, `crates/stt/src/whisper.rs`, or the model catalog, and
every Phase 1 sign-off. Adding this to an actual CI pipeline is future work,
not spent here — this repo has no CI for anything else yet either, and
standing one up is a bigger, separate decision (macOS runner, ~190 MB model
caching strategy) than this ticket's scope.

### A8 — 2026-09-27 · OS floor rises to **macOS 26+** (amends L2, L4, §2.4, §2.5, §2.9; closes TUR-37)

**Decision: meet-ai ships macOS 26 and later only.** Board call, made on the icon evidence below, and it settles the OS floor for the whole project — not just for icons.

**How it came up.** TUR-22 rebuilt the app icon with large art only (128pt and up). On macOS 26 that is correct and verified on two signed bundles. Below 26, the system has to downscale that art to 16px and 32px itself, and there is no macOS 14.x or 15.x machine or VM here to look at the result. TUR-41 simulated the downscale on this host: readable at every size, near-identical to the old hand-drawn art at 32px and 64px, but visibly softer and greyer at 16px — the crisp tile edge goes, though the two brackets stay separate and the orange dot stays a dot. A good stand-in, explicitly **not** a real-device check: macOS 14's exact filter and rep-selection are not guaranteed to match.

**Why raise the floor rather than accept the simulation.** The simulation could only ever buy confidence in the icon. It could not buy confidence in anything else that was never run below 26 — and nothing in this project ever has been. `FINDINGS.md` records this as a standing limit in its own words: *"macOS 14.4–26. Everything here is macOS 27.0 only."* Keeping 14.4 in the spec was claiming support for a tier with zero executed tests behind it, icons included. Raising the floor makes the spec true instead of aspirational, and it is the option that removes work rather than adding it.

**What this changes:**

| Was | Now |
|---|---|
| L2 floor `macOS 14.4+` | `macOS 26+` |
| `LSMinimumSystemVersion = 14.4`, `tauri.conf.json` `minimumSystemVersion: "14.4"` | `26.0` |
| Engine default: 26+ → Apple, 14.4–25 → whisper | mac → Apple, always |
| whisper-rs on mac = version fallback tier | whisper-rs on mac = **manual override only** |

**What this does *not* decide.** whisper-rs stays in the tree and stays in §2.5. It is still the Windows engine (L1: Windows next) and still the safety net behind engine 3, whose APIs are preview. Whether mac should keep a whisper path *at all* — which would also delete the model download manager, the Metal build config and mac-side VAD chunking from v1 — is a separate scope question and is **not** spent here.

**Deliberately left alone.** `FINDINGS.md` and `spikes/phase0a-tcc/build.sh` still say 14.4. They are records of what was true when the research and the Phase 0a spike ran; rewriting them would falsify a result rather than update a decision.

**Cost, stated plainly:** anyone on macOS 14 or 15 cannot run meet-ai. Given L17 (not public) and a single known user on macOS 26.6.2, that population is currently zero.

### A7 — 2026-09-27 · The permission-check tone is audible, and plays every recording, not just at onboarding (amends §8.1; spun out of TUR-10 as TUR-24)

A6 item 3 established *that* meet-ai must play a known tone and listen for it, but not what the tone sounds like or how often it plays. Three options were on the table: an audible chime, an inaudible (quiet or ultrasonic) tone, or a tone played once at onboarding only. Decided: **audible, and on every recording start, not just onboarding.**

1. **The control has to survive the exact path it is testing.** Quiet or ultrasonic content is the first thing Bluetooth codecs, device resampling and AGC throw away — the very loss the tone exists to detect. Loud enough to be heard is loud enough to be measured.
2. **It costs nothing extra in UX.** The app wants a "recording started" cue anyway; the same sound serves both jobs.
3. **Onboarding-only does not cover permission revoked mid-life.** macOS lets a user flip System Settings → Privacy & Security at any time, silently, with no signal to a running app. A check that ran once at setup would let meet-ai record an hour of silence after a later revocation and report success. Checking at the start of every recording catches that case; onboarding-only cannot.

Implementation: `crates/audio/src/chime.rs` — a rising two-note chime (A5 880 Hz → E6 1318.5 Hz, ~220 ms, −12 dBFS), a Goertzel-based detector requiring both notes present with contrast against each other (rejects a sustained tone that happens to contain both partials), and a poll-until-heard probe sized to the 1.07 s tap settle time measured in TUR-4. Full rationale and test coverage in that file.

**Open item, not yet measured:** whether the chime still reaches the tap when system output is muted or at zero volume. If it does not, a muted Mac reads identically to a denial. Until this is measured, "no chime" must surface as *needs explaining*, not a bare "permission denied" — see the ⚠️ in `chime.rs`.

*Decided by Nia (onboarding UX owner) on TUR-24. Rune (capture owner) had no preference beyond the tone surviving the audio path.*

### A6 — 2026-09-27 · L3 resolves to **in-process Rust** (resolves L3; supersedes the §5 Phase-0a pass branch)

L3 did not pre-decide the capture location — it delegated the decision to the Phase 0a spike and named the Swift sidecar only as the thing to *test first*. The spike passed, and §5's table reads "Pass → capture lives in the sidecar". **That branch is not being taken**, and this amendment records why rather than leaving the divergence in `FINDINGS.md` alone. Full evidence: `FINDINGS.md` §9, §9.1, §10.

**§5 chose the sidecar for one reason, and the spike removed it.** The worry was TCC attribution — that a bundled helper might not inherit the app's audio-capture grant, or would prompt in its own name. `FINDINGS.md` §8.2/§10.4 measured that it inherits cleanly and that the grant is keyed to the bundle ID. With that gone, the choice is ordinary engineering cost, and four things point one way:

1. **Nothing in the capture path needs Swift.** Every call the spike makes is plain C Core Audio (`AudioHardwareCreateProcessTap`, `AudioHardwareCreateAggregateDevice`, `AudioDeviceCreateIOProcIDWithBlock`), plus one Objective-C object, `CATapDescription`, which `objc2-core-audio` already binds. Contrast `SpeechTranscriber` (L4/A2), which is Swift-concurrency-native and genuinely unreachable from Rust — that is what the sidecar exists for.
2. **The process boundary would land on the hardest exit gate.** Phase 0 is graded on drift < 200 ms between the two tracks. The mic side is already Rust (`cpal`). A capture sidecar means two processes, two clocks, a pipe between them, and two writers for `segments.json` — across the one measurement most likely to fail slowly and silently (§7 🔴).
3. **The permission self-check is now mandatory, and it is much cheaper in-process.** §10.1 measured that a *denied* tap is indistinguishable from a silent room: every `OSStatus` is `noErr`, the IOProc is created, callbacks fire at the normal rate, and every sample is a bit-exact zero. So meet-ai must play a known tone and confirm it returns through the tap before claiming permission (A-note: this supersedes §8.1's return-code check — see §10.2). In one process that is a function. Across a sidecar it is an IPC handshake with cross-process tone synchronisation, on the startup path, before the UI can enable recording.
4. **The Windows seam only exists in Rust.** §8.2 puts `AudioSource` + the `stub-audio` `--target x86_64-pc-windows-msvc` cross-check into `just check` from day one and ⛔-marks OS-specific code outside `crates/audio/src/macos/`. A Swift capture sidecar leaves that seam untested and unported.

**Rejected middle ground:** Swift tap + Rust mic. It puts the boundary exactly where it hurts (drift) and keeps two languages in the 🔴 module.

**What this costs, stated plainly.** ~250 lines of working Swift get ported to `objc2-core-audio`. `crates/audio` stays 🔴 rather than dropping to 🟡, and the ~1 week §5 hoped to save is **not available** — re-plan Phase 0 at its original 1–2 weeks.

**Conditions attached to the decision:**

- The Phase 0a spike bundle (`spikes/phase0a-tcc/`) is retained as the **test oracle**, not merely reference. The Rust port must reproduce the spike's numbers against the same tone before Phase 0 is called done.
- **§5's kill criterion is unchanged and still live:** drift unsolved after 2 weeks → rewrite as native Swift, macOS-only, Windows dropped permanently. This amendment does not spend that escape hatch; it is the fallback if the port fails on its own terms.
- `sidecar/meet-stt` keeps transcription only, exactly as A2 scoped it. §2.6's table row "System audio capture — only if Phase 0a passes" is now **struck**: capture never goes in the sidecar.

*Decided by Alen (chief of staff) on TUR-4, on the escalation from TUR-3/TUR-10. Recommended by Rune in `FINDINGS.md` §9.*

### A5 — 2026-09-27 · `segments.json` gains checkpoint anchors (amends §3.4, §6; no §1 decision touched)

The Phase 0 exit gate in §5 is "drift < 200ms end-to-end, **with the measured number reported**", and §6 says `drift-check` gets that number by reading `segments.json`. Writing the reader first — `crates/audio/src/segments.rs`, tests included, before any capture code exists — showed that the §3.4 shape cannot produce it.

**1. §3.4's shape cannot express the gate.** A 45-minute call with no device switch is **one** segment: one `start_host_ns` and two frame counts. The only quantity derivable from that is `(mic_frames − sys_frames) / rate`, which compares the two tracks against *each other* and never against a clock. Three ways that fails:

- **Common mode is invisible.** When both tracks run off the same device clock — the same AirPods, the same USB interface — they slide off wall time together and the subtraction reads ≈0. A transcript a quarter-second out by minute 45 passes the gate. `common_mode_drift_is_invisible_to_a_track_subtraction_and_caught_by_anchors` is that exact case, 100 ppm on both channels: the naive metric reads <1 ms, the real drift is 270 ms.
- **It cannot separate drift from teardown.** The two streams stop at different instants, so ragged shutdown and genuine clock error land in the same subtraction.
- **It can be structurally zero.** Drive the resampler at a fixed ratio off input frames and output counts stay locked together by construction. A gate that cannot fail is not a gate — §7's 🔴 "silent failure at ~30min" risk, wearing a green light.

**So each segment carries an `anchors` array — one entry per 5-second checkpoint:**

```json
{"version":1,"segments":[{"idx":0,"start_host_ns":123456789,
  "start_continuous_ns":123456789,"start_unix_ns":1759000000000000000,
  "mic_rate":16000,"sys_rate":16000,"mic_frames":43200000,"sys_frames":43200000,
  "reason":"start","mic_device_rate":48000,"sys_device_rate":48000,
  "anchors":[{"mic_host_ns":128456789,"mic_frames":80000,
              "sys_host_ns":128451203,"sys_frames":79998}]}]}
```

An anchor pairs the IO callback's own `mHostTime` with the WAV-domain frame index that buffer ends at — per channel, since the two callbacks fire independently, and *not* the flush position, which would measure our own writer latency instead of the device clock. Drift per channel is then `frames/16000 − (host_ns − start_host_ns)/1e9`, which is a curve with a maximum, a final value and a crossing minute. ~540 rows for a 45-minute call, a few tens of KB against ~230 MB of audio.

Consequences, all of them additive — `crates/stt` ignores unknown fields and needs no change:

- **`mic_rate`/`sys_rate` are `16000` in every segment of a real recording.** A WAV header carries one rate and we write one file per channel, so a device-rate change is absorbed by the resampler (§2.3) rather than expressed in the file. §3.4's `"mic_rate":48000` literal is illustrative and cannot occur. The hardware rate moves to `mic_device_rate`/`sys_device_rate`.
- **Frame 0 of both channels is `start_host_ns`.** Whichever stream comes up later is head-padded with silence, and **those padded frames count** in `*_frames` and in every anchor — the pad stands for real elapsed time.
- **A segment boundary is a real gap and is never padded.** `drift-check` reports it per boundary in milliseconds instead of asserting a switch "survived".
- **Sleep is a segment boundary of its own.** `mach_absolute_time()` does not advance while the machine sleeps, so a lid closed for twenty minutes moves `start_host_ns` by ~nothing and every later transcript line lands twenty minutes early. A wake therefore closes the segment and opens a new one with `reason:"system_wake"`, and each segment carries `start_continuous_ns` (mach *continuous* time) and `start_unix_ns`. `Δcontinuous − Δhost` across the boundary is exactly the time asleep.

**2. §6: `drift-check` refuses rather than flatters.** With no system track it must exit non-zero with "not measurable" — treating a missing tap as `sys_frames = 0` and subtracting reports a passing number for a recording with no system audio in it at all. Same refusal for a file with no anchors, a non-monotonic anchor series, or anchors claiming more frames than their segment.

**3. The `kill -9` frame-count invariant is an inequality, not an equality.** `segments.json` and the two WAV headers are three separate writes and the trio is not atomic. Each checkpoint writes sample bytes → `segments.json` (temp file + `rename(2)`) → WAV headers in place, because the two crash windows are not equally bad. Header first and the header declares frames no segment accounts for: real speech gets decoded with **no timestamp** and silently dropped. `segments.json` first and the segments describe frames the header does not expose yet, which nobody ever asks about. So:

> `sum(*_frames across segments) >= wav_header_frames` for each channel, always — with equality after a graceful stop.

Strict equality is not achievable across three writes under an arbitrary kill and must not be asserted. A corollary for every consumer: a recording's duration is `wav_header_frames / 16000`, never `sum(*_frames)`, which after a crash overstates by up to one checkpoint interval. Worst-case tail loss on force-quit is therefore ≤ 5 s, and that is the tolerance to test against.

### A4 — 2026-09-27 · Phase 1 implementation notes (amends §3.5, §6; corrects §2.4 and SETUP.md §1.1)

Building Phase 1 turned up four things the spec either did not say or said wrongly. None touches a §1 locked decision.

1. **§3.5 gains `transcription.engine`.** The Phase 1 exit gate requires that "engine switch is a config change only", but `config.jsonc` had no field to change — only `model`, `language` and `live`. Added `"engine": "auto"`, with `"apple-speech"` and `"whisper"` as explicit overrides. `auto` is the §2.5 defaults table. Forcing an engine that is unavailable is an **error**, not a silent fallback: if you asked for it, you want to know why you did not get it.

   ```jsonc
   "transcription": {
     "engine": "auto",              // or "apple-speech" | "whisper"
     "model": "large-v3-turbo-q5_0",
     "language": "en",
     "live": true
   }
   ```

2. **§2.4 model size was wrong.** `ggml-large-v3-turbo-q5_0.bin` is **574 MB**, not ~1.6 GB — that figure belongs to an unquantized large-v3. `small.en-q5_1` is 190 MB as stated. The first-run download risk in §7 is correspondingly smaller. Both files are pinned by URL *and* SHA-256, taken from Hugging Face's published LFS object ids rather than computed from a local download, which would only prove the bytes matched themselves.

3. **SETUP.md §1.1 mis-describes `earshot`.** At the pinned 1.2.2 it is **not** a WebRTC VAD port; it is a small neural detector scoring 256-sample (16 ms) frames at 16 kHz and returning 0..1. The choice still holds for the reasons given (pure Rust, `libm` its only dependency, no bundled ONNX model), but there are no WebRTC aggressiveness modes to reach for — there is a threshold, in `stt::vad::SegmentConfig`. The `Vad` trait seam for Silero is in place as §1.1 intended.

4. **§6 gains `room-tone-30s.wav`.** `silence-30s.wav` is digitally perfect zeroes, which any VAD passes trivially. Measured against `small.en-q5_1`, ungated whisper produced phantom text on *every* quiet input tried: `[BLANK_AUDIO]` on pure silence, `[no speech detected]` on quiet pink noise, `(water rushing)` on louder pink noise. The hallucination guard is therefore three layers, and the fixture set needs a noise case to exercise them:
   - **VAD gating** — whisper is only ever handed sample ranges the detector returned. Silence yields zero spans, so zero inference calls. This is the layer that does the work.
   - **`no_speech_thold`** — whisper.cpp's own per-segment probability. All three phantoms above scored 0.79–0.97 against a 0.6 threshold.
   - **A shape rule plus a phrase list** — a segment wrapped entirely in `[...]`, `(...)` or `*...*` is a sound annotation, not speech. Being structural, it catches annotations nobody has seen yet, which an enumerated list cannot; only `[BLANK_AUDIO]` was on the list.

**Still open at the end of Phase 1.** The gate is verified against synthetic text-to-speech fixtures only. TTS has no room tone, no codec artefacts, no crosstalk and no overlapping speakers, so it proves the plumbing, the speaker split and the timestamp maths — not real-world accuracy. Phase 1 is not complete until the same checks run against a real `meet-rec` recording from Phase 0.

### A3 — 2026-09-01 · Pre-implementation audit (amends §3.4, §2.5, §2.3/§4, §5, §6, §2.6, §8.1)

A pre-flight audit against the real repo found nine gaps. Three would have cost real time.

1. **Relicensed MIT → Apache-2.0.** A `LICENSE` was already committed as MIT while SETUP.md instructed Apache-2.0; the step would have silently overwritten it. Apache-2.0 chosen for its explicit patent grant, which matters once L17 flips to public. Sole author, so the relicense is clean; MIT remains in prior commits, as expected. `NOTICE` added.
2. **§3.4 transcript line contract made binding** — one utterance = one line, whitespace collapsed, no escaping, empty text never written, append-only. These had to land *before any transcript exists*: L7 makes markdown the source of truth, so a later change means migrating every recorded meeting.
3. **§2.5 "only finalized text is persisted."** Apple's engine emits volatile → finalized; without this rule it would write and then rewrite lines in `transcript.md`. All engines now normalize through one `TranscriptSink`. The live pane and the file are deliberately not identical mid-meeting.
4. **§4 watcher self-write suppression** (750ms + content hash). Without it the app reloads `notes.md` underneath the user's cursor while they type. Agent writes are explicitly exempt.
5. **§5/§6 Phase 0a precondition: a signed release bundle.** TCC keys on the signed bundle identity, so `tauri dev` is not a valid test environment — running the spike there yields a false pass or false fail on the project's biggest unknown. Also recorded the trap: prompt appears but samples are silent is a **fail**.
6. **§2.6 sidecar test strategy** — tested from Rust (`crates/stt/tests/sidecar.rs`) against fixture WAVs, not via XCTest. One suite, and it tests the actual process/JSON contract.
7. **§6 fixture WAVs named** so they get created rather than assumed. **§8.1 onboarding** now owns the permission-denied path.

**Confirmed frozen:** product name `meet-ai`, identifier `pro.saleschat.meetai`. **Verified on the build machine:** `swiftc` 6.3.3 targeting `arm64-apple-macosx26.0` and `Speech.framework` present — A2's sidecar is buildable today with zero installs.

### A2 — 2026-09-01 · Swift sidecar for Apple frameworks (amends L2, L3, L4)

Research into macOS 26 changed the transcription picture materially. Apple's `SpeechAnalyzer` / `SpeechTranscriber` ships with the OS (**0 MB download**), runs ~**2× faster than whisper large-v3-turbo** at top-tier on-device accuracy, and does **native long-form streaming** with volatile → finalized results. All of it is Swift-only — the API is built on Swift concurrency and is not reachable through `objc2`.

**Change:** add one Swift CLI, `sidecar/meet-stt`, inside the app bundle. New §2.5 (engine matrix) and §2.6 (sidecar scope).

**Why a sidecar is safe here** where L3 rejected one: transcription reads a WAV off disk — no microphone, no audio-capture permission, nothing for TCC to attribute. The TCC risk applies only to *capture*, which Phase 0a now tests explicitly in 2 days rather than assuming.

**What this deletes from the critical path:** the 1.6GB first-run download on macOS 26, `whisper-rs` Metal build config, the model download manager as a Phase-1 blocker, and the entire VAD-chunking design on mac (L4's "2–8s utterance lag" becomes true live captions with *less* code).

**Framework decision re-confirmed, not reopened.** Swift-on-Windows was evaluated and rejected: every win above is an Apple *framework*, none of which exist on Windows, and **SwiftUI does not exist there at all**. The Browser Company had to build its own WinUI language bindings to ship Arc on Windows, then pivoted away from SwiftUI for Dia over performance. For a solo vibe-coded project the decisive factor is training data: SwiftWin32 / swift-winrt have almost none, while React is the densest region there is. The sidecar captures 6 of 7 Apple wins; going all-Swift would buy exactly one more (App Intents/Shortcuts) at the cost of Windows and the entire UI velocity story.

**Also:** Windows has its own free on-device STT (Windows AI Speech Recognition — preinstalled on Copilot+ NPUs, on-demand on CPU-only), and it is WinRT, so Rust calls it **directly with no sidecar**. Cloud-only-on-Windows was considered and rejected: it would break the privacy claim on a whole platform, it is not actually cheap (25MB API caps vs ~350MB WAVs means Opus encode + chunking + reassembly), and it fails offline — the exact situation meetings get recorded in.

### A1 — 2026-09-01 · Dependency research (see [`SETUP.md`](./SETUP.md))

Live registry check produced four substantive changes, all simplifications. No §1 decision is affected.

1. **VAD: Silero+`ort` → `earshot` 1.2.2.** `voice_activity_detector` 0.2.1 pins `ort =2.0.0-rc.10` — an exact pin on a pre-release ONNX runtime. `earshot` is pure Rust with `libm` as its only dependency. Removes `ort`, `ort-sys`, `ndarray` and the 2MB `silero_vad.onnx` from v1. Silero stays available behind a `Vad` trait if Phase 1's silence test shows hallucination on noise.
2. **Frontmatter: `serde_yaml` + `gray_matter` → `yaml-rust2` 0.12.** `serde_yaml`'s latest release is literally `0.9.34+deprecated` (Mar 2024). `yaml-rust2` also satisfies the preserve-unknown-keys requirement properly, since nothing is deserialized into a struct.
3. **Config: `json_comments` → `jsonc-parser` 0.33.1.** The former last shipped in 2023.
4. **Three deliberate non-latest pins** where a new major buys nothing this project needs and costs LLM familiarity: `sha2` 0.10.9 (not 0.11), `reqwest` 0.12.28 (not 0.13), `notify` 8.2.0 (9.0 is an rc).

Also recorded: this machine has **no Rust toolchain installed** and no `just`; full Xcode is **not** required (Command Line Tools suffice, since L2 drops ScreenCaptureKit). Toolchain pinned to Rust **1.98.0**. *(Narrowed by A10: Xcode 26+ is needed only to regenerate the app icon's `Assets.car`.)*
