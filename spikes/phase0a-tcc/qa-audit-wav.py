#!/usr/bin/env python3
"""Tess / QA — independent re-measurement of the Phase 0a spike WAVs.

Reads the RIFF header itself and computes every number from the sample bytes.
Deliberately does NOT read probe-result.json and does NOT shell out to ffmpeg,
so it cannot inherit either meter's mistakes.
"""
import array
import math
import struct
import sys


def read_wav(path):
    with open(path, "rb") as fh:
        blob = fh.read()
    if blob[:4] != b"RIFF" or blob[8:12] != b"WAVE":
        raise ValueError(f"{path}: not a RIFF/WAVE file")
    pos, fmt, data = 12, None, None
    while pos + 8 <= len(blob):
        cid = blob[pos:pos + 4]
        csz = struct.unpack_from("<I", blob, pos + 4)[0]
        body = blob[pos + 8:pos + 8 + csz]
        if cid == b"fmt ":
            fmt = struct.unpack_from("<HHIIHH", body, 0)
        elif cid == b"data":
            data = body
        pos += 8 + csz + (csz & 1)
    if fmt is None or data is None:
        raise ValueError(f"{path}: missing fmt or data chunk")
    tag, ch, rate, _bps, _align, bits = fmt
    if tag != 3 or bits != 32:
        raise ValueError(f"{path}: expected float32 (tag 3/32), got tag {tag}/{bits}")
    samples = array.array("f")
    samples.frombytes(data[:len(data) - len(data) % 4])
    return ch, rate, samples


def goertzel(chan, rate, freq):
    """Single-bin DFT magnitude, scaled to sine amplitude."""
    n = len(chan)
    k = 2.0 * math.cos(2.0 * math.pi * freq / rate)
    s1 = s2 = 0.0
    for x in chan:
        s0 = x + k * s1 - s2
        s2, s1 = s1, s0
    real = s1 - s2 * (k / 2.0)
    imag = s2 * math.sin(2.0 * math.pi * freq / rate)
    return 2.0 * math.sqrt(real * real + imag * imag) / n


def audit(path):
    ch, rate, s = read_wav(path)
    frames = len(s) // ch
    total = len(s)
    acc = 0.0
    peak = 0.0
    zeros = 0
    for v in s:
        acc += v * v
        a = abs(v)
        if a > peak:
            peak = a
        if v == 0.0:
            zeros += 1
    rms = math.sqrt(acc / total) if total else 0.0
    print(f"\n=== {path} ===")
    print(f"format          : {rate} Hz, {ch} ch, float32")
    print(f"frames          : {frames}   (samples {total}, {frames / rate:.3f} s)")
    print(f"RMS             : {rms:.6f}" + (f"  ({20 * math.log10(rms):.2f} dBFS)" if rms > 0 else "  (-inf dBFS)"))
    print(f"peak            : {peak:.6f}")
    print(f"zero samples    : {zeros}/{total}  fraction {zeros / total if total else 0:.9f}")
    if rms == 0.0:
        print("verdict         : DIGITAL SILENCE (every sample bit-exact zero)")
        return
    # one-second slice from the middle, per channel, at the expected tone bins
    start = (frames // 2) * ch
    span = min(rate, frames - frames // 2)
    for c in range(min(ch, 2)):
        chan = s[start + c: start + (span * ch) + c: ch]
        crms = math.sqrt(sum(v * v for v in chan) / len(chan))
        bins = {f: goertzel(chan, rate, f) for f in (440, 660, 1000)}
        name = "L" if c == 0 else "R"
        print(f"  {name} rms={crms:.5f}  " + "  ".join(f"@{f}Hz={a:.5f}" for f, a in bins.items()))
    print("verdict         : NON-SILENT")


if __name__ == "__main__":
    for p in sys.argv[1:]:
        try:
            audit(p)
        except Exception as exc:  # noqa: BLE001 - report and keep going
            print(f"\n=== {p} ===\nERROR: {exc}")
