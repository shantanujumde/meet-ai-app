# TUR-92 — Phase 2b — wire the real recorder and live transcription behind the Record button

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **blocked** |
| Priority | high |
| Owner | Nia |
| Created | 2026-09-28 07:00 UTC by Alen |
| Parent | [TUR-90](TUR-90.md) transcripts are not appearing, even I am speaking and my sys audio is also on |
| Blocked because | Waiting on TUR-95 (blocked), TUR-96 (blocked), TUR-97 (blocked), TUR-98 (blocked) |

## Sub-tasks

- [TUR-94](TUR-94.md) **done** — Phase 2b-1 — lift the meet-rec record loop into crates/audio as a start/stop session
- [TUR-95](TUR-95.md) **blocked** — Phase 2b-2 — put the real recorder behind the Record button
- [TUR-96](TUR-96.md) **blocked** — Phase 2b-3 — live transcript: lines on screen and in transcript.md while the meeting runs
- [TUR-97](TUR-97.md) **blocked** — Phase 2b-4 — a recording killed mid-meeting still leaves usable files
- [TUR-98](TUR-98.md) **blocked** — Phase 2b-5 — the gate run: a real 45-minute meeting, recorded and read inside the app
- [TUR-119](TUR-119.md) **done** — Watchdog review for TUR-92

## Description

Raised by TUR-90: the owner recorded two meetings with the mic live and system audio on, and both meeting folders came back with an empty `audio/` directory and a 0-byte `transcript.md`.

#### Why it happened

`src-tauri/src/recording.rs` is still the TUR-17 stub. Its own header says so: it runs the start/stop state machine and creates the SPEC §3.1 meeting folder, and it writes **no audio and no transcript lines**. `Status.stub` is hardcoded `true`.

`src-tauri` pulls in `audio` only for `permission_check` and `stt` only for the engine/model settings screens. Nothing in the app ever calls `meet-rec` or opens an `stt` session. The recorder crate and both engines work — TUR-4, TUR-33, TUR-66..TUR-70 are done — they are simply not connected to the button.

#### Scope

- Replace the stub recorder with the real one: start `meet-rec` (or its library seam) on `start`, write `audio/mic.wav`, `audio/system.wav` and `segments.json` into the meeting folder, close them cleanly on `stop`.
- Open a live `stt` session against the running tap using the seam from TUR-15/TUR-31, and let `TranscriptSink` append to `transcript.md`.
- Stream lines into the window as they land (SPEC §2 live transcript view), preserving scroll position when the user has scrolled up.
- Drop `Status.stub` and the two "not wired up yet" notices it drives, in `src/ui/Shell.tsx` and the empty state in `src/routes/Review.tsx`.
- Surface a real capture failure (device disappeared, tap refused) as a visible error, not as a silent empty transcript.

#### Depends on

TUR-71 is the Phase 1 exit gate and is still blocked. Decide whether this ships before or after that gate run.

#### Done when

A recording started from the window produces non-empty `audio/*.wav` and a `transcript.md` with lines in it, and the review view shows them.

## Comments (10)

### Paperclip (automatic) · 2026-09-28 12:34 UTC

> Paperclip could not resolve this issue's missing disposition automatically. The source assignment is unchanged and a board decision is required.

### Nia · 2026-09-28 12:33 UTC

> Checked again this heartbeat: no change since the last status. TUR-115's `request_confirmation` (asking whether Tess may run steps 3-7 of the gate autonomously — real audio + revoking system Microphone/System Audio permission on this live machine) is still `pending`, no human response yet. TUR-95 stays `blocked` on that gate; TUR-96/97/98 stay correctly `blocked` behind TUR-95. TUR-90 has no new comments since the original report. Working tree clean, HEAD still at `02b400d`. Nothing actionable for me here — this is a human decision, not one I'll approve on Tess's behalf. Leaving TUR-92 `in_progress`; it resumes when the human answers TUR-115's confirmation.

### Paperclip (automatic) · 2026-09-28 12:33 UTC

> Paperclip needs a disposition before this issue can continue.

### Nia · 2026-09-28 12:32 UTC

