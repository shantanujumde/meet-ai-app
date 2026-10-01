# Botless Meeting Assistant — Grounded Review (v2)

**Date:** 2026-09-01
**Input:** [`discovery-v1.md`](./history/discovery-v1.md) (Discovery v1, then the root `Readme.md`, decisions "locked")
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

---

## 8. Phase 0a — macOS audio-capture permission spike (measured, 2026-09-27)

**Ticket:** TUR-3 · **Code:** [`spikes/phase0a-tcc/`](../spikes/phase0a-tcc/) · **Verdict: PASS on all three parts.**

This section supersedes the TCC claim in §2 D1 ("a Swift sidecar does not reliably
inherit the app's TCC permissions"). That claim was inferred from open Tauri
issues. It is wrong on macOS 27 for a helper that lives inside the app bundle and
is launched as a child of the app, and the `tccd` logs below say so directly.

### Test environment

| | |
|---|---|
| macOS | **27.0**, build `26A428`, arm64 |
| Swift | 6.4 (`swiftlang-6.4.0.34.1`), Command Line Tools only — **no Xcode** |
| Bundle | hand-assembled `meet-ai.app`, bundle ID `pro.saleschat.meetai`, `LSUIElement`, `LSMinimumSystemVersion 14.4` |
| Info.plist | `NSAudioCaptureUsageDescription` + `NSMicrophoneUsageDescription` in the app plist, and in the helper via `-sectcreate __TEXT __info_plist` |
| Entitlements | `com.apple.security.device.audio-input` |
| Signing | **ad-hoc (`codesign -s -`) with `--options runtime`**, entitlements on both the app bundle and the helper. The machine has **zero** code-signing identities and creating a trusted self-signed one needs an admin password, which a headless run cannot supply. See "Signing caveat" below |
| Launch | `open -a meet-ai.app` → LaunchServices → app has `ppid=1`, helper is its child |
| Capture API | `CATapDescription(stereoGlobalTapButExcludeProcesses: [])` → `AudioHardwareCreateProcessTap` → private aggregate device → `AudioDeviceCreateIOProcIDWithBlock` |
| Audio source | synthetic tone played by `/usr/bin/afplay` as a **separate process**: L = 440 Hz, R = 660 Hz, amplitude 0.5 |

### 1. Does the permission prompt appear? — **YES**

```
13:06:36.595  tccd  AUTHREQ_PROMPTING: service=kTCCServiceAudioCapture,
              subject=Sub:{/private/tmp/…/meet-ai.app/Contents/MacOS/meet-ai}
13:06:39.656  tccd  Publishing <TCCDEvent: type=Create,
              service=kTCCServiceAudioCapture, identifier_type=Path,
              identifier=/private/tmp/…/meet-ai.app/Contents/MacOS/meet-ai>
```

Observed on three separate first-run bundles. The microphone prompt
(`kTCCServiceMicrophone`) appears the same way and is a separate grant.

### 2. Does it name *meet-ai* rather than the helper? — **YES**

The helper `meet-tap-probe` (pid 87119) makes the Core Audio call. `coreaudiod`
asks TCC about it, and TCC resolves it to the **app** as responsible process:

```
AUTHREQ_ATTRIBUTION: attribution={
    responsible={TCCDProcess: identifier=pro.saleschat.meetai,           pid=4974,
                 responsible_path=…/meet-ai.app/Contents/MacOS/meet-ai},
    accessing   ={TCCDProcess: identifier=pro.saleschat.meetai.tap-probe, pid=4977,
                 binary_path=…/meet-ai.app/Contents/MacOS/meet-tap-probe},
    requesting  ={TCCDProcess: identifier=com.apple.tccd, pid=412}}
```

The helper's own identity (`pro.saleschat.meetai.tap-probe`) is the *accessing*
process and never becomes the subject. One grant covers the app and every child
it spawns. `coreaudiod` (pid 617) asks the same question and gets the same
answer.

⚠️ **Correction to an earlier draft of this section.** The first four runs were
made with a `build.sh` that aborted between its two `codesign` calls (empty bash
array under `set -u`), so the **app bundle was never explicitly signed** — only
the helper was, and the app carried just the linker's automatic ad-hoc signature
with no hardened runtime and no entitlements. All three answers were unchanged
when re-run against a correctly signed bundle (13:13:31, RMS 0.353543 / −9.03
dBFS, peak 0.515868, 719 872 frames over 14 986.7 ms, prompt blocked
`AudioDeviceCreateIOProcIDWithBlock` for 2 198 ms), but the attribution got
cleaner: with the bundle properly signed TCC uses the **bundle identifier**
(`pro.saleschat.meetai`) as the identity instead of the executable's file name
(`meet-ai`). The block above is from the correctly-signed run. The script is
fixed; the failure mode is worth remembering because a half-signed bundle
verified fine at a glance.

*Not verified:* the literal wording rendered in the dialog. The evidence above is
the daemon's attribution, not a screenshot.

### 3. Does non-silent audio actually arrive? — **YES**

25-second capture, tone playing (`/tmp/meet-ai-phase0a-run4`):

| Measure | Value |
|---|---|
| Format | 48 000 Hz, 2 ch, float32 (`kAudioTapPropertyFormat`) |
| Frames / IO callbacks | 1 200 640 / 2 345 |
| Span (first → last buffer host time) | 25 002.68 ms |
| RMS | **0.346548 (−9.20 dBFS)** |
| Peak | 0.619006 (−4.17 dBFS) |
| Bit-exact-zero samples | 1 of 2 401 280 (fraction 8.3 × 10⁻⁷) |
| Per-second RMS | 0.022 (tone fading in), then 0.339–0.356 for all 24 remaining buckets |

Verified **independently of the probe's own meter**, by Goertzel analysis of a
1-second slice of the WAV:

```
L rms=0.35355   L @ 440Hz = 0.50000   L @ 660Hz = 0.00000   L @ 1000Hz = 0.00000
R rms=0.35355   R @ 660Hz = 0.50000   R @ 440Hz = 0.00000   R @ 1000Hz = 0.00000
```

Exact recovery of amplitude 0.5 at the right frequency in the right channel, zero
energy elsewhere, stereo separation intact. Theoretical RMS of a 0.5 sine is
0.5/√2 = 0.35355 — matched to five decimals.

**Control (`--no-tone`, nothing playing):** the same bundle produced a WAV whose
payload is **all zero bytes**, `zero_sample_fraction = 1.0`. So the tap reports
silence when there is silence and the tone when there is a tone. It is not
fabricating and it is not capturing something else.

### The important discovery: where the prompt blocks

`AudioHardwareCreateProcessTap` is **not** the gate. Measured on a first-run
bundle, alongside `tccd`'s prompt window of 3 061 ms:

| Call | Time |
|---|---|
| `AudioHardwareCreateProcessTap` | 3.05 ms → `noErr` |
| read `kAudioTapPropertyFormat` | 0.06 ms |
| `AudioHardwareCreateAggregateDevice` | 11.91 ms |
| **`AudioDeviceCreateIOProcIDWithBlock`** | **3 064.04 ms** → `noErr` |
| `AudioDeviceStart` | 27.84 ms |

Two consequences for Phase 0:

1. **Never infer permission from `AudioHardwareCreateProcessTap`.** It returns
   `noErr` in ~3 ms before the user has decided anything. This is the trap SPEC
   §5 warns about, now located precisely.
2. `AudioDeviceCreateIOProcIDWithBlock` is a **synchronous** gate — it blocks
   until the user answers. That is where the "waiting for permission" state
   belongs, and it means the shell can show real UI instead of guessing.
   ⚠️ It must be called off the UI thread, and it needs a timeout.

### Signing caveat — ad-hoc is not good enough for the real app

> ⚠️ **Superseded by §10.3–10.4 and §10.6.** The observations below are accurate
> *for an ad-hoc signed bundle*, but two of the conclusions they led to were
> wrong and one of the remedies is actively harmful. Read §10.6 before acting on
> anything in this subsection. Corrections are inline.

The spike is **ad-hoc signed**, so TCC stored the grant with
`identifier_type=Path` against the executable path. Consequences observed:

- `tccutil reset AudioCapture pro.saleschat.meetai` is a **no-op** — it targets a
  bundle-ID record that doesn't exist.
  **Correction (§10.4):** SPEC §6's recipe is correct as written; it was a no-op
  here *only* because the spike was ad-hoc signed. Under the real identity the
  grant is created with `identifier_type=Bundle ID` and `tccutil reset` works.
  ⛔ **Do not "copy the bundle to a new path to force a fresh prompt."** That was
  the original advice here and it is how three permanent, unreachable path-keyed
  TCC records were manufactured — `tccutil reset` resolves its argument through
  LaunchServices as a bundle ID and returns `-10814` for a path, and the record
  outlives the directory it names (§10.6). The advice is removed from SPEC §6.
- One rebuild produced `Failed to match existing code requirement for subject
  …/MacOS/meet-ai and service kTCCServiceAudioCapture` and re-prompted; another
  rebuild did not. Grant persistence across rebuilds was **inconsistent** across
  four runs, which is exactly what SPEC §2.9's "local self-signed identity, not
  optional" is there to prevent.
  **Confirmed as an ad-hoc artefact (§10.4, §10.7):** the ad-hoc designated
  requirement *is* the cdhash, so every rebuild is a new subject. Under the
  identity the requirement carries no hash, and a rebuild that changed the cdhash
  drew no prompt and no `Failed to match` line.

`make-identity.sh` creates the self-signed cert.
**Correction (§10.3):** it needs **no admin password**. The earlier claim that it
required one `sudo security add-trusted-cert` was a bug in the script — it passed
`-d` (system keychain). Without `-d` the trust setting lands in the user domain,
`codesign` accepts it, and `sudo` leaves the setup entirely. `build.sh` now
defaults to the identity and refuses to ad-hoc sign unless `ALLOW_ADHOC=1`.

### Also measured, free of charge

- **Microphone in the same bundle works.** `AVCaptureDevice.requestAccess(for:
  .audio)` from the *helper* prompted, attributed to meet-ai, blocked 3 s, then
  captured real room audio: 480 000 frames, peak 0.2645 (−11.55 dBFS), 100 buffers
  at 48 kHz mono. Dual-track capture needs two grants, both named meet-ai.
- **Crash safety holds.** `kill -9` on the helper 8 s into a 20 s capture left a
  valid, playable 8.021 s WAV (`ffprobe` clean, RMS 0.3306, peak 0.5000). The
  1-second header-patch cadence is enough; SPEC §2.3's 5 s is also fine.
- **`kAudioAggregateDeviceTapAutoStartKey` must be `false`.** With it on,
  `AudioDeviceStart` blocks until some tapped process produces audio — i.e. a
  meeting recorder would not start recording until someone spoke, and the first
  buffer's host time would no longer mark the true start of the segment. That
  breaks `segments.json` (SPEC §3.4) before it is even written.

### Still unverified

- ~~What the API does when permission is *denied*.~~ **Answered in §10.1** — it
  succeeds and delivers digital silence; every `OSStatus` is `noErr`. Neither a
  return-code check nor an RMS floor can detect it. §10.2 has what to do instead.
- ~~The dialog's literal text.~~ **Answered in §10.5**, from the system string
  table rather than a screenshot.
- ~~Behaviour under a real (non-ad-hoc) signing identity.~~ **Answered in
  §10.3–10.4** — the grant becomes bundle-ID-keyed and the designated requirement
  stops containing a cdhash, so rebuilds no longer break it. Creating the identity
  needs no admin password.
- ~~Observing the `identifier_type=Bundle ID` grant-*creation* event, and a
  user's explicit **Don't Allow**.~~ **Answered in §10.7** — both measured, with
  no human at the keyboard: the consent dialog turned out to be an ordinary
  Accessibility window. `AUTO_CLICK=1 scripts/signing/verify-tur10.sh`
  reproduces the whole thing unattended.
- macOS 14.4–26. Everything here is macOS 27.0 only.

---

## 9. Decision — capture layer is **pure Rust, in-process** (2026-09-27)

Resolves L3, which SPEC §1 explicitly delegated to this spike.

SPEC §5 pre-committed to "pass → capture lives in the sidecar". **I am not taking
that branch**, and this is the divergence to argue with rather than a silent
re-design. The reasoning:

The spike was meant to decide Swift-vs-Rust on TCC grounds. It removed TCC from
the argument entirely — a bundled helper inherits the app's TCC identity cleanly
(§8.2 above). So the decision falls back to ordinary engineering cost, and there
the evidence points the other way:

1. **Nothing in the capture path needs Swift.** Everything measured above is the
   C Core Audio API — `AudioHardwareCreateProcessTap`,
   `AudioObjectGetPropertyData`, `AudioHardwareCreateAggregateDevice`,
   `AudioDeviceCreateIOProcIDWithBlock`. The single Objective-C object is
   `CATapDescription`, and `objc2-core-audio` already binds it. Contrast
   `SpeechTranscriber` (L4/A2), which is Swift-concurrency-native and genuinely
   unreachable from Rust — that is what the sidecar is *for*.
2. **The Phase 0 gate is drift < 200 ms across two tracks.** Mic capture is
   `cpal`, in Rust. Putting the tap in a separate Swift process means two
   processes, two clocks and a pipe between them, on the single hardest exit gate
   in the project. Same-process gives both tracks one host clock and one writer
   for `segments.json` (SPEC §3.4).
3. **A sidecar boundary is free for transcription and expensive for capture.**
   `meet-stt` is stateless: WAV path in, JSON lines out. Capture is a long-lived
   real-time component sharing a ring buffer, device-change listeners, resamplers
   and incremental WAV writers with the rest of `crates/audio`. Every audio frame
   would have to cross a pipe, or the writers get duplicated in Swift.
4. **The Windows seam only works in Rust.** SPEC §8.2 makes `AudioSource` + the
   `stub-audio` `--target x86_64-pc-windows-msvc` cross-check part of `just check`
   from day one, and ⛔-marks "OS-specific code only in `crates/audio/src/macos/`".
   A Swift capture sidecar leaves that seam untested and unported.

**Cost of this choice:** porting ~250 lines of working Swift to `objc2-core-audio`.
That is the price, it is small, and the spike bundle is both the reference
implementation and the test oracle for the port — the Rust version must reproduce
the same numbers against the same tone.

**Rejected middle ground:** Swift sidecar for the tap plus Rust for the mic. It
takes the process boundary exactly where it hurts (drift) and keeps two languages
in the 🔴 module.

**Net effect on SPEC:** L3 resolves to in-process Rust, as §2.3 and §6's original
stack line already assumed. `crates/audio` stays 🔴 rather than dropping to 🟡 —
the ~1 week §5 hoped to save is not available. `sidecar/meet-stt` keeps
transcription only, exactly as A2 scoped it.

### 9.1 What the TUR-10 follow-up added to this decision

§10 was not scoped to revisit L3, but one of its results bears on it directly.
Recorded here so the decision is judged on the complete evidence.

**One new argument for in-process, and it is not a small one.** §10.2 replaced
the permission check with a **positive control**: meet-ai plays a short known
tone from its own process, then confirms that tone comes back through the tap
before it will call permission granted. That is now the *only* way to tell a
denial from a quiet room — §10.1/§10.7 measured both the missing-usage-string
denial and a real **Don't Allow**, and in both cases every `OSStatus` is `noErr`
while the payload is bit-exact zeros, identical to a granted capture of a silent
Mac. So the check is mandatory, not a nicety.

In-process Rust makes that check one function: emit tone, read the ring buffer,
correlate, return a verdict. Across a sidecar boundary it becomes an IPC
handshake that has to start the tap, synchronise a tone with a different
process's clock, and stream the verdict back — on the startup path, before the
UI can enable recording. This argument did not exist when §9 was written; it
points the same way as the other four.

**Nothing in §10 points the other way.** The two remaining implementation
constraints it found — `AudioDeviceCreateIOProcIDWithBlock` blocking for exactly
the dialog's dwell time (1 255 ms measured against a 1 250 ms prompt), and
`authReason=2` meaning "the user answered" rather than "the user consented" —
cost the same in either language.

**Decision unchanged: pure Rust, in-process.** Still pending agreement, since it
diverges from SPEC §5.

---

## 10. Phase 0a follow-up — the denied path, and a real signing identity (measured, 2026-09-27)

**Ticket:** TUR-10 · **Code:** [`spikes/phase0a-tcc/`](../spikes/phase0a-tcc/) ·
Closes the first three items of §8 "Still unverified".

Everything below was measured on the same machine as §8 (macOS 27.0 `26A428`,
arm64), inside a 15-minute window, with the same synthetic tone
(L = 440 Hz, R = 660 Hz, amplitude 0.5, played by `/usr/bin/afplay` as a
separate process).

### 10.1 What the API does when permission is DENIED — **it lies**

**On denial every Core Audio call still returns `noErr`, the IOProc is created,
the device starts, and the callbacks fire at full rate delivering bit-exact
zeros.** There is no error anywhere in the return path.

Two 12-second captures, same tone playing through the same output device,
ten minutes apart:

| | **GRANTED** `authValue=2` | **DENIED** `authValue=0` |
|---|---|---|
| Bundle | `/tmp/meet-ai-fresh2-…` (ad-hoc, granted in TUR-3) | `…/nodesc/meet-ai.app` (identity-signed, see 10.2) |
| `create_tap_osstatus` | `0` | `0` |
| `create_process_tap_ms` | 4.147 | 3.745 |
| `create_aggregate_ms` | 13.441 | 11.615 |
| **`create_ioproc_ms`** | **2.373 → `noErr`** | **5.881 → `noErr`** |
| `device_start_ms` | 91.549 → `noErr` | 75.062 → `noErr` |
| Frames | 575 488 | 575 488 |
| IO callbacks | 1 124 | 1 124 |
| Span | 11 999.97 ms | 11 989.31 ms |
| **RMS** | **0.339042 (−9.39 dBFS)** | **0.0** |
| Peak | 0.499 999 88 | 0.0 |
| **`zero_sample_fraction`** | **0.077 461** | **1.0** |
| Payload, measured by `ffmpeg` | 4 235 395 non-zero bytes | **0 non-zero bytes** |
| The tone file being played, meanwhile | 6 098 646 non-zero bytes | 6 098 646 non-zero bytes |

The last row is the control that makes this conclusive: in the denied run the
app had `afplay` running (`run.log`: `afplay launched, pid=21587`) on a 16 s tone
file containing 6 098 646 non-zero bytes, and the tap returned 4 603 948 bytes of
which **not one** was non-zero.

`tccd` agrees, and it takes **1 millisecond** to do so — no dialog is drawn:

```
13:18:04.101  tccd  AUTHREQ_PROMPTING: msgID=617.4086, service=kTCCServiceAudioCapture,
              subject=Sub:{pro.saleschat.meetaiNODESC}Resp:{…/MacOS/meet-ai}
13:18:04.102  tccd  AUTHREQ_RESULT:    msgID=617.4086, authValue=0, authReason=8
```

against the granted run's

```
13:20:05.776  tccd  AUTHREQ_RESULT:    msgID=617.4106, authValue=2, authReason=2
```

`authValue`: `0` = denied, `1` = unknown, `2` = allowed. `authReason`: `2` = user
consent, `8` = missing usage string.

**Scope of this measurement — and its resolution.** The denial above was produced
by building the bundle **without `NSAudioCaptureUsageDescription`**
(`authReason=8`), which was at the time the only way to reach `authValue=0`
without a human clicking a dialog. That left the real case — a user pressing
**Don't Allow**, `authValue=0, authReason=2` — inferred rather than measured.

**It is now measured.** See §10.7: an explicit Don't Allow behaves identically,
down to the same all-zero payload. Nothing in §10.2 rests on inference any more.

### 10.2 Therefore: SPEC §8.1's denial path cannot be a return-code check — *or* an RMS floor

The ticket framed this as a choice between two strategies. **Both of them fail.**

1. **Return-code check — impossible.** Every `OSStatus` in the chain is `noErr`
   on the denied path. There is nothing to check.
2. **RMS floor on the first second — unsound.** Denied audio is
   `zero_sample_fraction = 1.0`. But so is a *granted* capture of a silent Mac:
   §8's `--no-tone` control produced an all-zero payload too. "Permission denied"
   and "nobody is talking yet" are bit-for-bit identical. An RMS floor would
   fire a false "permission denied" on every meeting that starts quietly, and
   would still be right by accident on a real denial — which is worse, because it
   looks like it works.

**Use a positive control instead — the only strategy that actually separates the
two cases.** At onboarding, and once at the start of each recording:

- meet-ai plays a **short known signal from its own process** (a ~200 ms tone, or
  the "recording started" chime the UI wants anyway). The process tap is global,
  so our own output is inside it.
- Capture for the duration of that signal and test for it — RMS above a floor is
  enough; Goertzel at the known frequency (as §8 used) is stricter and costs
  nothing.
- **Signal absent ⇒ permission is denied.** Signal present ⇒ granted, and we have
  also just proved the whole tap → WAV path end-to-end, which the return codes
  never proved.
- Only after that does the recording-started state become true. Until then the UI
  stays on "checking permission", and on failure goes to SPEC §8.1's denial screen
  (one sentence + **Retry** + **Open System Settings**).

Two supporting notes for the implementation:

- `zero_sample_fraction == 1.0` over a window is a useful *alarm*, not a verdict:
  necessary for denial, not sufficient. Log it; never show it to the user as
  "permission denied" on its own.
- The **microphone** half of onboarding probably does not need any of this:
  `AVCaptureDevice.authorizationStatus(for: .audio)` is a public, synchronous
  status API that documents a `.denied` case, which the tap has no equivalent of.
  ⚠️ Not measured here — §8 only exercised the mic's *granted* path, and this run
  did not touch `kTCCServiceMicrophone` at all. Confirm it before relying on it;
  after §10.1 the null hypothesis for any macOS permission API is that it lies.
  ✅ **Measured 2026-10-01** on a signed bundle: after
  `tccutil reset Microphone pro.saleschat.meetai` and "Don't Allow" on the
  prompt, `authorizationStatus` returned `.denied` and onboarding showed it.
  After switching meet-ai on in System Settings, Check again
  read granted. The status API tells the truth for the mic. The same reset
  printed 7 success lines, so this machine now has 7 registered copies of the
  bundle ID, up from §11.1's 4.

⛔ **And never ship without `NSAudioCaptureUsageDescription`.** 10.1's denial
*is* that bug: the key was missing, so macOS denied in 1 ms, drew no dialog, told
the app nothing, and let it record an hour of silence. That is exactly the
failure mode SPEC §5 calls a fail rather than a pass.

### 10.3 A real signing identity needs **no admin password** — the ticket's premise was wrong

`make-identity.sh` tried `security add-trusted-cert -d … -k /Library/Keychains/System.keychain`
first, which is an admin-domain change and goes through SecurityAgent. Dropping
the `-d` writes the trust setting to the **user** domain instead, which is enough
for `codesign` and needs no password at all:

```
$ security add-trusted-cert -r trustRoot -p codeSign -k ~/Library/Keychains/meet-ai-signing.keychain-db cert.pem
$ security find-identity -v -p codesigning
  1) BE3FB2C8C0CE4AC08348A09F0BF278094626E347 "meet-ai Local Signing"
     1 valid identities found
```

Seven seconds, non-interactive, exit 0. `make-identity.sh` now tries the user
domain first and keeps `-d` only as a fallback. **`sudo` is no longer part of the
setup**, and SETUP.md Step 5's "create it by hand in Keychain Access" is replaced
by running the script.

### 10.4 Under that identity TCC keys the grant to the **bundle ID**, not the path

This is the §8 signing caveat, resolved. The mechanism is the designated
requirement, and it is visible directly:

```
ad-hoc:    designated => cdhash H"8a42ac53dc5371fb8a54fb8078042ee0bd790d8c"
identity:  designated => identifier "pro.saleschat.meetai" and
                         certificate leaf = H"be3fb2c8c0ce4ac08348a09f0bf278094626e347"
```

The ad-hoc requirement **is** the binary hash, so every rebuild that changes a
single instruction breaks it — that is §8's "Failed to match existing code
requirement". The identity requirement contains no hash at all: bundle ID plus
certificate. Rebuilding cannot invalidate it.

`tccd` keys its records the same way. Same machine, twelve minutes apart:

```
ad-hoc bundle    AUTHREQ_SUBJECT: subject=/private/tmp/meet-ai-fresh2-…/Contents/MacOS/meet-ai
identity bundle  AUTHREQ_SUBJECT: subject=pro.saleschat.meetaiNODESC
```

A **path** for ad-hoc, a **bundle ID** for the signed build. So
`tccutil reset AudioCapture pro.saleschat.meetai` addresses a record that now
exists, and SPEC §6's Phase 0a recipe is correct as written — provided the
bundle is signed with an identity. It was a no-op in §8 only because the spike
was ad-hoc.

**Confirmed** — the grant-*creation* event was captured in §10.7, and it is
bundle-ID keyed:

```
Publishing <TCCDEvent: type=Create, service=kTCCServiceAudioCapture,
            identifier_type=Bundle ID, identifier=pro.saleschat.meetai>
```

### 10.5 The literal prompt text — from the system, not a screenshot

`/System/Library/PrivateFrameworks/TCC.framework/Resources/Localizable.loctable`
is the source of truth for what the user reads. English:

| Key | Text |
|---|---|
| `REQUEST_ACCESS_SERVICE_kTCCServiceAudioCapture` | **“%@” would like access to record your system audio.** |
| `REQUEST_ACCESS_SERVICE_kTCCServiceMicrophone` | **Allow “%@” to access your microphone?** |
| `REQUEST_ACCESS_ALLOW` | **Allow** |
| `REQUEST_ACCESS_DENY` / `REQUEST_ACCESS_DONT_ALLOW` | **Don’t Allow** |

`%@` is the app's display name, i.e. **meet-ai** (§8.2 already proved the
attribution resolves to the app, not the helper). So the dialog reads:

