> [!WARNING]
> **SUPERSEDED — historical discovery document.**
> The implementation contract is **[`SPEC.md`](../../SPEC.md)** (Production Spec v2, locked 2026-09-01).
> Six of the twelve decisions below were reversed after research; see **[`docs/findings.md`](../findings.md)** for the evidence behind each reversal.
> Kept for the reasoning trail only. Do not implement from this file.
> To build and run the project, start at **[`CONTRIBUTING.md`](../../CONTRIBUTING.md)**.

# Botless Meeting Assistant — Design Doc (Discovery v1)

**Date:** 2026-09-01
**Status:** Discovery complete — decisions locked for v1
**One-liner:** A local-first, botless meeting recorder for developers that turns meetings into merged PRs — capture system audio, transcribe locally, analyze with the user's own Claude Code, and produce actionable tickets.

---

## 1. Product Vision

- **Target user:** Developers who live in Claude Code / Codex / Cursor.
- **Core loop:** Meeting happens → app records (no bot joins) → real-time local transcription → Claude Code + skill analyzes → tickets appear in-app (and optionally in Jira/Linear/GitHub/local MD) → memory connects future meetings to past ones.
- **Positioning:** Granola is "AI notes for meetings." We are **"meetings → merged PRs"** — the only notetaker that closes the loop into the developer's actual work system, fully local.
- **Privacy stance:** Audio never leaves the machine. Cloud is opt-in only via user-supplied API keys (BYOK). Personal app — consent/compliance out of scope for now.

---

## 2. Locked Decisions

| # | Decision | Choice | Rationale |
|---|----------|--------|-----------|
| D1 | Desktop framework | **Tauri 2.0 + Rust core, Swift helper for macOS capture** | Ecosystem consensus (Open Granola, Anarlog, Meetily are Tauri); ~45MB idle vs ~400MB Electron; app runs alongside resource-hungry meeting clients; Rust is the right layer for audio + whisper.cpp FFI; Tauri 2.0 keeps iOS/Android door open |
| D2 | Transcription strategy | **Local by default (whisper.cpp), BYOK cloud opt-in** | Privacy-first; cloud (OpenAI/Groq/Deepgram) only with user's own keys |
| D3 | Transcription timing | **Real-time streaming** | Live transcript in UI; VAD-chunked to avoid mid-sentence cuts |
| D4 | Audio capture | **Two separate channels: mic = "You", system loopback = "Others"** | Pattern proven by Muesli & Open Granola; free two-way diarization without ML. ML diarization of the Others channel (pyannote-style) deferred to v2 |
| D5 | Recording trigger | **Auto-detect meeting + notify; user manually confirms start** | Avoids surprise recordings; matches Granola's "click into the note" flow |
| D6 | Calendar | **Google + Outlook in v1** | Auto-title notes, pre-create meeting pages, 1-min-before reminders, attendee context |
| D7 | Analysis engine | **User's Claude Code CLI (with a bundled skill)** | Zero API cost to us; user already has it; skills = extension model. Pattern proven by `pasrom/meeting-transcriber` |
| D8 | Ticket destinations | **In-app ticket UI by default; opt-in sync to local MD / Jira / Linear / GitHub Issues** | In-app tickets are the USP surface; trackers are user's choice |
| D9 | Storage / memory | **Plain markdown files only (frontmatter + body). No database** | Greppable, Obsidian-compatible, agent-native. Tickets are MD files with frontmatter that the UI renders. Semantic/vector search deferred — Claude Code greps the corpus |
| D10 | v1 platforms | **macOS + Windows together** (Linux later, mobile optional/mic-only) | Matches user priority: mac > windows > linux > mobile |
| D11 | MCP server | **Ship in v1** | Query meetings from inside Claude Code/Cursor; turns the app into infrastructure |
| D12 | In-meeting UI | **Full workspace: live transcript, notes pane, tasks, memory recall, upcoming meetings** | The app is a meeting cockpit, not a background recorder |

---

## 3. Tech Stack

### App shell
- **Tauri 2.0** — window, tray, global hotkeys, notifications, IPC, auto-update
- **Frontend:** React + TypeScript + Tailwind (rendered in OS webview: WebKit on macOS, WebView2 on Windows)
- **Backend:** Rust — audio pipeline orchestration, model management, file I/O, MCP server

### Audio capture (per-OS)
- **macOS:** Swift helper binary — CoreAudio **process tap** (primary), **ScreenCaptureKit** fallback (macOS 13+). Requires Microphone + "Screen & System Audio Recording" permissions (macOS bundles system-audio access under screen recording)
- **Windows:** **WASAPI loopback** from Rust (comparatively easy; no special permission step)
- **Linux (later):** PipeWire/PulseAudio monitor sources
- Two simultaneous streams captured: mic (You) + system loopback (Others)
- Device hot-swap handling (AirPods/Bluetooth switches mid-meeting) — re-attach capture on default-device change

