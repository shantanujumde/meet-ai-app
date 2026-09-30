#!/usr/bin/env python3
"""Small file checks for the TUR-97 kill gate (python3 stdlib only).

Each subcommand prints one line of JSON on stdout and exits 0 when the check
passes, 1 when it fails. gate.sh turns that into its ok/FAIL lines and keeps
the JSON as evidence.

  check.py wav <file.wav> [--expect-seconds N]
      Walks the RIFF chunks (does not assume a 44-byte header) and reports
      what the header declares versus what is actually on disk.
  check.py segments <segments.json>
      Parses segments.json and counts segments and checkpoint anchors.
  check.py words <meet-stt-output.jsonl> <sentence>
      Collects the "text" of every meet-stt result line and counts how many
      of the sentence's content words came back.
  check.py logscan <log file> <byte offset>
      Prints ERROR / panic lines written after <byte offset>.
"""

import json
import re
import struct
import sys

RATE = 16000
CHANNELS = 1
BITS = 16
BYTES_PER_SECOND = RATE * CHANNELS * BITS // 8

# Largest amount of recorded audio a kill may lose: one 5 s checkpoint
# interval, plus a second of slack for the time between the last checkpoint
# write and the kill landing.
MAX_LOSS_S = 6.0


def emit(ok, **fields):
    fields["ok"] = ok
    print(json.dumps(fields, sort_keys=True))
    return 0 if ok else 1


def parse_wav(path):
    """Return a dict describing the header and the file, or raise ValueError."""
    with open(path, "rb") as f:
        data = f.read()
    file_len = len(data)
    if file_len < 12:
        raise ValueError(f"file is {file_len} bytes, too short for a RIFF header")
    if data[0:4] != b"RIFF" or data[8:12] != b"WAVE":
        raise ValueError(f"not a RIFF/WAVE file (starts {data[0:12]!r})")
    riff_size = struct.unpack_from("<I", data, 4)[0]

    fmt = None
    data_offset = None
    data_size = None
    pos = 12
    chunks = []
    while pos + 8 <= file_len:
        cid = data[pos:pos + 4]
        csize = struct.unpack_from("<I", data, pos + 4)[0]
        chunks.append({"id": cid.decode("latin-1"), "offset": pos, "size": csize})
        body = pos + 8
        if cid == b"fmt ":
            if csize < 16 or body + 16 > file_len:
                raise ValueError("fmt chunk is truncated")
            tag, ch, rate, byte_rate, align, bits = struct.unpack_from("<HHIIHH", data, body)
            fmt = {"format_tag": tag, "channels": ch, "rate": rate,
                   "byte_rate": byte_rate, "block_align": align, "bits": bits}
        elif cid == b"data":
            data_offset = body
            data_size = csize
            # data is the last chunk meet-ai writes; whatever follows is PCM
            # the header may or may not account for.
            break
        pos = body + csize + (csize & 1)

    if fmt is None:
        raise ValueError("no fmt chunk before the data chunk")
    if data_offset is None:
        raise ValueError("no data chunk")

    on_disk = file_len - data_offset
    return {
        "file_len": file_len,
        "riff_size": riff_size,
        "fmt": fmt,
        "chunks": chunks,
        "data_offset": data_offset,
        "data_size_declared": data_size,
        "pcm_bytes_on_disk": on_disk,
        "declared_s": round(data_size / BYTES_PER_SECOND, 3),
        "on_disk_s": round(on_disk / BYTES_PER_SECOND, 3),
        "beyond_declared_s": round(max(0, on_disk - data_size) / BYTES_PER_SECOND, 3),
    }


def cmd_wav(args):
    path = args[0]
    expect = None
    if "--expect-seconds" in args:
        expect = float(args[args.index("--expect-seconds") + 1])
    try:
        info = parse_wav(path)
    except (OSError, ValueError) as e:
        return emit(False, path=path, header_ok=False, duration_ok=False, tail_ok=False,
                    reason=f"does not parse: {e}")

    fmt = info["fmt"]
    problems = []
    if (fmt["format_tag"], fmt["channels"], fmt["rate"], fmt["bits"]) != (1, CHANNELS, RATE, BITS):
        problems.append(f"format is tag={fmt['format_tag']} ch={fmt['channels']} "
                        f"rate={fmt['rate']} bits={fmt['bits']}, expected PCM 16 kHz mono 16-bit")
    # What the header promises must hang together: RIFF size covers exactly
    # the chunks up to the end of the declared data...
    expected_riff = info["data_offset"] + info["data_size_declared"] - 8
    if info["riff_size"] != expected_riff:
        problems.append(f"RIFF size {info['riff_size']} != data_offset+data_size-8 = {expected_riff}")
    # ...and must not promise more audio than the file holds.
    if info["data_size_declared"] > info["pcm_bytes_on_disk"]:
        problems.append(f"data size {info['data_size_declared']} is more than the "
                        f"{info['pcm_bytes_on_disk']} bytes on disk")
    if info["data_size_declared"] % 2:
        problems.append("data size is odd (not whole 16-bit samples)")
    info["header_ok"] = not problems
    info["header_reason"] = "; ".join(problems) or (
        f"RIFF {info['riff_size']}, data {info['data_size_declared']} of "
        f"{info['pcm_bytes_on_disk']} bytes on disk")

    # Check 4: audio on disk that the header does not declare is audio a
    # player will never play.
    info["tail_ok"] = info["beyond_declared_s"] < MAX_LOSS_S
    info["tail_reason"] = (f"{info['beyond_declared_s']:.1f}s of PCM beyond the declared data "
                           f"(declares {info['declared_s']:.1f}s, disk holds {info['on_disk_s']:.1f}s)")

    # Check 3: the declared duration is at most one checkpoint behind the
    # time we actually recorded for.
    if expect is None:
        info["duration_ok"] = None
        info["duration_reason"] = "no expected duration given"
    else:
        info["expect_s"] = expect
        info["duration_ok"] = info["declared_s"] >= expect - MAX_LOSS_S
        info["duration_reason"] = (f"declares {info['declared_s']:.1f}s, recorded ~{expect:.0f}s "
                                   f"(needs >= {expect - MAX_LOSS_S:.0f}s)")

    ok = info["header_ok"] and info["tail_ok"] and info["duration_ok"] is not False
    return emit(ok, path=path, **info)


