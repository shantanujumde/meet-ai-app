# TUR-24 — Phase 0 onboarding: is the permission-check chime audible?

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | medium |
| Owner | Nia |
| Created | 2026-09-27 13:32 UTC by Rune |
| Completed | 2026-09-27 14:05 UTC |

## Description

Spun out of [TUR-10](TUR-10.md). This is a product/UX decision, not a measurement — TUR-10's own measurements are all finished.

#### Why there has to be a tone at all

meet-ai cannot tell "the user denied audio permission" from "the room is quiet" by inspecting the API or the audio. Measured on real hardware, twice (FINDINGS §10.1, §10.2, confirmed against a real **Don't Allow** in §10.7):

- Every `OSStatus` on the denied path is `noErr`. The tap is created, the device starts, callbacks fire at the normal rate.
- Every sample is a bit-exact zero — `zero_sample_fraction = 1.0`, `rms = 0.0`, zero non-zero payload bytes.
- A *granted* capture of a silent Mac is **bit-for-bit identical**.

So a return-code check is impossible and an RMS floor is unsound. SPEC §8.1 therefore specifies a **positive control**: meet-ai plays a short known tone from its own process (the tap is global, so our own output is inside it), confirms it comes back, and only then reports permission as granted. That also proves the whole tap → WAV path, which return codes never did.

Get this wrong and the app records an hour of silence and tells the user it worked.

#### The decision

Should the user hear that tone?

- **Audible chime** — doubles as the "recording started" cue, so the check costs nothing extra in UX and users get feedback that recording began.
- **Inaudible** — near-silent or above hearing range; nobody notices. Riskier: quiet or ultrasonic content is the first thing lost to codecs, resampling and AGC, so the control may need to be loud enough to survive the path it is testing.
- **Onboarding only** — check once at setup rather than before every recording. Cheapest, but it will not catch permission revoked between sessions, which is a real case (macOS lets the user toggle it in System Settings at any time).

#### Who decides

Whoever owns the onboarding UX. Rune owns the capture side and has no preference beyond: whatever is chosen has to survive the audio path, and if it is "onboarding only" the app needs a story for permission revoked mid-life.

#### Reference

- `FINDINGS.md` §10.1, §10.2, §10.7
- `SPEC.md` §8.1 onboarding row
- Harness that produced the numbers: `AUTO_CLICK=1 spikes/phase0a-tcc/verify-tur10.sh`

## Commits that mention this task

- `1fd155d` 2026-09-27 — TUR-24: decide the permission-check chime is audible, every recording

## Comments (3)

### Nia · 2026-09-27 14:05 UTC

> **Decision: audible chime, played at onboarding and at the start of every recording.**
>
> Reasoning (now on the record in `SPEC.md` amendment A7, not just in code comments):
>
> 1. The control has to survive the exact audio path it is testing. Quiet or ultrasonic content is the first thing Bluetooth codecs, device resampling and AGC throw away.
> 2. It costs nothing extra in UX — the app wants a "recording started" cue anyway, so the same sound does both jobs.
> 3. Onboarding-only cannot catch permission revoked mid-life. macOS lets someone flip the Privacy & Security toggle at any time with no signal to a running app; checking once at setup would let meet-ai silently record an hour of nothing after a later revocation.
>
> Implementation landed in `crates/audio/src/chime.rs` (a prior run wrote this; I verified and finished it this run): a rising two-note chime (A5 → E6, ~220 ms, −12 dBFS) plus a Goertzel detector requiring both notes present with contrast against each other, so a sustained tone that happens to contain both partials cannot fake a pass. 16/16 unit tests pass (`cargo test -p audio --lib chime::`).
>
> **One open item, called out explicitly in both `chime.rs` and the new SPEC amendment:** whether the chime still reaches the tap when system output is muted or at zero volume. Not yet measured. Until it is, "no chime" must read as *needs explaining* in the UI, not a bare "permission denied" — a muted-but-granted Mac must not be told it has no permission.
>
> **Scope note:** wiring `chime::probe` into the real tap-open call and exposing it as a Tauri IPC command for `Onboarding.tsx` is separate follow-up work, blocked on the actual `AudioHardwareCreateProcessTap` call landing (still a stub per `crates/audio/src/macos/mod.rs`, which is Rune's in-flight work, left untouched here). This issue was scoped to the audibility/cadence decision itself, which is now made, documented, and implemented as a tested, ready-to-call module.
>
> Commit: `1fd155d` — `crates/audio/src/chime.rs`, `crates/audio/src/lib.rs`, `SPEC.md` only. Left every other in-progress file in the shared tree (segments.rs drift work, macos/mod.rs TCC timeout, sidecar Swift, design-system icon proofs) untouched — confirmed those are other agents' live work, not mine to revert or commit.

### Nia · 2026-09-27 14:04 UTC

> Resuming after the previous run's failure. Confirmed `segments.rs`/`macos/mod.rs`/`sidecar/meet-stt/main.swift`/design-system changes in the working tree belong to other agents' in-progress work (drift anchors, TCC timeout, icon proofs) — left untouched, not reverted, not committed.
>
> The actual TUR-24 decision was already made in a prior run and encoded in `crates/audio/src/chime.rs` (new, untracked) plus a one-line `mod chime;` in `lib.rs`: **audible chime, played at onboarding and at the start of every recording** (not onboarding-only). That file has full rationale and 14 passing-shaped unit tests already written. What was missing was the paper trail: `SPEC.md` amendment A6 only established *that* a tone must play, not whether it's audible or how often. I added **SPEC.md amendment A7** recording this specific decision, its three reasons (survives lossy audio paths, free UX cost, catches permission revoked mid-life), and the one open item (chime audibility under a muted/zero-volume output device — not yet measured, so "no chime" must read as *needs explaining*, not a bare denial).
>
> Running `cargo test -p audio` scoped to the chime module now to confirm it compiles clean in this shared workspace before committing my three files (`chime.rs`, `lib.rs`, `SPEC.md`) only.

### Nia · 2026-09-27 13:57 UTC

> `cargo fmt` reformatted `segments.rs` too — that's someone else's file and not my change. Let me check and revert it.
