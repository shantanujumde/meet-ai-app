# TUR-98 — Phase 2b-5 — the gate run: a real 45-minute meeting, recorded and read inside the app

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **blocked** |
| Priority | high |
| Owner | Tess |
| Created | 2026-09-28 07:09 UTC by Alen |
| Parent | [TUR-92](TUR-92.md) Phase 2b — wire the real recorder and live transcription behind the Record button |
| Blocked because | Waiting on TUR-95 (blocked), TUR-96 (blocked), TUR-97 (blocked) |

## Description

The exit gate for Phases 0, 1 and 2 at once. It has been unreachable until now for one reason: there was no way to make a real recording without dropping to the CLI, so TUR-71 and TUR-7 have both been sitting blocked. After 2b-2 and 2b-3 the app itself produces the recording, and all three gates can be closed in a single sitting.

#### Scope

Join a real 45-minute call in the signed app. Record it end to end. Swap to AirPods somewhere in the middle. Then:

**Phase 0 gate** — both WAVs intact and non-truncated (`ffprobe`), drift under 200 ms end to end (`drift-check`), the device swap shows up as a clean segment boundary rather than a hole.

**Phase 1 gate** — read the transcript on **both** engines and say out loud whether it is accurate enough to be useful. That is a judgement call and it needs to be made in words, not inferred from a WER number (this is TUR-71's own framing). Check `You` versus `Others` late in the recording, not just at the start. Check timestamps at a few points near the end, where any clock drift would finally show.

**Phase 2 gate** — the subjective one, and the one the spec actually cares about: would you pick this over Notes for the next real meeting? Answer honestly. A no here is more useful than a yes, and it is what decides what gets built next.

#### Output

A verification report on this ticket in the shape of the Phase 0 report on TUR-54. Then close out TUR-71 and TUR-7, or say precisely what still fails.

Blocked on 2b-2 and 2b-3. Do not attempt this before the recorder is real — that is the mistake that produced TUR-90.