def cmd_segments(args):
    path = args[0]
    try:
        with open(path) as f:
            doc = json.load(f)
    except FileNotFoundError:
        return emit(False, path=path, reason="segments.json does not exist")
    except (OSError, ValueError) as e:
        return emit(False, path=path, reason=f"segments.json does not parse: {e}")
    segs = doc.get("segments") if isinstance(doc, dict) else None
    if not isinstance(segs, list):
        return emit(False, path=path, reason="no \"segments\" list in segments.json")
    anchors = sum(len(s.get("anchors") or []) for s in segs if isinstance(s, dict))
    mic = sum(int(s.get("mic_frames") or 0) for s in segs if isinstance(s, dict))
    sys_ = sum(int(s.get("sys_frames") or 0) for s in segs if isinstance(s, dict))
    reason = (f"{len(segs)} segment(s), {anchors} anchor(s), "
              f"mic {mic / RATE:.1f}s, system {sys_ / RATE:.1f}s")
    if not segs:
        reason = "segments.json parses but has no segments"
    return emit(bool(segs), path=path, segments=len(segs), anchors=anchors,
                mic_frames=mic, sys_frames=sys_, version=doc.get("version"), reason=reason)


STOPWORDS = {"the", "a", "an", "and", "of", "to", "is", "it", "in", "on", "over",
             "while", "this", "that", "for", "with", "as", "at", "by", "be"}


def words(text):
    return [w for w in re.findall(r"[a-z']+", text.lower()) if w not in STOPWORDS]


def cmd_words(args):
    path, sentence = args[0], args[1]
    wanted = sorted(set(words(sentence)))
    texts = []
    errors = []
    try:
        with open(path) as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    obj = json.loads(line)
                except ValueError:
                    continue
                if obj.get("type") == "error" or "code" in obj and "text" not in obj:
                    errors.append(obj.get("message") or obj.get("code") or line)
                if isinstance(obj.get("text"), str):
                    texts.append(obj["text"])
    except OSError as e:
        return emit(False, reason=f"no meet-stt output: {e}")
    heard = set(words(" ".join(texts)))
    found = [w for w in wanted if w in heard]
    need = max(3, len(wanted) // 2)
    ok = len(found) >= need
    transcript = " ".join(t.strip() for t in texts)
    if ok:
        reason = f"{len(found)}/{len(wanted)} sentence words heard ({', '.join(found)})"
    elif not texts:
        reason = "meet-stt returned no text" + (f" (error: {errors[0]})" if errors else "")
    else:
        reason = (f"only {len(found)}/{len(wanted)} sentence words heard, need {need}; "
                  f"transcript: {transcript[:120]!r}")
    return emit(ok, found=found, wanted=wanted, transcript=transcript, reason=reason)


def cmd_logscan(args):
    path, offset = args[0], int(args[1])
    try:
        with open(path, "rb") as f:
            f.seek(0, 2)
            size = f.tell()
            # A log that shrank was rotated or truncated; read it all.
            f.seek(offset if offset <= size else 0)
            new = f.read().decode("utf-8", "replace")
    except FileNotFoundError:
        return emit(True, lines=[], new_lines=0, reason="app log does not exist (nothing logged)")
    lines = new.splitlines()
    bad = [l for l in lines if re.search(r"\[ERROR\]|\bpanic|panicked", l, re.IGNORECASE)]
    reason = (f"{len(bad)} ERROR/panic line(s) in {len(lines)} new log line(s)"
              + (f"; first: {bad[0][:160]}" if bad else ""))
    return emit(not bad, lines=bad, new_lines=len(lines), reason=reason)


def main():
    if len(sys.argv) < 3:
        print(__doc__, file=sys.stderr)
        return 2
    cmd, args = sys.argv[1], sys.argv[2:]
    handlers = {"wav": cmd_wav, "segments": cmd_segments, "words": cmd_words,
                "logscan": cmd_logscan}
    if cmd not in handlers:
        print(__doc__, file=sys.stderr)
        return 2
    return handlers[cmd](args)


if __name__ == "__main__":
    sys.exit(main())
