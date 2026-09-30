# TUR-66 — Phase 1a — run the whisper fallback for the first time and measure it

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | high |
| Owner | Vox |
| Created | 2026-09-28 04:11 UTC by Alen |
| Completed | 2026-09-28 04:17 UTC |
| Parent | [TUR-5](TUR-5.md) Phase 1 — transcription: Apple built-in + Whisper fallback |

## Description

Half of the Phase 1 exit gate — "the same recording reads accurately on **both** engines" — has no evidence behind it. The Apple path is green on the synthetic fixtures (WER 6.5% on `mic.wav`, 0.0% on `system.wav`, speaker split correct, verified 2026-09-28). The whisper path has never been run at all.

#### Why it has never run

- `crates/stt/Cargo.toml` puts the whisper accuracy and silence tests behind the `whisper-model-tests` feature, off by default, so `cargo test -p stt` (114 passed) skips them silently.
- No model is on disk on this machine. `just models` has not been run here.

Both of those are reasonable defaults — `just check` should not need 190 MB and a network. But it means nobody has seen `WhisperEngine` transcribe anything.

#### Scope

- Download the pinned model (`just models`) and run `cargo test -p stt --features whisper-model-tests` green.
- Report the measured WER per track for the whisper engine, next to the Apple numbers above, on this issue.
- Confirm the speaker split (`You` from `mic.wav`, `Others` from `system.wav`) holds on the whisper path too.
- If the fixtures' WER thresholds are wrong for whisper, say so and set them where a regression would actually trip, rather than where the run happens to land.

#### Done when

The whisper accuracy and silence suites run green on a machine with the model present, and the numbers are posted here. Synthetic fixtures only — the real-recording half of the gate is a separate ticket.

Repo: `/Users/shantanujumde/apps/meet-ai`, branch `main`. Source of truth: `SPEC.md` §5 (Phase 1 row). Parent: TUR-5.

## Comments (1)

### Vox · 2026-09-28 04:17 UTC

> ## Whisper fallback: first run, measured
>
> Downloaded the pinned model (`just model small.en-q5_1`, 190 MB, checksum verified) and ran `cargo test -p stt --features whisper-model-tests -- --nocapture`. All 100 tests pass (75 unit + accuracy/live/sidecar/silence suites), including both whisper silence tests (`whisper_writes_nothing_for_silence`, `whisper_streams_nothing_over_thirty_quiet_seconds`).
>
> **WER, whisper vs Apple, same `two-speaker-60s` fixture (small.en-q5_1, greedy decode, temperature 0 — fully deterministic, reran twice with identical results):**
>
> | Track | Apple | Whisper |
> |---|---|---|
> | `mic.wav` (You) | 6.5% | 3.2% |
> | `system.wav` (Others) | 0.0% | 0.0% |
>
> Whisper's only error: "redis" → "riddance" (Apple made the same kind of miss: "redis" → "release"). Both are the same proper-noun-into-nonsense-word failure mode on synthetic TTS speech, not a structural problem.
>
> **Speaker split:** holds on the whisper path — `assert_speaker_split` passed for both tracks (no cross-track leakage), same as Apple.
>
> **Silence/hallucination guard:** whisper produced zero lines on `silence-30s.wav` and `room-tone-30s.wav`, in both the batch and streamed (live) paths. The VAD gate (layer 1) is doing its job — worth noting whisper never actually reached layers 2/3 (no-speech threshold, phrase blocklist) on these fixtures, since no spans were ever opened.
>
> **Threshold correction:** the `whisper_reads_the_same_recording_accurately` test had `wer < 0.30` — set before anyone had run it. Since the actual numbers (3.2%/0.0%) are far below that and are deterministic across runs, I tightened it to `wer < 0.15` (commit `8549075`) — same spirit as Apple's 25% bound: loose enough to survive a proper noun, tight enough to catch a real regression.
>
> **Caveat (per the issue's own scope note):** synthetic TTS only, as intended for this ticket — no room tone, codec artefacts, crosstalk, or real accents. The real-recording half of the Phase 1 gate is the separate ticket referenced in scope.
>
> Done: model downloaded, whisper suite green, WER measured and posted, speaker split confirmed, threshold corrected.
