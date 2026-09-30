# TUR-54 — verify phase 0, create tickets for phase 1

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | medium |
| Owner | Alen |
| Created | 2026-09-27 18:06 UTC by You (board) |
| Completed | 2026-09-28 04:13 UTC |

## Description

_No description._

## Document: Phase 0 verification report — gate by gate

_Key `phase0-verification`, last updated 2026-09-28 04:10 UTC._

### Phase 0 verification — what I ran, what passed, what did not

Verified by Alen on 2026-09-28 against `main` at `3f6c4e4`, on this Mac
(macOS 27.0, arm64). Gate wording is `SPEC.md` §5, Phase 0 row, verbatim:

> **45-min real Zoom call: both files intact, drift < 200ms end-to-end, survives an
> AirPods switch mid-call, survives `kill -9`**

I did not certify off anyone's status comment. Every number below came from a
command I ran in this session, and I re-probed the WAV bytes myself rather than
trusting `meet-rec`'s own summary line.

**Verdict: Phase 0 does not pass the gate yet.** Two of four conditions pass at
smoke scale, one cannot be demonstrated at all because the feature is not built,
and one passes with a contract violation attached. `SPEC.md` §5 says "miss a gate
→ stop, don't stack work on a broken layer", so Phase 1's *final* gate run stays
behind this. Most Phase 1 work does not.

---

#### What I ran

| Command | Result |
|---|---|
| `cargo test -p audio` | **84 passed, 2 ignored** (the two real-hardware closed-loop tests) |
| `cargo test -p stt` | **114 passed** |
| `cargo run -p audio --bin meet-rec -- --out … --duration 8` | recorded both channels, exit 0 |
| `meet-rec --duration 120` then `kill -9` at 14 s | both files decode; see G4 |

`meet-rec` works end to end on this machine right now. Microphone and
system-audio-recording permission are both live here — the recording below has
real signal in it, not zeros, so the TCC question that TUR-4 is parked on is
already answered on this host.

---

#### G1 — both files intact and playable · **PASS at smoke scale, unproven at gate scale**

8-second run, probed independently with Python's `wave` module:

| | `mic.wav` | `system.wav` |
|---|---|---|
| format | 16 kHz / 1 ch / s16 | 16 kHz / 1 ch / s16 |
| frames | 131712 (8.23 s) | 132240 (8.27 s) |
| file size vs `44 + frames·2` | 263468 = 263468 ✓ | 264524 = 264524 ✓ |
| RMS | 0.003243 | 0.071158 |
| bit-exact zero samples | 0.43 % | 0.76 % |

Format matches the contract exactly. Header length and on-disk byte count agree
to the byte. Neither track is the "clean, playable, 45 minutes of zeros" failure
Tess's harness calls out — both carry real signal.

**Not proven:** 45 minutes, and a real Zoom call. Nothing has run longer than
13 seconds.

#### G2 — drift < 200 ms end-to-end, with the measured number · **NOT PROVEN**

`meet-rec` self-reported **18.6 ms** on both channels over 8 seconds. That is
comfortably inside the gate, and the `anchors` array in `segments.json` has the
per-checkpoint shape the measurement needs. But the gate is 45 minutes and drift
is cumulative — an 8-second sample says nothing about minute 40.

Second problem: **the `drift-check` binary does not exist.** `SPEC.md:414` and
§6 specify `cargo run -p audio --bin drift-check -- audio/`, and `crates/audio`
declares exactly one `[[bin]]`, `meet-rec`. The `drift()` logic lives in
`segments.rs` and is tested, but the tool that is supposed to read a finished
recording and refuse to certify a bad one has not been built. Today the only
drift number available is the recorder grading its own homework at stop time.

#### G3 — survives an AirPods switch mid-call · **CANNOT BE DEMONSTRATED**

Not a test gap. The feature is not wired up, and `meet-rec`'s own header comment
says so (`crates/audio/src/bin/meet-rec.rs:13-20`): the binary writes exactly one
segment per recording, and device-change detection never calls `close_segment`.
Both of my recordings confirm it — one segment, `"reason": "start"`, in every
`segments.json`.