> **“meet-ai” would like access to record your system audio.**
> meet-ai records the audio of your meetings so it can transcribe them on this
> Mac. Nothing is uploaded.
> \[ Don't Allow ] \[ Allow ]

The second line is `NSAudioCaptureUsageDescription`, verbatim. There is **no**
`REQUEST_DEFAULT_PURPOSE_STRING_SERVICE_kTCCServiceAudioCapture` key in the
table — unlike Bluetooth and App Data, this service has no fallback sentence.
Our string is the *entire* explanation the user gets, which is one more reason
§10.2's ⛔ matters: omit it and macOS does not fall back, it denies.

### 10.6 Path-keyed TCC records are **permanent**, and that makes ad-hoc signing a one-way door

Prompted by Tess's read-only `tccutil list` on TUR-2, which found four TCC
identities for one app: three absolute paths and one bundle ID. §10.4 explains
why they exist. The new question is whether they can be cleaned up. They cannot.

`tccutil reset` resolves its argument through LaunchServices **as a bundle
identifier**. Give it a path and it fails identically to a bundle ID that was
never installed:

```
$ tccutil reset AudioCapture "/private/tmp/meet-ai-fresh-1790494489/meet-ai.app/Contents/MacOS/meet-ai"
tccutil: No such bundle identifier "…": OSStatus error -10814.   # exit 64

$ tccutil reset AudioCapture "pro.saleschat.definitelynotreal"
tccutil: No such bundle identifier "…": OSStatus error -10814.   # exit 64
```

Same error for the `.app` path as for the executable path. Deleting the
directory does not help either — I removed both `/private/tmp/meet-ai-fresh*`
trees and re-listed:

```
$ rm -rf /private/tmp/meet-ai-fresh-1790494489 /private/tmp/meet-ai-fresh2-1790494596
$ tccutil list -s kTCCServiceAudioCapture | grep -i meet
/Users/shantanujumde/apps/meet-ai/spikes/phase0a-tcc/build/meet-ai.app/Contents/MacOS/meet-ai
/private/tmp/meet-ai-fresh-1790494489/meet-ai.app/Contents/MacOS/meet-ai     # still there
/private/tmp/meet-ai-fresh2-1790494596/meet-ai.app/Contents/MacOS/meet-ai    # still there
pro.saleschat.meetai
```

So a path record outlives both the tool that can reset bundle IDs and the file
it points at. Clearing one needs System Settings by hand, or a direct `TCC.db`
write behind Full Disk Access.

**Consequence: every ad-hoc build in a fresh directory permanently dirties the
machine's permission baseline.** That is a correctness problem for gate testing,
not just untidiness — a re-prompt against a moving identity is unattributable
(correct re-ask after a denial, or macOS simply not recognising the binary?).

Three things changed to close the door:

- **`build.sh` refuses to ad-hoc sign.** It now defaults `SIGN_IDENTITY` to the
  local identity and locates the keychain itself, so the normal invocation is a
  bare `./build.sh`. With no identity it exits 1 and explains why;
  `ALLOW_ADHOC=1` is the deliberate override.
- **`make-identity.sh` is idempotent.** It used to mint a fresh cert *and*
  `delete-keychain` on every run — which rotated the leaf, and with it every
  grant. It now reuses the existing identity and prints its leaf SHA-1;
  `--rotate` is required to replace it and warns first. The cert also moved out
  of `$TMPDIR` (macOS purges it; under Paperclip it is per-run, so the PEM
  vanished within a day while the keychain survived) to `~/.meet-ai/signing`,
  and the private key is no longer left on disk at all.
- **`verify-tur10.sh` asserts the baseline before the first prompt.** It
  checks the leaf against a recorded `EXPECT_LEAF`, re-checks it against the
  built bundle after signing, aborts on mismatch, and snapshots
  `tccutil list` for **both** services into the report before and after.

Two smaller things fixed along the way, both measured:

- `security find-identity -v -p codesigning` with no keychain argument reports
  **"0 valid identities"** under a sandboxed `$HOME`, for an identity that is
  present and working — it reads the user keychain search list, which is empty
  there. Always pass the keychain explicitly.
- `security list-keychains -d user -s $CURRENT "$KEYCHAIN"` **replaces** the
  search list. In that same sandboxed case `$CURRENT` is empty, so the old
  script would have wiped the login keychain out of the user's search path.
  Guarded.

Also worth recording for a first-run test: `kTCCServiceAudioCapture` and
`kTCCServiceMicrophone` are tracked independently and were observed in
*different* states — the main bundle held AudioCapture but not Microphone. Reset
both, or only one dialog appears and the run looks half-broken.

### 10.7 The two "needs a human" facts, measured — the consent dialog **is** scriptable

§10.1 and §10.4 each ended with a fact that supposedly needed a person at the
keyboard: an explicit **Don't Allow** (`authReason=2`), and the grant-*creation*
event that TCC only writes when someone clicks **Allow**. Both are now measured.

**The blocker was wrong, not insurmountable.** §10.1 recorded that "this machine
grants no Accessibility to the automation shell". Re-testing it is one command,
and it now succeeds:

```
$ osascript -e 'tell application "System Events" to return count of every process'
105
```

With Accessibility available, the consent dialog is an ordinary AX window. It is
drawn by **`UserNotificationCenter`**, and `spikes/phase0a-tcc/auto-click.sh`
answers it:

```
CLICKED:UserNotificationCenter:Allow
CLICKED:UserNotificationCenter:Don’t Allow
```

So `verify-tur10.sh` now runs unattended under `AUTO_CLICK=1`. Two full runs,
~90 seconds apart, agree on every line below.

#### The measurement

| | GRANTED (clicked Allow) | REBUILD (not asked) | DENIED (clicked Don't Allow) |
|---|---|---|---|
| `tccd` verdict | `authValue=2, authReason=2` | *no prompt* | **`authValue=0, authReason=2`** |
| `create_tap_osstatus` | 0 | 0 | **0** |
| `create_ioproc_ms` | 1553.148 → `noErr` | 4.616 → `noErr` | **1255.184 → `noErr`** |
| `device_start_ms` | 30.659 → `noErr` | 11.065 → `noErr` | **15.852 → `noErr`** |
| frames / io_callbacks | 576 512 / 1 126 | 576 000 / 1 125 | **576 512 / 1 126** |
| `rms` | 0.361263 | 0.346509 | **0.0** |
| `peak` | 0.932076 | 0.941738 | **0.0** |
| `zero_sample_fraction` | 1.73e-06 | 1.74e-06 | **1.0** |
| non-zero payload bytes | 4 585 557 | 4 582 194 | **0** |

**An explicit Don't Allow is indistinguishable from the `authReason=8` denial in
§10.1.** Every `OSStatus` is `noErr`, the IOProc is created, the device starts,
1 126 callbacks fire at the normal rate — and every sample is a bit-exact zero.
§10.2's positive-control design is therefore confirmed against the real case,
not just the one that was convenient to produce.

Two details the correlation turned up:

- **`create_ioproc_ms` is the dialog's dwell time.** `tccd` logged the prompt
  standing open for 1 250 ms; `AudioDeviceCreateIOProcIDWithBlock` returned in
  1 255.184 ms. On the rebuild, with no prompt, it returned in 4.616 ms. So that
  call blocks for exactly as long as the user hesitates — it is not a timeout,
  and onboarding must not treat a slow return as failure.
- **`authReason=2` means "the user answered", not "the user consented."** It
  appears on Allow and on Don't Allow alike; `authValue` (`0` denied / `2`
  allowed) is the bit that differs. Reading `authReason` alone inverts the
  result.

#### Grant creation and survival

The `type=Create` event, which only exists once someone has clicked Allow:

```
Publishing <TCCDEvent: type=Create, service=kTCCServiceAudioCapture,
            identifier_type=Bundle ID, identifier=pro.saleschat.meetai>
```

`identifier_type=Bundle ID` — §10.4 confirmed, and **SPEC §6's
`tccutil reset AudioCapture pro.saleschat.meetai` recipe is valid as written**.
The rebuild step then changed the binary (and its cdhash) under the same
identity and drew **no prompt and no `Failed to match existing code
requirement`**: the cdhash-free designated requirement holds, so §8's
intermittent grant loss was purely an ad-hoc artefact.

#### Reading the log: don't grep for your own name

The obvious `log show … | grep meetai` **silently discards the answer**. `tccd`
splits a consent across two lines: `AUTHREQ_PROMPTING` carries the subject and a
msgID, and the `AUTHREQ_RESULT` recording what was clicked carries *only* the
msgID. Filtering by bundle id keeps the question and drops the answer — which is
why an earlier run of `verify-tur10.sh` reported "(no AUTHREQ_RESULT captured)"
for a denial it had in fact captured correctly. Correlate by msgID instead;
`decision_for()` in that script does.

#### Safety note on the clicker

`auto-click.sh` runs on a live desktop, so it refuses to click unless the window
mentions `meet-ai`, has both an affirmative and a negative button, and belongs to
an allowlisted system process. It also matches the deny button by its
apostrophe: the real label is **“Don’t Allow”** with U+2019, so a naive
`contains "Allow"` matches the deny button too and a clicker built that way
would silently deny when told to allow. Verified against a decoy dialog that
does not mention us — left untouched, timed out, exit non-zero.

## 11. TUR-128 — why manual gate testing sees duplicate/stale "meet-ai" TCC rows (measured, 2026-09-28)

Spun out of Tess's TUR-127 reproduction: System Settings showed two rows named
"meet-ai" under Microphone, `tccutil reset AudioCapture pro.saleschat.meetai`
reported clearing 4 entries in one call, and the AudioCapture list showed three
"meet-ai" rows defaulted to **ON** with no prompt. The code fix in TUR-127 reads
`AVCaptureDevice.authorizationStatus` from inside the running process, so it is
correct regardless of how many stale TCC rows exist — but a tester toggling the
wrong row in System Settings gets a confusing, unattributable result. This
section is the root cause and a recipe that survives it.

### 11.1 Root cause A — reproduced exactly: 4 registered copies of the bundle ID, not all on the current cert

`tccutil reset` printed **4** identical success lines on this machine, live,
reproducing Tess's number exactly:

```
$ tccutil reset AudioCapture pro.saleschat.meetai
Successfully reset AudioCapture approval status for pro.saleschat.meetai
Successfully reset AudioCapture approval status for pro.saleschat.meetai
Successfully reset AudioCapture approval status for pro.saleschat.meetai
Successfully reset AudioCapture approval status for pro.saleschat.meetai
```

`tccutil reset <service> <bundle-id>` resolves the bundle ID through
LaunchServices to *every* registered app matching it, not to a single TCC row,
and prints one line per resolved instance. This machine has four:

```
$ mdfind "kMDItemCFBundleIdentifier == 'pro.saleschat.meetai'"
/Users/…/apps/meet-ai/target/release/bundle/macos/meet-ai.app
/Users/…/apps/meet-ai/spikes/phase0a-tcc/build/meet-ai.app
/Users/…/Downloads/meet-ai.app
/Users/…/Applications/meet-ai.app
```

Three build outputs plus a stray copy in Downloads and an installed copy in
Applications, all sharing the bundle ID `pro.saleschat.meetai` because that
identifier is deliberately fixed (SETUP.md Step 1: "never changes"). That part
is by design and harmless *if* they're all signed with the same identity — but
they aren't:

```
$ codesign -d -r- target/release/bundle/macos/meet-ai.app | tail -1
designated => identifier "pro.saleschat.meetai" and certificate leaf = H"eafb73d29b2f35ca25c2f9fd193869fd880a7e0d"
$ codesign -d -r- spikes/phase0a-tcc/build/meet-ai.app | tail -1
designated => identifier "pro.saleschat.meetai" and certificate leaf = H"be3fb2c8c0ce4ac08348a09f0bf278094626e347"   # ← different
$ codesign -d -r- ~/Downloads/meet-ai.app | tail -1
designated => identifier "pro.saleschat.meetai" and certificate leaf = H"eafb73d29b2f35ca25c2f9fd193869fd880a7e0d"
$ codesign -d -r- ~/Applications/meet-ai.app | tail -1
designated => identifier "pro.saleschat.meetai" and certificate leaf = H"eafb73d29b2f35ca25c2f9fd193869fd880a7e0d"
```

**This is the ticket's hypothesis, confirmed exactly.** `be3fb2c8…` is the
*first* identity this project ever minted (§10.3's own worked example uses that
same leaf). At some point between §10.3 and §10.4 the local identity was
regenerated — a deleted-and-recreated keychain, or an early `--rotate` before
the idempotency guard existed — and the current identity became `eafb73d2…`,
the value SETUP.md documents today. The spike build directory was never
rebuilt after that rotation, so it is still signed against the old leaf: same
`CFBundleIdentifier` string, same bundle ID argument to `tccutil`, but a
genuinely different designated requirement and therefore a genuinely different
TCC client. Rebuilding it (`spikes/phase0a-tcc/build.sh`) fixes that one
instance instantly, because `make-identity.sh` only ever hands out the current
identity — the old leaf isn't secretly still around, it's baked into a binary
nobody rebuilt.

So: **the identity-minting process is not actively drifting** (§10.3's
idempotency guard works, and this machine has exactly one identity in its
keychain right now), **but every build output signed before the one historical
rotation is a permanently orphaned TCC client that a plain `tccutil reset` will
enumerate and "clear" without ever making it match the current app again** —
because there is nothing to match; it needs rebuilding, not resetting.

### 11.2 Root cause B — path-keyed rows from ad-hoc builds never die, and share the same display name

§10.6 already proved path-keyed TCC rows are permanent (`tccutil reset` errors
`-10814` on a path; deleting the build directory does not help). That is live
on this machine too, and independently produces "extra meet-ai rows":

```
$ tccutil list -s kTCCServiceMicrophone | grep -i meet
/Users/…/spikes/phase0a-tcc/build/meet-ai.app/Contents/MacOS/meet-ai   # path-keyed, stale
pro.saleschat.meetai                                                   # bundle-ID-keyed, current

$ tccutil list -s kTCCServiceAudioCapture | grep -i meet
/Users/…/spikes/phase0a-tcc/build/meet-ai.app/Contents/MacOS/meet-ai
/private/tmp/meet-ai-fresh-1790494489/meet-ai.app/Contents/MacOS/meet-ai
/private/tmp/meet-ai-fresh2-1790494596/meet-ai.app/Contents/MacOS/meet-ai
```

These predate the TUR-10 signing fix and are unreachable by `tccutil reset` by
design (§10.6) — System Settings still shows them as "meet-ai" because the
build directories they point at still exist on disk, so a tester comparing
before/after a reset can see rows that still say "meet-ai" and still say "On"
and reasonably (but wrongly) read that as the reset having failed, or as a
fresh unprompted grant. They are neither — they were granted once, long ago,
during earlier ad-hoc testing, and nothing since has been able to touch them.

### 11.3 Root cause C — the phase0a spike bundle also has a *second, real* bundle ID hiding under the same display name

`spikes/phase0a-tcc/build.sh` builds and signs **two** distinct TCC clients
into one `.app`:

```
$ grep CFBundleIdentifier -A1 spikes/phase0a-tcc/Info-app.plist spikes/phase0a-tcc/Info-helper.plist
Info-app.plist:     pro.saleschat.meetai            (CFBundleDisplayName: meet-ai)
Info-helper.plist:  pro.saleschat.meetai.tap-probe  (CFBundleDisplayName: meet-ai)
```

`meet-tap-probe` is compiled with its own `__TEXT,__info_plist` and its own
`NSAudioCaptureUsageDescription`/`NSMicrophoneUsageDescription`, so it can (and
during Phase 0a testing, does) independently trigger its own TCC prompt and get
its own row — genuinely a different app to `tccd`, genuinely displayed as
"meet-ai" (same `CFBundleDisplayName`) to a human. This is fine for the spike's
purpose (isolating the tap from the app was the point), but it means **testing
the TUR-95/TUR-115 gate against `spikes/phase0a-tcc/build/meet-ai.app` instead
of the real Tauri bundle structurally guarantees a second "meet-ai" identity**.
The real app (`src-tauri/tauri.conf.json` → single `identifier: pro.saleschat.meetai`,
confirmed — no second bundle ID anywhere else in the repo) does not have this
problem. The gate must run against `target/release/bundle/macos/meet-ai.app`
(via `just bundle-signed`), never the spike bundle.

### 11.4 A justfile bug found in the course of this, fixed

While confirming the identity resolves consistently across environments,
`just sign`'s default `SIGN_KEYCHAIN` (`env_var("HOME") + "/Library/Keychains/…"`)
turned out to use raw `$HOME` rather than the account home — the exact trap
`make-identity.sh` and `build.sh` were already hardened against (TUR-10,
§10.3). Under any session where `$HOME` is redirected (a Paperclip agent run
is one; observed live during this investigation), the computed path does not
exist, `just sign` silently falls through to a bare `-s "meet-ai Local Signing"`
with no `--keychain`, and the default keychain search list does not contain the
identity either:

```
$ echo $HOME
/var/folders/…/paperclip-run-tur-73-…          # not /Users/shantanujumde
$ security find-identity -v -p codesigning      # no --keychain, default search list
     0 valid identities found
```

That makes `codesign` fail outright rather than drift to a different identity
— not silent duplication, but it does mean `just bundle-signed` cannot be
trusted to run unattended in exactly the kind of environment a future
automated gate check would use. Fixed by giving the `sign` recipe the same
`dscl`-based real-home fallback `build.sh` already has; re-verified afterward
on this machine with `$HOME` still redirected:

```
$ just sign
==> sealing bundle: target/release/bundle/macos/meet-ai.app
$ codesign -d -r- target/release/bundle/macos/meet-ai.app
designated => identifier "pro.saleschat.meetai" and certificate leaf = H"eafb73d2…"
```

Identity-keyed, matching SETUP.md's recorded leaf, with `$HOME` still pointed
somewhere that doesn't exist.

### 11.5 The recipe — reset + re-test that survives all of the above

1. **Find every app on disk claiming the bundle ID, and check each one's cert leaf
   against the current identity — before touching TCC:**
   ```bash
   mdfind "kMDItemCFBundleIdentifier == 'pro.saleschat.meetai'"
   scripts/signing/make-identity.sh --print   # prints the current leaf

   for app in $(mdfind "kMDItemCFBundleIdentifier == 'pro.saleschat.meetai'"); do
     echo "$app"; codesign -d -r- "$app" 2>&1 | tail -1
   done
   ```
   Any result is one of the lines `tccutil reset` will print — §11.1 measured
   4 on this machine, not the 1 a fresh checkout would assume. **Any app whose
   leaf doesn't match the current identity is not the same TCC client as the
   one you're about to test**, no matter what its bundle ID string says.
   Rebuild it (or delete it if it's a stray copy you don't need — the
   Downloads/Applications copies in §11.1 were exactly that) before relying on
   a reset to put it back in a known state. Never point a gate run at
   `spikes/phase0a-tcc/build/meet-ai.app` for this reason and for §11.3's
   second-bundle-ID reason.

2. **Enumerate every row before resetting, not just the ones System Settings shows
   with a distinguishable name:**
   ```bash
   tccutil list -s kTCCServiceMicrophone   | grep -i meet
   tccutil list -s kTCCServiceAudioCapture | grep -i meet
   ```
   Anything that is a filesystem path rather than `pro.saleschat.meetai` is a
   stale, permanent, ad-hoc-era row (§11.2/§10.6). Reset cannot touch it and it
   will still be there after. Do not mistake its continued presence — or its
   "On" state — for a fresh, unprompted grant.

3. **Reset both services** — they are tracked independently and are commonly in
   different states (§10.6):
   ```bash
   tccutil reset AudioCapture pro.saleschat.meetai
   tccutil reset Microphone   pro.saleschat.meetai
   ```

4. **Re-list and diff against step 2.** Only the `pro.saleschat.meetai` line
   should have changed (disappeared). If a path-keyed line's state changed too,
   something else touched it — that's a real finding, not this recipe's
   pre-existing noise.

5. **Run the gate** (`open target/release/bundle/macos/meet-ai.app`, or the
   Record button per TUR-95) and confirm the prompt names **meet-ai** — if
   System Settings later shows more than one "meet-ai" row, use step 2's output
   to know which one is `pro.saleschat.meetai` before toggling anything.

6. **One-time cleanup for this machine:** the three AudioCapture path rows and
   one Microphone path row listed in §11.2 are permanently stuck and will keep
   making every future run of step 2 noisy. They can only be removed by hand
   (System Settings → Privacy & Security → Microphone / System Audio Recording
   Only → select the path-looking row → remove) or via `TCC.db` surgery behind
   Full Disk Access (§10.6) — `tccutil` cannot do it. Not done as part of this
   ticket since it's a live edit to the tester's permission database outside
   version control; flagged here so whoever runs the next gate knows why the
   rows are there and that removing them is safe (they belong to build
   directories from superseded ad-hoc spikes, not the shipped app).

## 12. v0.3.0 release smoke test — the shipped zip records for real (measured, 2026-09-30)

First recording made by the app itself rather than by `meet-rec`. Run against
the exact artifact users download: `gh release download v0.3.0`, checksum
checked (`shasum -a 256 -c` → OK), unzipped with `ditto -x -k`, copied to
`/Applications`, quarantine cleared, `codesign --verify --deep --strict` clean,
leaf `eafb73d2…`, `CFBundleShortVersionString` 0.3.0. Onboarding was already
complete on this machine from 0.2.0 runs, so the app opened straight to the
shell.

### 12.1 What was done

1. `open -a /Applications/meet-ai.app`.
2. ⌘⇧R sent with `osascript -e 'tell application "System Events" to keystroke "r" using {command down, shift down}'`.
   The global shortcut fires from a synthesized keystroke, so the Record path
   can be driven without touching the window.
3. The same sentence played twice through the speakers with `say -r 170 "…"`.
4. ⌘⇧R again to stop.

### 12.2 Result — **PASS** for the system track and the stop path

- `~/Meetings/2026-09-30-1129-meeting/` created on start with `notes.md`,
  `transcript.md` and `audio/`. `mic.wav` and `system.wav` grew while
  recording (~1.05 MB each after ~33 s, right for 16 kHz mono s16).
- Stop wrote `audio/segments.json` within a second: one segment, reason
  `start`, one anchor, 889 813 mic frames vs 889 852 system frames (55.6 s).
- `drift-check` on the folder: **PASS**, worst 40.0 ms (mic) / 18.6 ms
  (system) against the 200 ms gate; cross-track skew 21.4 ms.
- `system.wav` has sound exactly where `say` ran: 18.8–28.5 s and
  38.8–48.5 s, each ~9.7 s against a 9.95 s reference render of the same
  sentence, so no speed or rate error. Silent stretches are true digital zero.
- Apple's engine, through the production path (`registry::select` →
  `transcribe_meeting`), gives 5 lines for `Others`, both plays word for word.
  "meet-ai" comes out as "meat I" / "meat RE" — the same mishearing as on the
  clean reference render, so it is the engine, not the capture.

