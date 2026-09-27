#!/usr/bin/env python3
"""How much margin does the tone probe actually have?

FINDINGS §9.1 rests the permission check on a positive control: play a known
tone, confirm it comes back through the tap. A denial is otherwise
indistinguishable from a quiet room — every OSStatus is noErr, the IOProc is
created, callbacks fire at the normal rate, and every sample is a bit-exact
zero (§10.1, and TUR-10 §3 for a real "Don't Allow").

"It comes back" needs a threshold and a window length, and neither is in the
harness. This measures both, so the constants in the Rust probe are chosen from
a number rather than from taste.

    ./tone-probe-margin.py /tmp/meet-ai-tur10

Reads <root>/<run>/system.wav and <root>/<run>/tone.wav for each run present.
Exits non-zero if a granted run fails the recommended threshold or a denied run
clears it — so this doubles as the standing regression check on the probe.

Deliberately dependency-free: no numpy on the QA machine, and a single Goertzel
bin is a handful of lines anyway.
"""

import array
import math
import pathlib
import struct
import sys

# The probe's tone. Confirmed against tone.wav rather than assumed.
TONE_HZ = 440.0

# Recommended probe constants, from the margins this script prints.
#
# WINDOW_MS/THRESHOLD: at 50 ms the granted floor was 0.447 and off-tone leakage
# topped out at 0.061; denied was exactly 0.0. 0.2 sits clear of both with room
# for a quieter output device than the one measured.
WINDOW_MS = 50
THRESHOLD = 0.2

# ONSET_TIMEOUT_MS: the probe must POLL for the tone, not sample once and judge.
# On the granted run the tone was present from the first frame, but only because
# create_ioproc had blocked 1393 ms behind the consent dialog while the tone was
# already playing. On the rebuilt run — no dialog, ~45 ms of warm tap setup —
# the 440 Hz bin did not clear 0.2 until t=1.07 s. That is the FAST path, so it
# is the common one, and a probe that checks 50 ms after starting the tone would
# read denied on a perfectly granted system.
#
# Note the trap: the first 1.07 s is not silent. It measures -24 dBFS with peak
# 0.41 and only one bit-exact zero sample, so a plain "is any audio flowing?"
# check passes there. Only the tone-matched bin tells the truth, and only after
# the onset. 2000 ms is ~2x the measured worst case.
ONSET_TIMEOUT_MS = 2000


