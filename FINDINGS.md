# Botless Meeting Assistant — Grounded Review (v2)

**Date:** 2026-09-01
**Input:** `Readme.md` (Discovery v1, decisions "locked")
**Purpose:** challenge every decision against what comparable projects actually shipped, then cut v1 down to what is realistically buildable by an LLM-driven (vibe-coded) workflow.
**Verdict in one line:** product thesis is strong and unoccupied; the *stack* claims are half-supported by the evidence and the v1 scope is roughly 3x too big. Six decisions change, three shrink, three survive intact.

---

## 1. Evidence base — what the comparable projects actually run

Every repo below was verified live, not recalled.

| Project | Shell | Capture | STT | Storage | Summarizer | Signal |
|---|---|---|---|---|---|---|
| **Muesli** (1.1k★) | **Native Swift / AppKit + SwiftUI** | CoreAudio **process tap** default, ScreenCaptureKit fallback | CoreML on Neural Engine | **SQLite (WAL)** | OpenAI / OpenRouter / Ollama | Best capture reference — and it is *not* Tauri |
| **pasrom/meeting-transcriber** (151★) | **Native Swift 6.2 / SwiftUI** | `CATapDescription` (macOS 14.2+) | WhisperKit *or* Parakeet TDT v3 | files/local | **Claude Code CLI** *or* OpenAI-compatible *or* none | Proves D7 works in a shipped app — pluggable, not Claude-only |
| **Meetily** (Zackriya) | Tauri + Next.js + Rust | mic + system | Whisper **or Parakeet**, GPU accel | **SQLite** | Ollama | Closest architectural twin; "4x faster Parakeet" is their headline |
| **Hyprnote → char → anarlog** (YC S25) | Tauri monorepo | mic + system | Whisper local | **SQLite** | HyprLLM (Qwen3-1.7B finetune) local | Same product, funded, team already pivoted twice |
| **open-granola** | Tauri + Vite/TS | per-platform loopback | Whisper | **SQLite + vector** | local GGUF | **3 stars / 1 fork** — a weekend project, not a reference |
| **Granola** (the incumbent) | Electron | ScreenCaptureKit + AVFoundation | cloud | cloud | cloud | **Records first, transcribes on stop** — deliberately, for stability |

Three conclusions fall straight out:

1. **The two best macOS capture references are pure Swift.** The Readme's "ecosystem consensus is Tauri" is overstated — the Tauri examples are one pivoted startup, one direct competitor, and one 3-star repo.
2. **Storage consensus is SQLite. Unanimously. 5/5.** The Readme's "no database" is a deliberate contrarian bet, not a consensus choice — it must be defended as such.
3. **Nobody streams whisper live and calls it done.** The market leader chose *not* to.

---

## 2. Decision-by-decision challenge

Legend — **KEEP** (evidence backs it) · **CHANGE** (evidence contradicts it) · **SHRINK** (right idea, wrong v1 size)

### D1 — Tauri 2.0 + Rust + Swift helper → **CHANGE (partly)**