### Speech-to-text
- **Runtime:** whisper.cpp (bundled binary per platform, GGML models downloaded on first run — no Python)
- **Default model, Apple Silicon:** Whisper **Large v3 Turbo** (Metal)
- **Default model, modest Windows machines:** Whisper small/medium quantized; **NVIDIA Parakeet** via sherpa-onnx as fast English-only option
- **VAD:** Silero VAD — chunk at natural speech boundaries for streaming
- **BYOK cloud tier (opt-in):** OpenAI / Groq / Deepgram

### Analysis
- **Claude Code CLI** invoked headlessly post-meeting (and on-demand), with a bundled **skill** that:
  - Summarizes; extracts decisions, action items (owner + due date), and tickets
  - Optionally repo-aware: reference actual files/functions when a repo is linked
- Extension model: users drop their own skills/hooks ("after every meeting, run this")

### Storage (all plain files)
```
~/Meetings/
  2026-09-01-standup/
    meeting.md          # frontmatter: title, date, attendees, calendar-id
    transcript.md       # timestamped, You/Others labeled
    notes.md            # user's live notes
    tickets/
      TICK-001.md       # frontmatter: status, assignee, synced-to, external-id
  .app/config.toml
```
- Audio deleted after transcription by default (retention configurable)
- UI reads/writes these files directly; external edits (Obsidian, vim) are respected

### Integrations
- **Calendar:** Google Calendar + Microsoft Graph (Outlook) — OAuth, read-only event metadata
- **Trackers (opt-in sync):** Jira, Linear, GitHub Issues — user's API tokens, stored in OS keychain
- **MCP server:** exposes `search_meetings`, `get_transcript`, `list_tickets`, `get_decisions` etc. to Claude Code / Cursor / any MCP client

---

## 4. Meeting Detection

1. **Calendar-based:** upcoming event with 2+ attendees → notify 1 min before
2. **App/process detection:** Zoom, Teams, Meet (browser tab/process heuristics), Webex, Zoho, Slack Huddles, Discord
3. **Audio-activity heuristic:** sustained two-way audio as fallback signal
4. On detection → notification → **user clicks to start** (D5). Auto-stop suggestion when meeting audio ends.

---

## 5. v1 Feature Scope

**In:**
- Two-channel capture (macOS + Windows), real-time local transcription with live transcript view
- Notes pane, in-app tickets, memory panel (recent related meetings via grep/keyword), upcoming meetings list
- Claude Code post-meeting analysis (summary, decisions, action items, ticket drafts)
- Ticket sync: local MD, Jira, Linear, GitHub Issues (opt-in)
- Google + Outlook calendar
- MCP server
- BYOK cloud transcription option
- Auto-update, code signing + notarization (macOS)

