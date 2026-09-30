# TUR-96 — Phase 2b-3 — live transcript: lines on screen and in transcript.md while the meeting runs

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **blocked** |
| Priority | high |
| Owner | Vox |
| Created | 2026-09-28 07:09 UTC by Alen |
| Parent | [TUR-92](TUR-92.md) Phase 2b — wire the real recorder and live transcription behind the Record button |
| Blocked because | Waiting on TUR-95 (blocked) |

## Description

The second half of TUR-90: the owner's `transcript.md` came back 0 bytes. Everything needed to fix that already exists and is tested — it is simply never called from the app.

What is already built: `SttSession` and `LiveEmitter` (`crates/stt/src/session.rs`), the volatile/final line model (`LiveUpdate`, L138), `MarkdownSink` (`crates/stt/src/sink.rs`), both engines (TUR-33, TUR-66..TUR-70), and the engine choice already reachable from Settings. `src-tauri` pulls `stt` in for the settings screens only.

#### Scope

- Open a live `stt` session against the running recording from 2b-2, one per track, using the seam TUR-15 and TUR-31 settled.
- Append finalized lines to `transcript.md` through `MarkdownSink`, in the strict SPEC §3.4 format. `You` is the mic track, `Others` is the system track.
- Push updates to the window as they land — volatile text while the speaker is still talking, replaced by the final line when it settles.
- Scroll behaviour from SPEC §5 Phase 2: no layout jump as lines arrive, and if the user has scrolled up to read something, stay there instead of yanking them to the bottom.
- Use the engine the user picked in Settings. No network call at transcription time on either path (L9–L11).
- If the engine fails mid-meeting, keep recording. Audio is the thing that cannot be recovered later; a transcript can be re-run from the WAVs. Say on screen that transcription stopped.

#### Gate

- Talk during a recording in the signed app: lines appear within a couple of seconds, on the right speaker.
- After stop, `transcript.md` is well-formed §3.4 and matches what was on screen.
- A quiet stretch produces nothing — the silence-hallucination guard has to hold live, not only against the fixture (TUR-67).
- Works on both engines, switched only in Settings.