def load_wav(path):
    """Minimal RIFF reader. Returns (left_channel_samples, sample_rate).

    Walks the chunks rather than assuming a 44-byte header: the probe writes a
    float32 WAV, whose fmt chunk is larger than the classic PCM one.
    """
    raw = path.read_bytes()
    if raw[0:4] != b"RIFF" or raw[8:12] != b"WAVE":
        raise ValueError(f"{path} is not a RIFF/WAVE file")

    fmt = data = None
    pos = 12
    while pos + 8 <= len(raw):
        chunk_id = raw[pos : pos + 4]
        size = struct.unpack("<I", raw[pos + 4 : pos + 8])[0]
        body = raw[pos + 8 : pos + 8 + size]
        if chunk_id == b"fmt ":
            fmt = struct.unpack("<HHIIHH", body[:16])
        elif chunk_id == b"data":
            data = body
        pos += 8 + size + (size & 1)  # chunks are word-aligned

    if fmt is None or data is None:
        raise ValueError(f"{path} is missing a fmt or data chunk")

    tag, channels, rate, _byte_rate, _align, bits = fmt
    if tag != 3 or bits != 32:
        raise ValueError(f"{path}: expected float32 (tag 3), got tag {tag}/{bits} bit")

    samples = array.array("f")
    samples.frombytes(data[: len(data) // 4 * 4])
    return samples[0::channels], rate


def goertzel(samples, rate, freq):
    """Magnitude of a single DFT bin, normalised so a full-scale sine reads 0.5."""
    coeff = 2.0 * math.cos(2.0 * math.pi * freq / rate)
    s1 = s2 = 0.0
    for value in samples:
        s1, s2 = value + coeff * s1 - s2, s1
    return math.sqrt(s1 * s1 + s2 * s2 - coeff * s1 * s2) / (len(samples) / 2)


def dominant_frequency(samples, rate):
    """Coarse-then-fine scan, used to confirm tone.wav is the tone we think."""
    middle = samples[len(samples) // 2 : len(samples) // 2 + int(0.2 * rate)]
    coarse = max(range(200, 4001, 20), key=lambda f: goertzel(middle, rate, f))
    return max(range(coarse - 20, coarse + 21), key=lambda f: goertzel(middle, rate, f))


def onset_seconds(capture, rate, freq, threshold, window_ms=10):
    """When the tone first clears `threshold`, or None if it never does.

    This is what the real probe does: poll until the tone shows up. Measured in
    10 ms steps regardless of WINDOW_MS, so the answer is a property of the
    audio rather than of the probe's chosen window.
    """
    width = int(rate * window_ms / 1000)
    for start in range(0, len(capture) - width, width):
        if goertzel(capture[start : start + width], rate, freq) >= threshold:
            return start / rate
    return None


def measure(capture, rate, freq, window_ms, skip_seconds):
    """Tone-bin and off-tone-bin magnitudes over non-overlapping windows.

    Starts after `skip_seconds` — the tone's onset — and stops a second before
    the end, since the tone stops before the capture does. Windowing across the
    onset would report the warm-up as a weak tone rather than as absent, which
    is the distinction the whole probe rests on.
    """
    width = int(rate * window_ms / 1000)
    # A bin the tone does not occupy and which is not one of its harmonics.
    control_hz = freq * 1.77 + 31
    first = int(skip_seconds * rate)
    last = max(first, len(capture) - rate) - width
    tone_bins, control_bins = [], []
    for start in range(first, last, width):
        window = capture[start : start + width]
        tone_bins.append(goertzel(window, rate, freq))
        control_bins.append(goertzel(window, rate, control_hz))
    return tone_bins, control_bins


def main():
    root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "/tmp/meet-ai-tur10")
    if not root.is_dir():
        sys.exit(f"no such directory: {root} — run ./verify-tur10.sh first")

    failures = []
    for run in ("granted", "rebuilt", "denied"):
        capture_path = root / run / "system.wav"
        if not capture_path.exists():
            print(f"{run}: no system.wav, skipping")
            continue

        capture, rate = load_wav(capture_path)
        freq = TONE_HZ
        tone_path = root / run / "tone.wav"
        if tone_path.exists():
            tone, tone_rate = load_wav(tone_path)
            found = dominant_frequency(tone, tone_rate)
            if abs(found - TONE_HZ) > 1:
                print(f"{run}: tone.wav is {found} Hz, not {TONE_HZ:.0f} Hz — measuring {found}")
                freq = found

        onset = onset_seconds(capture, rate, freq, THRESHOLD)
        shown = "never" if onset is None else f"{onset * 1000:.0f} ms"
        print(f"\n{run} — {freq:.0f} Hz bin vs an off-tone control bin (tone onset: {shown})")

        if run == "denied":
            if onset is not None:
                failures.append(f"denied run found the tone at {shown} — expected never")
            onset = 0.0
        elif onset is None:
            failures.append(f"{run} run never produced the tone")
            continue
        elif onset * 1000 > ONSET_TIMEOUT_MS:
            failures.append(
                f"{run} tone onset {shown} exceeds the {ONSET_TIMEOUT_MS} ms probe timeout"
            )

        print(f"  {'window':>8}  {'tone min':>10}  {'tone med':>10}  {'control max':>12}")
        for window_ms in (20, 50, 100, 200, 500):
            tone_bins, control_bins = measure(capture, rate, freq, window_ms, onset)
            if not tone_bins:
                continue
            low = min(tone_bins)
            median = sorted(tone_bins)[len(tone_bins) // 2]
            print(f"  {window_ms:5d} ms  {low:10.6f}  {median:10.6f}  {max(control_bins):12.6f}")

            if window_ms != WINDOW_MS:
                continue
            # The recommended window is the one that has to hold.
            if run == "denied" and low > 0:
                failures.append(f"denied run shows {low:.6f} in the tone bin — expected exactly 0")
            elif run != "denied" and low < THRESHOLD:
                failures.append(
                    f"{run} run floor {low:.6f} is under the {THRESHOLD} threshold after onset"
                )

    print()
    if failures:
        for failure in failures:
            print(f"FAIL: {failure}")
        sys.exit(1)
    print(
        f"PASS — the tone arrives within {ONSET_TIMEOUT_MS} ms, then holds above "
        f"{THRESHOLD} in every {WINDOW_MS} ms window; denied never shows it at all."
    )


if __name__ == "__main__":
    main()