The *format* side is done: `close_segment`, close anchors and
`start_continuous_ns` all exist in `audio::segments` and are covered by
`segments_fixtures` (18 tests green). What is missing is the code that notices
the device changed.

#### G4 — survives `kill -9` · **PASS, with a contract violation**

`SIGKILL` at 14 s, last checkpoint at 10 s. Both files decode cleanly with a
valid RIFF header and play back the audio up to the last checkpoint. That is the
gate condition, and it is met.

Two things the run also showed:

**1. Expected and fine — an undeclared tail.** `mic.wav` is 439596 bytes on disk
but its header declares 164480 frames (329004 bytes). The ~110 KB past the
declared end is audio captured after the last header patch. That is the design:
the header only ever claims bytes that are already fsynced, so a reader sees a
short, valid file rather than a long, corrupt one.

**2. Not fine — the headers claim more than `segments.json` accounts for.**

| | header frames | `segments.json` frames | difference |
|---|---|---|---|
| mic | 164480 | 164138 | **+342** |
| system | 164281 | 163939 | **+342** |

Contract rev 2 §11 states the invariant as `sum(segment frames) >= header_frames`,
**always** — segments over-cover the file, never the other way round. Here the
header over-covers by 342 frames (21 ms) on both channels.

The cause is in the checkpoint itself, not in the crash. `checkpoint()`
(`meet-rec.rs:196-219`) reads `position()`, writes those counts into
`segments.json`, then calls `patch_header()` — and `patch_header` declares
`self.appended_frames`, the *live* count at patch time
(`wav_writer.rs:102-115`), not the count that was just written out. The capture
thread keeps appending during the atomic `segments.json` write, so the header
lands ahead of the segments by however much audio arrived in that window. The
identical +342 on both channels is that window.

A clean stop hides it, because the final exact snapshot overwrites both with
matching numbers. It only becomes visible after a force-quit — which is exactly
the case the invariant was written for. Downstream, `crates/stt/src/segments.rs`
treats a frame past the declared total as proof the upstream invariant broke: it
warns and extrapolates a timestamp for the final ~21 ms of every force-quit
recording.

**The fix is small:** have `patch_header` declare the frame count that was passed
to `segments.json`, rather than re-reading the live one.

---

#### Also worth knowing

- **TUR-4's blocker is stale.** It sits `blocked` on a pending question asking a
  human to grant microphone and system-audio permission and run
  `cargo test -p audio --test mic_closed_loop -- --ignored`. Both grants are
  already live on this host — I recorded real, non-silent audio through both
  paths this session. Rune re-ran that closed-loop test green twice on
  2026-09-27 and verified the tap the same way. The question can be closed.
- **A real bug was caught by running the thing**, not by review: `SystemSource::stop()`
  used to clear the state that `position()` reads, so the final system frame count
  was written as 0. Fixed in `5ffb58c`. Worth noting because it is the kind of
  defect only an end-to-end run finds.

#### What Phase 0 needs to close

1. Wire device-change detection into `meet-rec` so a segment closes and reopens
   on an output-device switch (G3).
2. Build the `drift-check` binary SPEC §6 specifies (G2).
3. Fix the header/segments frame-count inversion at checkpoint time (G4).
4. Then the real 45-minute call, with the drift number reported (G1, G2).


## Comments (9)

### Alen · 2026-09-28 04:13 UTC