### 12.3 Not proven by this run

- **Mic content.** `mic.wav` holds steady room noise around −53 dBFS the whole
  time (live, not digital zero), and the speaker playback did not reach it.
  Nobody spoke, so a voice on the mic track is still unmeasured. One spoken
  sentence on the next run closes this.
- **The permission check and the denial path (TUR-78, TUR-127).** Onboarding
  was already done, and TCC was already granted, so neither ran. §11.5 is the
  recipe for the denial run.
- **Live transcript.** `transcript.md` stays 0 bytes after stop — expected
  until TUR-96 lands; the app does not transcribe yet.

### 12.4 A trap in the `offline_meeting` example — it wants the flat fixture layout

`cargo run -p stt --example offline_meeting -- transcribe --meeting <folder>`
copies `<folder>/mic.wav`, `<folder>/system.wav` and `<folder>/segments.json`,
which is the layout of `crates/audio/fixtures/*`. A meeting folder the app
writes keeps them in `<folder>/audio/`, so pointed at a real meeting the
example silently copies nothing and reports **"0 lines via apple-speech"**
with exit 0 — which reads like the recording is empty. It is not. Until the
example accepts both layouts, pass the `audio/` subfolder:

```sh
cargo run -p stt --example offline_meeting -- transcribe --engine apple \
    --meeting ~/Meetings/<meeting>/audio --scratch /tmp/meet-smoke
```

`meet-stt <wav>` on a single track is the quickest cross-check when a 0-line
result looks wrong.
