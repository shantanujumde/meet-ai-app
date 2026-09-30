# TUR-90 — transcripts are not appearing, even I am speaking and my sys audio is also on

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **blocked** |
| Priority | medium |
| Owner | Alen |
| Created | 2026-09-28 06:57 UTC by You (board) |
| Blocked because | Waiting on TUR-92 (blocked) |

## Sub-tasks

- [TUR-92](TUR-92.md) **blocked** — Phase 2b — wire the real recorder and live transcription behind the Record button

## Description

![](/api/assets/0bfb4c91-5e2e-4d1e-85ff-4785980fee62/content)

## Questions you answered

- **Q:** Should TUR-92 (wiring the recorder and live transcription into the app) start now, or wait for TUR-71 first?
  **A:** Start TUR-92 now, in parallel

## Comments (1)

### Alen · 2026-09-28 07:01 UTC

> ## Your mic is fine. The app never recorded anything.
>
> The Record button in the window is not connected to the recorder yet. It runs the start/stop state machine and creates the meeting folder, and that is all it does — no audio is captured and nothing is transcribed, whether or not anyone is speaking.
>
> ### Evidence on your disk
>
> Both folders contain only empty files:
>
> ```
> ~/Meetings/2026-09-28-1216-meeting/  audio/ (empty)  notes.md 0B  transcript.md 0B
> ~/Meetings/2026-09-28-1225-meeting/  audio/ (empty)  notes.md 0B  transcript.md 0B
> ```
>
> An empty `audio/` is the tell. If capture had run and only transcription had failed, there would be `mic.wav` and `system.wav` sitting there.
>
> ### Where it comes from
>
> `src-tauri/src/recording.rs` is still the placeholder from TUR-17 (the Phase 2a shell). Its own header says it out loud: it writes *no audio and no transcript lines*, and `Status.stub` is hardcoded `true`.
>
> The app pulls in the `audio` crate only for the permission check and the `stt` crate only for the engine/model settings screens. Nothing in the app calls `meet-rec` or opens a transcription session. The recorder and both engines are built and working (TUR-4, TUR-33, TUR-66–TUR-70 are all done) — they were simply never plugged into the button.
>
> ### What I changed now
>
> One honesty fix, in `src/routes/Review.tsx`. The empty state you hit said *"Either nobody spoke, or this meeting was recorded before transcription was switched on. The audio, if it was kept, is still in the meeting folder"* — none of which is true here, and it sends you hunting a microphone problem you do not have. While capture is stubbed it now reads:
>
> > **Nothing was recorded** — Audio capture is not wired up in this build yet, so no sound was saved and nothing was transcribed — whether or not anyone was speaking. The recording controls and the meeting folder are real; the capture behind them lands next.
>
> There is already a matching banner while a recording is running; this closes the gap after you stop. Tests, typecheck and lint pass on the changed file.
>
> ### The real fix
>
> Raised as **TUR-92 — Phase 2b: wire the real recorder and live transcription behind the Record button** (high priority, now blocking this issue). It covers starting `meet-rec` on start, writing the WAVs and `segments.json`, opening a live `stt` session so `TranscriptSink` appends to `transcript.md`, streaming lines into the window, and deleting the stub flag and both "not wired up yet" notices.
>
> One thing to decide: TUR-71, the Phase 1 exit gate (a real recording read on both engines), is still blocked. Do you want TUR-92 to wait for that gate, or go ahead in parallel? Say the word and I will assign it.
