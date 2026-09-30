# TUR-71 — Phase 1f — the gate run: a real meet-rec recording read on both engines

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **blocked** |
| Priority | medium |
| Owner | Vox |
| Created | 2026-09-28 04:12 UTC by Alen |
| Parent | [TUR-5](TUR-5.md) Phase 1 — transcription: Apple built-in + Whisper fallback |
| Blocked because | Waiting on TUR-98 (blocked) |

## Description

This is the Phase 1 exit gate itself. Everything else under TUR-5 runs on synthetic fixtures; this one needs real audio.

`crates/stt/tests/accuracy.rs` already says this out loud in its header: the fixtures are text-to-speech, with no room tone, no codec artefacts, no crosstalk, no accents and nobody talking over anybody. They prove the plumbing, the speaker split and the timestamp maths. They do not prove the gate.

#### Scope

Take a real recording produced by `meet-rec` — `mic.wav`, `system.wav`, `segments.json` — and:

- Transcribe it on **both** engines and read both transcripts. Is it accurate enough to be useful? That is a judgement call and it needs to be made out loud, not inferred from a WER number.
- Confirm `You` and `Others` land on the right people across the whole recording, not just at the start.
- Confirm quiet stretches inside a real meeting produce nothing, on both engines.
- Check the timestamps against the recording at a few points late in the file, where any `frame_to_sec` drift would show.

#### Blocked by

TUR-4. Phase 0 does not pass its gate yet — see the Phase 0 verification report on TUR-54. There is no 45-minute recording to read, device-change segment reopening is not wired up, and under `kill -9` the WAV headers currently claim more frames than `segments.json` accounts for, which is the exact condition `crates/stt/src/segments.rs` warns about and extrapolates through.

A shorter real recording is enough to start on the accuracy and speaker-split questions before the full 45-minute run exists. Do not close this on one.

Repo: `/Users/shantanujumde/apps/meet-ai`, branch `main`. Source of truth: `SPEC.md` §5. Parent: TUR-5.

## Comments (3)

### Paperclip (automatic) · 2026-09-28 04:58 UTC

> Paperclip could not resolve this issue's missing disposition automatically. The source assignment is unchanged and a board decision is required.

### Vox · 2026-09-28 04:58 UTC

> I've kicked off the real work for TUR-71: a genuine 150-second recording through this Mac's actual microphone and system-audio taps (via `meet-rec`), plus the release build and checksum-verified download of the whisper fallback model. Both are running in the background now. I'll resume the transcription comparison automatically once they finish — no further action needed from me until then.

### Paperclip (automatic) · 2026-09-28 04:50 UTC

> Paperclip needs a disposition before this issue can continue.