- **Claim challenged:** "ecosystem consensus." Not really: Muesli and meeting-transcriber, the two repos you'd actually copy capture code from, are native Swift. Tauri buys you Windows (D10) — that is the real and sufficient reason. Say that instead.
- **Hard problem the doc doesn't price in:** a **Swift sidecar does not reliably inherit the app's TCC permissions**. Open Tauri issues: sidecar `PermissionDenied` (#4653), sidecar doesn't inherit accessibility rights (#8329), macOS mic permission never prompted (#11951). Unsigned/ad-hoc-signed binaries can fail to even register in the TCC database. A separate helper process is the single most likely thing to eat a week with no visible progress.
- **Do instead:** call Core Audio taps **in-process from Rust** via `objc2-core-audio` (bindings for the tap object and `AudioHardwareCreateProcessTap` exist today). TCC identity then = the app bundle, one permission prompt, one signature. Keep the Swift helper as *fallback only*, and if used, sign it with the same Team ID + inherit entitlement inside the bundle.
- **Keep:** Tauri 2.0, Rust core, React/TS frontend. ~45MB vs ~400MB argument stands and matters here (you run beside Zoom).

### D2 — Local by default, BYOK cloud → **KEEP**

5/5 references do local-first. Nothing to challenge. This is the only decision with unanimous support.

### D3 — Real-time streaming transcription → **CHANGE**

- **Evidence against:** Whisper was trained on 30-second clips. On short/low-energy chunks it hallucinates subtitle-like text and produces "ghost transcripts" that repeat and shuffle previous chunks. Correct streaming needs a 10–15s rolling buffer, emit-only-the-stable-prefix, overlap reconciliation, plus a hallucination filter. Granola looked at this and chose record-then-transcribe as "simpler and more stable."
- **This is also the worst possible vibe-coding target** — an incremental-merge algorithm with no test oracle, exactly the "dirty weeds of an algorithm" class where LLM output needs constant re-alignment.
- **Do instead — v1 = near-real-time, utterance-level:** Silero VAD segments on speech boundaries → transcribe each *finished* utterance → append. Lag 2–8s, feels live, zero overlap-merge code, no silence hallucinations (skip the whisper call when VAD sees no speech). If true low-latency is ever wanted, switch that channel to **Parakeet streaming** (`parakeet-rs` / `sherpa-rs`, NVIDIA Parakeet Realtime EOU 120M) rather than fighting whisper.

### D4 — Two channels = free 2-way diarization → **KEEP, with one correction**

- Correct and cheap. But it yields *two* labels, not N — "Others" with 4 people is one blob, which weakens U6 (ownership map) and U4 (who said the estimate) until v2.
- **Correction to the v2 plan:** "pyannote-style" in the doc implies Python. In a Rust app the path is **`sherpa-rs` speaker embedding/diarization**, not pyannote. (Muesli and meeting-transcriber get pyannote-via-FluidAudio on the ANE only because they're Swift — that path is closed to you.)
- **Unpriced risk:** if the user isn't wearing headphones, mic *and* loopback both contain everyone → duplicated transcript lines. Needs an energy/cross-correlation dedupe or an enforced headphone warning. Decide in v1, it's user-visible.

### D5 — Auto-detect, user confirms → **KEEP**

Cheap, matches Granola's flow, avoids the surprise-recording failure mode. No change.

### D6 — Google + Outlook OAuth in v1 → **CHANGE (biggest scope win available)**

- **Evidence against:** Google Calendar read is a **sensitive scope** → mandatory verification before public distribution: per-scope written justification, demo video of the full consent flow, authorized domain verified in Search Console, privacy policy hosted on your domain. That's weeks of *non-code* work, plus shipping an OAuth client secret inside a distributed desktop binary.
- **Do instead:** **EventKit on macOS.** It reads the unified calendar store — iCloud, Google, Exchange, Outlook, CaldAV, all accounts the user already configured — behind one standard macOS permission prompt and **zero OAuth**. This is what MeetingBar, Mindwtr and Morgen do. Windows later: Microsoft Graph (easier registration, no video review) or a subscribed `.ics` URL as the universal fallback.
- **Result:** D6 goes from "two OAuth apps + Google review" to "one native API call." Google OAuth deferred to whenever a real user needs it.

### D7 — User's Claude Code CLI as the analysis engine → **KEEP, but pluggable**

- **Best-supported non-obvious decision in the doc.** meeting-transcriber (151★, shipped) does exactly this — and notably offers **Claude Code CLI *or* OpenAI-compatible (Ollama/LM Studio) *or* none**. Mirror that shape, don't hard-depend.
- **Risks to price in:** non-deterministic output shape, CLI version drift, multi-second cold start, and subscription rate limits (Max plan comfortably drives ~1–3 steady agents; heavy automation wants an API key).
- **Do:** `claude -p --output-format json`, validate against a JSON schema, one retry, then degrade. Ship **Ollama as the fallback analyzer**, not "transcript-only mode" — every competitor ships a local summarizer and users will expect one.

### D8 — Jira + Linear + GitHub sync in v1 → **SHRINK hard**

- Three trackers = 3 auth flows, 3 field mappings, 3 conflict models, plus webhooks if status is to round-trip. This is a quarter of work that produces zero product differentiation.
- **Leverage you already have:** Claude Code has first-party Linear and Atlassian/Jira connectors, and `gh` for GitHub Issues. The analysis skill can file the ticket **using the user's existing auth** — no sync engine, no keychain, no token UX, no API drift owned by you.
- **v1:** markdown tickets + one "Push to tracker" action that hands the ticket to Claude Code. Build a *native* sync engine later, for the one tracker you personally use, only if delegation proves too flaky.

### D9 — Markdown only, no database → **CHANGE to markdown + rebuildable index**

- **Evidence against is unanimous:** SQLite in Meetily, Hyprnote, Muesli (WAL), open-granola (+vector). You'd be the only one without it.
- Markdown-as-truth is genuinely your differentiator (greppable, Obsidian-native, agent-native) — **keep it as the source of truth.** But the UI needs list/filter/sort across hundreds of meetings, full-text search, and "all open tickets" queries. Spawning `grep` from a webview for every keystroke is the wrong shape, and Claude Code grepping a 500-meeting corpus burns tokens for a job FTS5 does in a millisecond.
- **Do:** a **derived** SQLite index (`.app/index.db`) built by a file watcher. FTS5 now, `sqlite-vec` later without re-architecting. **Invariant: nothing exists only in the DB — delete it and a rescan rebuilds everything.** That keeps the philosophy and buys the ergonomics.

### D10 — macOS + Windows together in v1 → **CHANGE to macOS first**

- Shipping both doubles the *hardest* layer (capture) and forks the STT performance story (Metal vs low-end Windows CPU, model tiering, Parakeet path) before a single validated user — and n=1 is you, on a Mac.
- Meetily and Hyprnote both shipped macOS first; their Windows builds arrived later and noisier.
- **Do:** macOS to daily-usable, then Windows as v1.1. Put capture behind a Rust trait (`AudioSource`) on day 1 so the port is additive, not surgery. `wasapi` crate already supports loopback (including per-process) with working examples, so the Windows port is a known-cost item you can safely postpone.

### D11 — MCP server in v1 → **KEEP, with a structural tweak**

- Cheapest high-leverage item in the doc. `rmcp` is now **Tier 1** conformance (promoted Aug 2026, stable 3.0.1, 67/67 server conformance), generates JSON Schema from Rust types at compile time, ~7MB idle.
- **Tweak:** ship it as a **separate small binary over the corpus + index**, not embedded in the app process. Then it works when the app is closed, has no window/TCC dependencies, and can be vibe-coded and tested in complete isolation from the audio stack.

### D12 — Full in-meeting workspace → **SHRINK**

During a meeting you are *in the meeting*. Granola's entire UX bet is one note pane; the transcript is deliberately secondary. Five simultaneous panels is React surface area spent on the moment the user has least attention.
**v1:** note pane + collapsible live transcript. Tasks / memory recall / upcoming move to the pre- and post-meeting views, where they're actually read.

---

## 3. USPs — challenged

| USP | Verdict | Why |
|---|---|---|
| **U1 Start Work button** | **Build first. Confirmed.** | Pure prompt assembly over data you already have. Makes the pitch literal. Cheapest wow in the doc. |
| **U5 Pre-meeting brief + git** | **Keep, #2.** | Needs only calendar + `git log`. The git twist is the genuinely novel bit and it's ~50 lines. |
| **U3 MCP slash commands** | **Keep, #3.** | Rides the MCP binary. Commands are the product, plumbing is done. |
| **U7 Hooks** | **Keep, trim to 3.** | Seven hooks is a versioned-payload contract to maintain before anyone has asked for one. Ship `on_transcript_ready`, `on_analysis_complete`, `on_meeting_end`. Add the rest when someone requests them — the directory convention makes that free. |
| **U8 JSONC config** | **Keep. Well-argued.** | `$schema` → free VS Code autocomplete is the correct call for this audience. One note: `serde_jsonc`/`jsonc-parser` are less battle-tested than TOML in Rust; pin and test round-trip writes. |
| **U2 Drift detection** | **Defer, as the doc says — but harder.** | Depends on decision extraction being *reliable*, which it won't be for months. A false "this PR contradicts the meeting" is worse than silence. Needs a confidence gate before it ships. |
| **U4 Estimate calibration** | **Keep, near-free.** | Extraction is one prompt line; the payoff needs months of data. Start writing `estimate:` frontmatter in v1 so the dataset exists — the *feature* ships in v2. |
| **U6 Ownership map** | **Defer past v2.** | Blocked on real N-speaker diarization (D4). Two labels can't attribute ownership. |

**Gaps the doc misses entirely:**

- **Crash safety.** Recording must survive an app crash mid-meeting — incremental WAV writes with periodic header flush. Part of why Granola and Meetily record-then-transcribe.
- **First-run download.** 2–3GB of models. Needs resumable download, checksum, and a visible progress story. Use quantized `large-v3-turbo` (~1.6GB), not full large-v3.
- **Two-stream clock alignment.** Mic and loopback are separate clocks at different sample rates (48k loopback → 16k for whisper). Without a shared timebase the two transcripts drift apart over a 45-minute call and interleaving breaks. This is a concrete, un-vibe-codeable detail — write it down as a task, not a footnote.
- **Retention.** "Audio deleted after transcription" needs an undo/keep-last-N, or the first bad transcript is unrecoverable.

---

## 4. Vibe-coding achievability

Grades reflect where LLM-driven implementation lands with a compile-fix loop and no hand-holding.

### 🟢 Green — expect this to essentially write itself
React/TS UI · markdown frontmatter read/write · SQLite index + FTS5 + file watcher · MCP server via `rmcp` (schema from types, tight compile loop) · JSONC config + JSON Schema · hooks runner (spawn + timeout + log) · `claude -p` invocation and JSON validation · ticket UI/CRUD · U1 prompt assembly · U5 `git log` parsing.

### 🟡 Yellow — works, but budget a debug cycle
Tauri plumbing (IPC, tray, hotkeys, updater) · `whisper-rs` build and per-platform bundling (documented gotchas on Windows and Apple Silicon) · resumable model downloader · Silero VAD segmentation · WASAPI loopback (crate + examples exist) · EventKit via `objc2` · signing + notarization CI.

### 🔴 Red — plan to read Apple docs and port working code by hand
1. **Core Audio process tap setup.** Officially "poorly documented," and CoreAudio's shape makes correct setup hard to derive. **Mitigation: port from `insidegui/AudioCap`** (Apple-quality sample for 14.4+) and sudara's 14.2 tap gist. Do not expect this generated from a prompt.
2. **Two-stream clock alignment + resampling.** No test oracle, silent failure mode, drift only visible at the 30-minute mark.
3. **Streaming overlap-merge / hallucination filtering.** Avoided outright by the D3 change — that's most of why D3 changes.
4. **TCC permission behavior across bundle/sideload/signing states.** Trial and error against `tccutil`, not a code problem.
5. **Acoustic echo cancellation.** Don't. Recommend headphones in v1.

**The rule that makes the red zone survivable:** every red item goes in **one small Rust module with a CLI entry point and a fixture-WAV test harness**, so iteration doesn't require a live Zoom call and failures can't hide behind the UI.

---

## 5. Revised v1 — the achievable cut

Each phase has an exit gate. Miss the gate, stop and reconsider — don't proceed with a broken layer underneath.

| Phase | Deliverable | Exit gate |
|---|---|---|
| **0. Capture CLI** (mac) | `meet rec` → `mic.wav` + `system.wav`. No UI, no Tauri. | 45-minute real Zoom call recorded, both files intact, **drift < 200ms end-to-end**, survives an AirPods switch. |
| **1. Transcribe on stop** | `whisper-rs` + large-v3-turbo → `transcript.md`, You/Others labeled, timestamped. | The Phase-0 call reads as an accurate transcript. |
| **2. App shell** | Tauri + React: meeting list, transcript view, note pane, VAD-chunked live utterances. | You choose it over Notes for a real meeting. |
| **3. Analyzer** | `claude -p` + skill → summary, decisions, action items, MD tickets. Ollama fallback. | 5 consecutive meetings produce usable tickets with no manual repair. |
| **4. Leverage** | MCP binary over corpus + index; **U1 Start Work**. | One meeting → ticket → Claude Code session → merged PR, end to end. |
| **5. Calendar + brief** | EventKit, auto-title, 1-min reminder, **U5 git brief**. | You open the app before meetings without being prompted. |
| **6. Windows** | WASAPI loopback behind the `AudioSource` trait. | Parity on the Phase-1 gate. |

**Then, in order:** hooks + config (U7/U8) → tracker delegation (D8) → real diarization via `sherpa-rs` → U4/U2.

**Cut from v1** (was in, now out): Google/Outlook OAuth · native Jira/Linear/GitHub sync · Windows at launch · true streaming transcription · 5-panel workspace · 7 hooks · ML diarization.

**Kill criteria — be honest with yourself up front:**
- Phase 0 drift unfixable after two weeks → go native Swift, macOS-only, and drop Windows from the plan entirely.
- Phase 3 tickets need hand-repair every time → the product is a transcriber, not a PR pipeline. Reposition.
- You stop using it yourself for two weeks → the daily-habit hook (U5) is missing, not the features.

---

## 6. Revised stack (deltas only)

```
Shell          Tauri 2.0 + React/TS/Tailwind          (unchanged; justified by Windows, not by "consensus")
Core           Rust                                   (unchanged)
macOS capture  objc2-core-audio process tap IN-PROCESS (was: Swift sidecar — TCC inheritance is broken)
               ScreenCaptureKit fallback for <14.4
Windows        wasapi crate loopback, behind AudioSource trait   (v1.1, not v1)
STT            whisper-rs + large-v3-turbo (q5) + Silero VAD, utterance-level   (was: true streaming)
               sherpa-rs/parakeet-rs if low-latency ever needed
Storage        markdown = truth  +  DERIVED SQLite FTS5 index    (was: markdown only)
Calendar       EventKit (macOS, no OAuth)  →  Graph / .ics later (was: Google + Outlook OAuth in v1)
Analysis       claude -p --output-format json  |  Ollama  |  none  (was: Claude Code only)
Tickets        markdown + delegate push to Claude Code's own MCP/gh   (was: 3 native sync engines)
MCP            rmcp, SEPARATE binary over corpus                 (was: in-app)
Diarization    2-channel now; sherpa-rs speaker embeddings later  (was: "pyannote-style" — Python, wrong lane)
```

---

## 7. Unresolved questions

1. Headphones mandatory, or build echo dedupe? — affects D4, user-visible.
2. macOS floor: 14.4 (clean tap API) or 13 (needs ScreenCaptureKit path too)?
3. Windows at launch, or accept v1.1? — biggest scope lever left.
4. Ollama fallback in v1, or Claude-Code-only and accept the hard dependency?
5. Derived SQLite index acceptable, or is markdown-only ideological?
6. Which tracker do *you* actually use? — only that one deserves native sync.
7. Distributing publicly, or personal-only? — decides whether notarization + Google verification ever matter.
8. Kill date if Phase 0 drift isn't solved?