> ## Phase 0 verified, Phase 1 broken into six tickets
>
> Full evidence: [#document-phase0-verification](TUR-54.md). Everything below came from commands I ran this session against `main` at `3f6c4e4`; I re-probed the WAV bytes myself rather than trusting `meet-rec`'s own summary.
>
> ### Phase 0 — does not pass the gate yet
>
> Gate is `SPEC.md` §5: *45-min real Zoom call, both files intact, drift < 200 ms end-to-end, survives an AirPods switch mid-call, survives `kill -9`.*
>
> | | Verdict |
> |---|---|
> | Both files intact | **Pass at 8 s**, unproven at 45 min. 16 kHz/1 ch/s16 on both, header length matches on-disk bytes to the byte, real signal in both tracks |
> | Drift < 200 ms | **Not proven.** 18.6 ms over 8 s, but drift is cumulative and nothing has run past 13 s. The `drift-check` binary SPEC §6 specifies was never built |
> | AirPods switch | **Cannot be demonstrated.** `meet-rec` writes one segment per recording; device-change detection is not wired up. The format side (`close_segment`, close anchors) is done and tested |
> | `kill -9` | **Pass, with a contract violation.** Files decode and play. But the headers claim 342 more frames than `segments.json` accounts for, on both channels — the inverse of the rev-2 invariant |
>
> The `kill -9` finding is new and it is a real bug, not a crash artefact. `checkpoint()` reads `position()`, writes it to `segments.json`, then `patch_header()` declares the *live* frame count instead of the one just written — so the header always lands ahead by however much audio arrived during the atomic write. A clean stop hides it; a force-quit shows it. `crates/stt/src/segments.rs` then extrapolates through the last ~21 ms of every force-quit recording. Filed on [TUR-4](TUR-4.md) with the fix.
>
> Also: **TUR-4's blocker is stale.** It is parked waiting for a human to grant microphone and system-audio permission. Both grants are already live on this host — I recorded non-silent audio through both paths this session. That question can be withdrawn.
>
> Good news buried in there: `cargo test -p audio` is 84 passed / 2 ignored, `cargo test -p stt` is 114 passed, and `meet-rec` genuinely records both channels end to end today.
>
> ### Phase 1 — six tickets, all children of [TUR-5](TUR-5.md), all `todo` and assigned to Vox
>
> I left them `todo` rather than starting them, so you can reorder or drop any before work begins.
>
> | | |
> |---|---|
> | [TUR-66](TUR-66.md) | Run the whisper fallback for the first time and measure it. **Half the gate has zero evidence** — the Apple path is green on fixtures (WER 6.5% / 0.0%), whisper has never been run. Its tests sit behind an off-by-default feature and no model is on disk |
> | [TUR-67](TUR-67.md) | The silence-hallucination guard on the whisper path. "Thank you." over a quiet stretch is a whisper failure; only the Apple engine is proven silent today |
> | [TUR-68](TUR-68.md) | Prove the model download on a cold machine — resume from a killed download, catch a corrupted one, skip a finished one |
> | [TUR-69](TUR-69.md) | Make the engine switch an actual config change. `SPEC.md` §5 makes this its own gate line and it currently fails — nothing reads `config.jsonc`, so switching engines means rebuilding |
> | [TUR-70](TUR-70.md) | Prove transcription works with the network off. The architecture makes it structurally true; nobody has run it |
> | [TUR-71](TUR-71.md) | The gate run itself — a real `meet-rec` recording read on both engines. **Blocked on TUR-4** |
>
> Only TUR-71 waits on Phase 0. The other five can start now.
>
> ### The one call I'd flag
>
> `SPEC.md` §5 says "miss a gate → stop, don't stack work on a broken layer." Phase 0 misses. I have kept to that where it bites — the real-recording gate run is blocked — but I have not held back the whisper engine, the model download or the config switch, because none of them touch the recorder. If you would rather freeze Phase 1 entirely until Phase 0 closes, say so and I will move the five to `blocked`.

### You (board) · 2026-09-28 04:04 UTC

> retry

### Alen · 2026-09-28 03:47 UTC

> ACP agent reported a terminal access failure.

### Alen · 2026-09-28 03:46 UTC

> ACP agent reported a terminal access failure.

### Alen · 2026-09-28 03:45 UTC

> ACP agent reported a terminal access failure.

### You (board) · 2026-09-28 03:45 UTC

> retry

### Alen · 2026-09-27 18:15 UTC

> ACP agent reported a terminal access failure.

### Alen · 2026-09-27 18:14 UTC

> ACP agent reported a terminal access failure.

### Alen · 2026-09-27 18:06 UTC

> ACP agent reported a terminal access failure.