> Update since last heartbeat: TUR-115 made real progress — Tess confirmed codesigning identity and produced a signed, verified `meet-ai.app` bundle (steps 1-2 of 7, both PASS). Tess then opened a `request_confirmation` on TUR-115 asking the human owner whether to run the rest of the gate (steps 3-7: launching the live app, scripted clicks, audible mic/system audio via `say`/`afplay`, and temporarily revoking system-wide Microphone + System Audio Recording permission) autonomously on this active machine, since Slack/WhatsApp/Messages/Docker are all running in the same session. That is rightly a call for the human, not something I am going to accept on Tess's behalf — it disrupts the user's live machine and breaks other apps' mic access until re-granted.
>
> - TUR-94 — done.
> - TUR-95 (mine) — implementation committed at `02b400d`, still `blocked` on TUR-115's exit gate.
> - TUR-96/97/98 — still correctly `blocked` behind TUR-95.
> - TUR-90 (parent) — no new human comments since the original report.
>
> No code changes needed this heartbeat. Working tree clean. Leaving TUR-92 `in_progress`; it resumes once the human answers TUR-115's confirmation and Tess finishes the gate.

### Nia · 2026-09-28 12:30 UTC

> Verified via API rather than trusting the cached summary: TUR-115 is still `in_progress`, actively running (Tess, run 69534015-d7bb-4ce1-952b-21030c827ad7), last activity 2026-09-28T12:29:14Z — about 7 minutes before this check, so it is genuinely still working, not stalled. TUR-96/97/98 confirmed still `blocked`. No drift since the last status comment. Working tree clean, HEAD still at 02b400d. Nothing actionable for me this heartbeat; leaving TUR-92 `in_progress` — it wakes on TUR-115 resolving.

### Nia · 2026-09-28 12:29 UTC

> Comment posted. TUR-92 stays `in_progress` — it has a live continuation path (wakes when TUR-115 resolves, which Tess is actively running in this same workspace right now). No code changes were needed this heartbeat: TUR-95's implementation is already committed and correctly blocked on Tess's verification gate, and TUR-96/97/98 remain correctly blocked behind it.

### You (board) · 2026-09-28 12:29 UTC

> Re-checked state this heartbeat, nothing has changed since the last status comment:
>
> - **TUR-94** (Vox) — done.
> - **TUR-95** (me) — implementation committed at `02b400d`, still `blocked` on **TUR-115**, which is `in_progress` right now with Tess actively running it in this shared workspace (0 comments posted yet, so no result to react to).
> - **TUR-96/97/98** — still `blocked`, correctly waiting on TUR-95 to close; nothing for me to unblock, owned by Vox/Rune/Tess respectively.
> - Checked TUR-90 (parent) for new human comments — none since the original report I already answered.
>
> No actionable code work this heartbeat. Working tree is clean, no uncommitted changes. Leaving TUR-92 `in_progress`; it resumes when TUR-115 resolves.

### Paperclip (automatic) · 2026-09-28 12:28 UTC

> Paperclip needs a disposition before this issue can continue.

### Nia · 2026-09-28 12:28 UTC

> Status check across the five children:
>
> - **TUR-94** (Vox) — done. `crates/audio` now exposes a start/tick/stop `RecordingSession`.
> - **TUR-95** (me) — implementation done, committed as `02b400d`: `recording.rs` drives the real session, `Status.stub` and its UI copy are gone, permission-denied and tap-open failures surface as real errors instead of a silent empty folder. All automated checks green (cargo/tsc/vitest/biome). Blocked on **TUR-115** (Tess, in progress right now) for the ticket's own exit gate — signed bundle + real mic/system audio + drift-check + permission-revoked check — which needs a codesigning identity and a human pressing Record that this sandbox does not have.
> - **TUR-96** (Vox) — blocked, not yet started. Live transcript on screen + `transcript.md` streaming.
> - **TUR-97** (Rune) — blocked, not yet started. Kill-mid-recording durability.
> - **TUR-98** (Tess) — blocked, not yet started. The real 45-minute gate run that also closes TUR-71/TUR-7.
>
> No action available to me right now beyond what's already delegated: TUR-95's code is done and waiting on Tess's TUR-115 verification, and TUR-96 through TUR-98 belong to other agents. Leaving this umbrella `in_progress` — it will move when TUR-115 closes and TUR-95 can go `done`, which unblocks TUR-96.

### Alen · 2026-09-28 07:10 UTC

> Broken down into five children under TUR-93, in the order they have to happen:
>
> - **TUR-94** — lift the `meet-rec` record loop out of the binary into `crates/audio` as a start/stop session. Doing this first because the whole loop is currently `fn record()` inside `src/bin/meet-rec.rs:380` — private, blocking, driven by a `--duration` flag. Nothing can call it until this lands.
> - **TUR-95** — put that session behind the Record button, delete `Status.stub`.
> - **TUR-96** — live transcript on screen and into `transcript.md`.
> - **TUR-97** — prove a killed recording still leaves usable files at the new boundary.
> - **TUR-98** — the real 45-minute gate run, which also closes TUR-71 and TUR-7.
>
> This issue stays as the umbrella.
