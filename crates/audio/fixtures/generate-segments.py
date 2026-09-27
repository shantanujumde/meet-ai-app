#!/usr/bin/env python3
"""Generate the `segments.json` fixtures for the Phase 0 drift gate.

Run via `just fixtures` (generate.sh calls this). Output lands in
`crates/audio/fixtures/segments/<fixture-name>/segments.json` and IS committed —
each file is a few tens of KB, and the on-disk shape is half of what these
fixtures test. `crates/audio/tests/segments_fixtures.rs` reads them back through
`Segments::from_json`, so a change to the serde shape breaks a test instead of
breaking a recording.

Every fixture here is synthetic: hand-computed anchors against a hand-computed
host clock. Nothing needs a microphone, `meet-rec`, or a transcription engine.

## The formula

SPEC A5 (`SPEC.md:520`) defines drift per channel at each checkpoint anchor as

    drift = anchors[i].frames / 16000 - (anchors[i].host_ns - start_host_ns) / 1e9

so a device clock fast by `p` ppm produces `drift = elapsed_s * p / 1e6`. Over a
2700 s (45 min) recording **200 ms is exactly 74.07 ppm**, which is why the edge
pair below is 74 / 75 ppm and not something rounder.

Frame counts are `floor(elapsed_s * 16000 * (1 + ppm/1e6) + 0.5)` — half-up, so
the value is the same on every machine and every language. The residual is at
most half a frame (31.25 us), inside the +/-1 frame (62.5 us) tolerance the
fixture spec allows.

## WAVs

Off by default. The drift maths reads `segments.json` alone; the WAVs exist only
so a future `drift-check` can cross-check a declared frame count against a real
header, and 2700 s of 16 kHz mono s16 is 86 MB per channel — about 1 GB across
the drift fixtures. Set `FIXTURE_WAVS=1` to write them (silence of exactly the
declared frame count). They are gitignored either way.
"""

from __future__ import annotations

import json
import math
import os
import struct
import sys
from pathlib import Path

# Mirrors of the constants in `crates/audio/src/segments.rs`. If one of these
# ever disagrees with the Rust side, the fixtures stop describing the thing
# under test — `segments_fixtures.rs` asserts the pair that matters
# (SAMPLE_RATE_HZ, CHECKPOINT_INTERVAL_S) rather than trusting this comment.
RATE = 16_000
CHECKPOINT_S = 5

# Non-zero so that a bug which forgets to subtract `start_host_ns` shows up as a
# wild number rather than accidentally reading correct against an origin of 0.
START_HOST_NS = 1_000_000_000
# 2026-09-27T09:00:00Z, give or take. Only ever rendered, never differenced.
START_UNIX_NS = 1_790_499_600_000_000_000
DEVICE_RATE = 48_000

FORTY_FIVE_MINUTES_S = 45 * 60

REASON_START = "start"
REASON_OUTPUT_DEVICE_CHANGED = "default_output_device_changed"

HERE = Path(__file__).resolve().parent
OUT = HERE / "segments"


def frames_at(elapsed_s: float, ppm: float) -> int:
    """Frames a clock `ppm` parts-per-million fast has produced by `elapsed_s`."""
    return math.floor(elapsed_s * RATE * (1.0 + ppm / 1e6) + 0.5)


def anchor(host_ns: int, mic_frames: int, sys_frames: int, sys_host_ns: int | None = None) -> dict:
    return {
        "mic_host_ns": host_ns,
        "mic_frames": mic_frames,
        "sys_host_ns": host_ns if sys_host_ns is None else sys_host_ns,
        "sys_frames": sys_frames,
    }