**v2 / later:**
- ML diarization of Others channel (Speaker 1/2/…)
- Semantic search (sqlite-vec or similar) if grep proves insufficient
- Linux build; mobile companion (mic-only)
- Standup mode (yesterday's meetings + git commits + open tickets → drafted update)
- Commitment tracking cross-referenced with merged PRs
- ADR generation committed to repo
- CLI (`meet last --tickets`, `meet search "redis migration"`)

---

## 6. Key Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| macOS capture complexity (permissions UX, process taps) | Swift helper modeled on Muesli; study Open Granola's `src-tauri` capture code; ScreenCaptureKit fallback |
| Real-time Whisper perf on low-end Windows | Model tiering (small/medium/Parakeet); batch-mode fallback if streaming lags |
| Echo/double audio (mic picks up speakers) | Two-channel capture largely isolates it; document headphone recommendation; explore AEC later |
| Bluetooth device hot-swap dropping capture | Listen for default-device change events; auto-reattach |
| Claude Code CLI not installed / version drift | Detect on first run; graceful degradation to "transcript only" mode; BYOK Anthropic API as fallback (future) |
| Markdown-only storage scaling poorly | Fine for personal volume; revisit with vec index in v2 |

---

## 7. Prototype Plan (riskiest slice first)

1. **Week 1–2:** macOS Swift helper — capture mic + system audio to two WAV streams during a real Zoom call
2. **Week 2–3:** whisper.cpp streaming with Silero VAD → live transcript in a bare Tauri window
3. **Week 3–4:** Post-meeting Claude Code invocation with skill → one Linear ticket created end-to-end
4. **Then:** Windows WASAPI parity, calendar, ticket UI, MCP server

**Reference repos to study/fork:**
- `anshuman-pandey/open-granola` — closest full-stack reference (Tauri, per-platform loopback, local models)
- `pHequals7/muesli` — best macOS capture + VAD + diarization reference
- `pasrom/meeting-transcriber` — Claude CLI as analysis brain
- `fastrepl/anarlog`, Meetily — additional Tauri capture references
- `OpenWhispr/openwhispr` — model management / BYOK UX patterns

---

## 8. Developer-Workflow USPs (Selected)

The theme that makes the pitch literal: **"meetings → merged PRs."** Each USP below closes part of that loop.

### U1 — "Start Work" button on tickets ⭐ (build first)
One click on an in-app ticket spawns a Claude Code session (or copies a ready prompt) pre-loaded with:
- The ticket body + acceptance notes
- The relevant transcript excerpt (timestamped)
- The linked repo path + affected files (if repo-linked)

Meeting → agent-with-full-context in one action. Cheap to build (mostly prompt assembly), demo-perfect.

### U2 — Spec-vs-PR drift detection
When a PR opens on a linked repo, the analysis skill checks it against decisions from related meetings:
> "Meeting on 09-01 agreed to use Redis for sessions; this PR uses in-memory cache — flag?"

Requirements drift is invisible today; this makes it a diff-able artifact. (Depends on: decisions extraction + repo link + GitHub token.)

### U3 — Editor-native recall via MCP slash commands
Beyond raw MCP tools, ship polished commands/prompts for Claude Code & Cursor:
- `/meeting last` — summary + action items of the most recent meeting
- `/meeting decisions <topic>` — every decision ever made about a topic
- `/meeting who <system>` — who talked about / owns this area

Recall without leaving the editor. The MCP server (D11) is the plumbing; the commands are the product.

### U4 — Estimate tracking & personal calibration
Capture estimates said out loud ("that's like two days") into ticket frontmatter (`estimate: 2d`, `estimated_on: <meeting>`), then compare against actual close dates. Over months this yields personal calibration data ("your 2-day estimates average 3.5 days") — genuinely unique, zero extra effort from the user.

### U5 — Pre-meeting brief with git context
One minute before a recurring meeting (calendar-triggered), show:
1. Last meeting's summary + open action items
2. **Your git commits since then touching related areas** (the twist nobody else has)
3. Open tickets tagged to this meeting series

Walk in already caught up. Daily-habit hook.

### U6 — "Who owns what" map
From months of transcripts, infer an ownership graph (person ↔ system/feature/topic) stored as a plain `ownership.md` table, queryable via MCP: "who was talking about the Kafka migration?" Grows automatically; no manual curation.

### U7 — Lifecycle hooks (scriptable everything)
Shell hooks defined in config, receiving JSON on stdin. Keep the set small, logical, and easy to reason about — one hook per lifecycle stage:

| Hook | Fires when | Typical use |
|------|-----------|-------------|
| `on_meeting_detected` | Meeting detected, before user confirms | Mute Slack/DND, log attendance |
| `on_recording_start` | Capture begins | Start a screen recorder, toggle status light |
| `on_transcript_ready` | Final transcript written to disk | Pipe into a personal search index |
| `on_analysis_complete` | Claude Code skill finishes | Custom notification, post summary to a channel |
| `on_ticket_created` | Any ticket file is created | Push to a tracker we don't support natively |
| `on_ticket_synced` | External sync succeeds (Jira/Linear/GH) | Cross-post the external URL back into notes |
| `on_meeting_end` | Recording stops + files finalized | Catch-all: run anything ("after every meeting, run this") |

Rules that keep it manageable:
- Hooks are **fire-and-forget with a timeout** (default 30s) — a hanging script never blocks the pipeline
- Each hook gets a **stable JSON payload** on stdin (meeting id, file paths, ticket data) — versioned schema
- Failures are logged to `~/Meetings/.app/hooks.log`, never fatal
- Hooks live as executable files in `~/Meetings/.app/hooks/<hook_name>` (convention over configuration) — drop a script in, it runs; delete it, it stops

### U8 — Dotfiles-friendly config (JSON with comments)
Single config file in a git-trackable location: `~/Meetings/.app/config.jsonc`.

**Format decision: JSONC (JSON with comments)** rather than plain JSON or TOML:
- JSON's ubiquity wins for a dev audience — every language/tool parses it, hook payloads are already JSON, and editors validate it against a schema we ship (`"$schema"` key → autocomplete in VS Code for free)
- Plain JSON forbids comments, which hurts dotfiles ergonomics — JSONC fixes that while staying 99% JSON
- No hidden state anywhere else; the whole app is reproducible from this file + the markdown corpus

```jsonc
{
  "$schema": "https://…/config.schema.json",
  "transcription": {
    "engine": "whisper.cpp",          // or "openai", "groq", "deepgram"
    "model": "large-v3-turbo"
  },
  "tickets": {
    "default_destination": "in-app",
    "sync": ["linear"]                 // opt-in: "jira", "github", "local-md"
  },
  "hooks": { "timeout_seconds": 30 },
  "calendar": { "providers": ["google", "outlook"] }
}
```

### USP build order
1. **U1** (start-work button) — cheapest, most demo-able, makes the core pitch true
2. **U5** (pre-meeting brief + git) — daily habit driver
3. **U3** (MCP slash commands) — rides on the v1 MCP server
4. **U7 + U8** (hooks + config) — cheap, unlocks community integrations early
5. **U2** (drift detection) — highest wow, needs decisions extraction to mature first
6. **U4, U6** (calibration, ownership map) — compounding value, ship once data accumulates