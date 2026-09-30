# TUR-67 — Phase 1b — the silence-hallucination guard on the whisper path

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | high |
| Owner | Vox |
| Created | 2026-09-28 04:11 UTC by Alen |
| Completed | 2026-09-28 04:22 UTC |
| Parent | [TUR-5](TUR-5.md) Phase 1 — transcription: Apple built-in + Whisper fallback |

## Description

"30 seconds of silence produces zero transcript lines" is a Phase 1 gate line, and TUR-5 names the exact failure it exists to catch: **whisper emitting "Thank you." over a quiet stretch.** That failure is whisper-specific.

What is proven today (verified 2026-09-28): `apple_engine_writes_nothing_for_silence` passes, `vad_finds_no_speech_in_either_silence_fixture` passes, and a silent meeting produces an empty `transcript.md` rather than a missing one. All of that is the Apple engine and the VAD.

What is not proven: that **whisper** stays silent. Its silence test sits behind the `whisper-model-tests` feature and has never run.

#### Scope

- Run the silence suite against the real whisper model and show it green.
- If whisper does hallucinate through the current VAD gating, that is the finding — fix the gating, do not loosen the test.
- Leave it as a standing regression test, not a one-time check. TUR-5 is explicit: "build the regression test for it, do not just eyeball it once."
- Say plainly how it runs in a routine check, given that `just check` deliberately does not download a model. A guard nobody runs is not a guard.

#### Done when

The whisper silence test runs green on a machine with the model, and there is a stated, repeatable way it gets run so a regression is caught rather than discovered.

Repo: `/Users/shantanujumde/apps/meet-ai`, branch `main`. Source of truth: `SPEC.md` §5. Parent: TUR-5.

## Comments (1)

### Vox · 2026-09-28 04:22 UTC

> Ran the whisper half of the silence gate for real, on this machine: downloaded and checksum-verified `small.en-q5_1` (SPEC §2.4), built the sidecar, then `just check-whisper`. Green: 118 tests, 0 failed — `whisper_writes_nothing_for_silence` passes on both `silence-30s.wav` and `room-tone-30s.wav`, and the live-session equivalent (`whisper_streams_nothing_over_thirty_quiet_seconds`) passes too.
>
> Finding: no hallucination got through. The existing three-layer guard (VAD gating, `no_speech_thold`, the shape+phrase blocklist) already holds against the real model — no gating change was needed.
>
> What was actually missing was not the test (it already existed, feature-gated correctly so `just check` stays networkless) but a reason for anyone to ever flip that feature on. Fixed that: `CONTRIBUTING.md` now names `just check-whisper` explicitly with trigger conditions (any change to `crates/stt/src/vad.rs`, `whisper.rs`, or the model catalog; every Phase 1 sign-off), and `SPEC.md` A9 records today's run.
>
> This repo has no CI at all yet, so the repeatable mechanism is a documented manual gate, not an automated one. If you want this wired into actual CI (macOS runner, model caching) that's a bigger, separate decision — flagging it rather than assuming it.
>
> Commit: 4b333c5. Note: TUR-66 (whisper accuracy/WER) was running concurrently in this same checkout and already landed as 8549075 — unrelated to this ticket, left untouched.