def anchor_series(start_host_ns: int, duration_s: float, mic_ppm: float, sys_ppm: float) -> list:
    """One anchor per completed checkpoint interval, host clock advancing truly.

    A segment that ends off the 5 s grid keeps the remainder as unanchored tail,
    which is exactly what a real stop does.
    """
    checkpoints = int(duration_s // CHECKPOINT_S)
    series = []
    for k in range(1, checkpoints + 1):
        elapsed_s = k * CHECKPOINT_S
        series.append(
            anchor(
                host_ns=start_host_ns + elapsed_s * 1_000_000_000,
                mic_frames=frames_at(elapsed_s, mic_ppm),
                sys_frames=frames_at(elapsed_s, sys_ppm),
            )
        )
    return series


def segment(
    idx: int,
    start_host_ns: int,
    duration_s: float,
    *,
    mic_ppm: float = 0.0,
    sys_ppm: float = 0.0,
    reason: str = REASON_START,
    asleep_ns: int = 0,
    mic_rate: int = RATE,
    sys_rate: int = RATE,
    anchors: list | None = None,
    mic_frames: int | None = None,
    sys_frames: int | None = None,
) -> dict:
    """A segment closed gracefully at `duration_s`, anchored every 5 s."""
    if anchors is None:
        anchors = anchor_series(start_host_ns, duration_s, mic_ppm, sys_ppm)
    if mic_frames is None:
        mic_frames = frames_at(duration_s, mic_ppm)
    if sys_frames is None:
        sys_frames = frames_at(duration_s, sys_ppm)

    elapsed_ns = start_host_ns - START_HOST_NS
    return {
        "idx": idx,
        "start_host_ns": start_host_ns,
        "start_continuous_ns": START_HOST_NS + elapsed_ns + asleep_ns,
        "start_unix_ns": START_UNIX_NS + elapsed_ns + asleep_ns,
        "mic_rate": mic_rate,
        "sys_rate": sys_rate,
        "mic_frames": mic_frames,
        "sys_frames": sys_frames,
        "reason": reason,
        "mic_device_rate": DEVICE_RATE,
        "sys_device_rate": DEVICE_RATE,
        "anchors": anchors,
    }


def render(segments: list) -> str:
    """`segments.json`, one anchor per line so a diff stays readable."""
    out = ['{\n  "version": 1,\n  "segments": [']
    for s_idx, seg in enumerate(segments):
        out.append("    {")
        body = {k: v for k, v in seg.items() if k != "anchors"}
        lines = [f"      {json.dumps(k)}: {json.dumps(v)}" for k, v in body.items()]
        anchors = seg.get("anchors")
        if anchors is None:
            out.append(",\n".join(lines))
        else:
            out.append(",\n".join(lines) + ',')
            out.append('      "anchors": [')
            out.append(
                ",\n".join(
                    "        " + json.dumps(a, separators=(", ", ": ")) for a in anchors
                )
            )
            out.append("      ]")
        out.append("    }" + ("," if s_idx + 1 < len(segments) else ""))
    out.append("  ]\n}")
    return "\n".join(out) + "\n"


def silence_wav(path: Path, frames: int) -> None:
    """A valid 16 kHz mono s16 WAV of exactly `frames` frames of silence."""
    data_bytes = frames * 2
    with path.open("wb") as f:
        f.write(b"RIFF")
        f.write(struct.pack("<I", 36 + data_bytes))
        f.write(b"WAVEfmt ")
        f.write(struct.pack("<IHHIIHH", 16, 1, 1, RATE, RATE * 2, 2, 16))
        f.write(b"data")
        f.write(struct.pack("<I", data_bytes))
        chunk = bytes(1 << 20)
        remaining = data_bytes
        while remaining > 0:
            f.write(chunk[: min(remaining, len(chunk))])
            remaining -= min(remaining, len(chunk))


def write(name: str, segments: list, *, wavs: tuple[str, ...] = ("mic", "system")) -> None:
    directory = OUT / name
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "segments.json").write_text(render(segments))

    if os.environ.get("FIXTURE_WAVS") != "1":
        return
    for channel, field in (("mic", "mic_frames"), ("system", "sys_frames")):
        path = directory / f"{channel}.wav"
        if channel not in wavs:
            path.unlink(missing_ok=True)
            continue
        silence_wav(path, sum(s[field] for s in segments))


# --- 2. Drift fixtures (fixture spec §2) ------------------------------------
#
# All 2700 s, one segment, 540 anchors. `sys` stays on a true clock except where
# the row says otherwise, so `max_track_skew_ms` has something to show.


def drift_fixtures() -> None:
    for name, mic_ppm, sys_ppm in [
        # 200 ms at 2700 s is 74.07 ppm; 74 and 75 straddle it by 0.2 / 2.5 ms.
        ("drift-clean-0ppm", 0.0, 0.0),
        ("drift-pass-50ppm", 50.0, 0.0),
        ("drift-edge-74ppm", 74.0, 0.0),
        ("drift-edge-75ppm", 75.0, 0.0),
        ("drift-fail-100ppm", 100.0, 0.0),
        # The regression test for the original blocker: both clocks slide
        # together, so a frame-count subtraction reads ~0 while the real drift
        # is 270 ms on both tracks.
        ("drift-common-mode-100ppm", 100.0, 100.0),
    ]:
        write(
            name,
            [
                segment(
                    0,
                    START_HOST_NS,
                    FORTY_FIVE_MINUTES_S,
                    mic_ppm=mic_ppm,
                    sys_ppm=sys_ppm,
                )
            ],
        )


# --- 3. Refusal fixtures (fixture spec §3) ----------------------------------
#
# Each must refuse rather than flatter. Four of the seven refuse today; the rest
# are written here as the acceptance tests for the refusals that do not exist
# yet, and their assertions are `#[ignore]`d with the reason.


def refusal_fixtures() -> None:
    # A failed tap. The supported signal is `sys_rate: 0` plus no `system.wav`
    # on disk — absent, not zero-length.
    no_system = segment(0, START_HOST_NS, 600, sys_rate=0, sys_frames=0)
    for a in no_system["anchors"]:
        a["sys_frames"] = 0
        a["sys_host_ns"] = 0
    write("refuse-no-system", [no_system], wavs=("mic",))

    # The contradictory shape: frames were written but the rate field says the
    # track never started. Proves the absent check keys on rate, not on frames.
    write("refuse-zero-rate", [segment(0, START_HOST_NS, 600, sys_rate=0)])

    # A revision-1-shaped file: valid segments, no `anchors` key at all.
    no_anchors = segment(0, START_HOST_NS, 600)
    del no_anchors["anchors"]
    write("refuse-no-anchors", [no_anchors])

    # One checkpoint, nothing to difference. Drift is measured against
    # `start_host_ns` rather than against the previous anchor, so this currently
    # yields a number — see the finding on TUR-29.
    write("refuse-one-anchor", [segment(0, START_HOST_NS, CHECKPOINT_S)])

    # F2, the one that matters most: the segment declares 2700 s of frames but
    # the anchors stop at 300 s. Measures 3 ms of drift over the 5 minutes it
    # can see and says nothing about the 40 minutes it cannot.
    stops_early = segment(
        0,
        START_HOST_NS,
        FORTY_FIVE_MINUTES_S,
        mic_ppm=10.0,
        anchors=anchor_series(START_HOST_NS, 300, 10.0, 0.0),
    )
    write("refuse-anchors-stop-early", [stops_early])

    # A stubbed clock: 60 checkpoints, every one reporting the host time of the
    # first. The frames keep coming because the audio is real.
    frozen_host_ns = START_HOST_NS + CHECKPOINT_S * 1_000_000_000
    frozen = segment(
        0,
        START_HOST_NS,
        300,
        anchors=[
            anchor(frozen_host_ns, frames_at(k * CHECKPOINT_S, 0.0), frames_at(k * CHECKPOINT_S, 0.0))
            for k in range(1, 61)
        ],
    )
    write("refuse-frozen-clock", [frozen])

    # Beyond the spec's seven, and the worse sibling of the row above: the
    # anchor latch itself is stuck, so host time AND frames repeat. Every drift
    # reads 0 and the recording certifies itself.
    stuck_frames = frames_at(CHECKPOINT_S, 0.0)
    stuck = segment(
        0,
        START_HOST_NS,
        300,
        anchors=[anchor(frozen_host_ns, stuck_frames, stuck_frames) for _ in range(60)],
    )
    write("refuse-frozen-clock-and-frames", [stuck])

    # Host time goes backwards between the last two anchors.
    backwards = segment(0, START_HOST_NS, 300)
    backwards["anchors"][-1]["mic_host_ns"] = backwards["anchors"][-2]["mic_host_ns"] - 1_000_000
    write("refuse-nonmonotonic", [backwards])


# --- 4. Device-switch fixtures (fixture spec §4) ----------------------------
#
# A boundary is unrecorded wall clock and is never padded (SPEC A5), so what it
# cost has to come out as a number.

GAP_MS = 400


def device_switch_fixtures() -> None:
    def pair(name: str, first_duration_s: float, second_duration_s: float, ppm: float = 0.0) -> None:
        first = segment(0, START_HOST_NS, first_duration_s, mic_ppm=ppm, sys_ppm=ppm)
        second_start = (
            START_HOST_NS
            + int(first_duration_s * 1_000_000_000)
            + GAP_MS * 1_000_000
        )
        second = segment(
            1,
            second_start,
            second_duration_s,
            mic_ppm=ppm,
            sys_ppm=ppm,
            reason=REASON_OUTPUT_DEVICE_CHANGED,
        )
        write(name, [first, second])

    # Seg 0 closes on the checkpoint grid, so its last anchor IS the close
    # anchor and the segment frame total matches it exactly.
    pair("device-switch-400ms", 1200, 1200)

    # The negative fixture. Seg 0 closes 4.2 s after its last ordinary
    # checkpoint, so there is no close anchor. The gap must still read 400 ms.
    no_close = segment(0, START_HOST_NS, 1199.2)
    second_start = START_HOST_NS + 1_199_200_000_000 + GAP_MS * 1_000_000
    write(
        "device-switch-no-close-anchor",
        [no_close, segment(1, second_start, 1200, reason=REASON_OUTPUT_DEVICE_CHANGED)],
    )

    # Two anchors in the whole recording: one per segment, and seg 1 ends 3 s
    # past its only checkpoint.
    pair("device-switch-short-tail", CHECKPOINT_S, 8)

    # Both segments drift at +100 ppm for 1200 s. Each re-anchors to the host
    # clock at its own start, so the gate number is 120 ms (the max over
    # segments) and never 240 ms (the sum).
    pair("device-switch-drift-reset", 1200, 1200, ppm=100.0)


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    drift_fixtures()
    refusal_fixtures()
    device_switch_fixtures()
    written = sorted(p.parent.name for p in OUT.glob("*/segments.json"))
    print(f"segments/ -> {len(written)} fixtures: {', '.join(written)}")
    if os.environ.get("FIXTURE_WAVS") != "1":
        print("segments/ -> WAVs skipped (set FIXTURE_WAVS=1 for ~1 GB of silence)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
