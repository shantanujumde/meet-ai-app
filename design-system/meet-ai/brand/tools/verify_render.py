#!/usr/bin/env python3
"""Assert that a screenshot is the page we asked for, not a picture of a failure.

Raised in review (TUR-12): `render-proximity.sh` screenshotted Chrome's own
`ERR_FILE_NOT_FOUND` page and committed it as a proof. Chrome does not exit
non-zero when a `file://` URL is missing — it renders an error page and
screenshots that happily, at exactly the window size that was asked for. So
neither a non-empty check nor a pixel-width check can see the failure.

What actually separates a real proof from an error page is where the ink is.
Chrome's error page is a small text block in the top-left eighth over a flat
field; everything below is empty. Every real proof here is dense top to bottom.
So: sample the lower part of the image and require that a real fraction of it
differs from the page background. That needs no per-page constants and it
catches the whole class — error pages, blank pages, pages whose CSS or images
failed to load.

Decoding is done on a downsample. Unfiltering a 2360x3600 PNG byte-by-byte in
pure Python takes minutes; `sips -Z` reduces it to ~128px first, which
preserves the flat-versus-dense property exactly and costs milliseconds. This
machine has no PIL.

    verify_render.py <png> --expect 2360x3600 [--min-ink 0.04]
"""
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib

SAMPLE = 128  # longest edge of the downsample we actually decode
TOL = 14  # per-channel delta that counts as "not the background"


# Exit codes separate the two kinds of failure, because only one is worth a
# retry. A missing or mis-sized write is Chrome losing its startup race, which
# is transient here. A render that came out the right size but empty is
# deterministic — retrying it five times just burns thirty seconds.
RETRYABLE = 2
FATAL = 3


def die(msg, code=RETRYABLE):
    sys.stderr.write("   verify: %s\n" % msg)
    sys.exit(code)


def decode_png(path):
    """Return (w, h, channels, pixel bytes) for an 8-bit non-interlaced PNG."""
    d = open(path, "rb").read()
    if d[:8] != b"\x89PNG\r\n\x1a\n":
        die("%s is not a PNG" % os.path.basename(path))
    pos, idat, ihdr = 8, [], None
    while pos + 8 <= len(d):
        (ln,) = struct.unpack(">I", d[pos : pos + 4])
        typ = d[pos + 4 : pos + 8]
        body = d[pos + 8 : pos + 8 + ln]
        if typ == b"IHDR":
            ihdr = struct.unpack(">IIBBBBB", body)
        elif typ == b"IDAT":
            idat.append(body)
        elif typ == b"IEND":
            break
        pos += 12 + ln
    if ihdr is None:
        die("no IHDR")
    w, h, depth, ctype, _, _, interlace = ihdr
    if depth != 8 or interlace != 0:
        die("unsupported PNG (depth=%d interlace=%d)" % (depth, interlace))
    nch = {0: 1, 2: 3, 4: 2, 6: 4}.get(ctype)
    if nch is None:
        die("unsupported colour type %d" % ctype)

    raw = zlib.decompress(b"".join(idat))
    stride = w * nch
    prev = bytearray(stride)
    out = bytearray()
    p = 0
    for _ in range(h):
        ft = raw[p]
        p += 1
        line = bytearray(raw[p : p + stride])
        p += stride
        if ft == 1:
            for i in range(nch, stride):
                line[i] = (line[i] + line[i - nch]) & 255
        elif ft == 2:
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 255
        elif ft == 3:
            for i in range(stride):
                a = line[i - nch] if i >= nch else 0
                line[i] = (line[i] + ((a + prev[i]) >> 1)) & 255
        elif ft == 4:
            for i in range(stride):
                a = line[i - nch] if i >= nch else 0
                b = prev[i]
                c = prev[i - nch] if i >= nch else 0
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[i] = (line[i] + pr) & 255
        elif ft != 0:
            die("bad filter type %d" % ft)
        out += line
        prev = line
    return w, h, nch, bytes(out)


def main():
    if len(sys.argv) < 2:
        die("usage: verify_render.py <png> --expect WxH [--min-ink F]")
    png = sys.argv[1]
    expect, min_ink = None, 0.04
    for i, a in enumerate(sys.argv):
        if a == "--expect":
            expect = sys.argv[i + 1]
        elif a == "--min-ink":
            min_ink = float(sys.argv[i + 1])

    if not os.path.isfile(png) or os.path.getsize(png) == 0:
        die("%s missing or empty" % os.path.basename(png))

    name = os.path.basename(png)

    # 1. dimensions — cheap, and catches a truncated or mis-sized write.
    w, h, _, _ = decode_png_header(png)
    if expect:
        ew, eh = (int(v) for v in expect.lower().split("x"))
        if (w, h) != (ew, eh):
            die("%s is %dx%d, expected %dx%d" % (name, w, h, ew, eh))

    # 2. content — the check that sees an error page.
    tmp = tempfile.mkdtemp()
    small = os.path.join(tmp, "s.png")
    try:
        subprocess.run(
            ["sips", "-s", "format", "png", "-Z", str(SAMPLE), png, "--out", small],
            check=True,
            capture_output=True,
        )
        sw, sh, nch, px = decode_png(small)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    def at(x, y):
        o = (y * sw + x) * nch
        return px[o : o + 3] if nch >= 3 else px[o : o + 1] * 3

    # Background is read from the top-right corner: every proof page here has
    # page padding there, and so does Chrome's error page.
    bg = at(sw - 2, 1)
    lo = int(sh * 0.40)  # below Chrome's error text block in every window size
    total = ink = 0
    for y in range(lo, sh):
        for x in range(sw):
            total += 1
            c = at(x, y)
            if max(abs(c[i] - bg[i]) for i in range(3)) > TOL:
                ink += 1
    frac = ink / total if total else 0.0

    if frac < min_ink:
        die(
            "%s looks empty below 40%% height (%.1f%% ink, need %.1f%%) — "
            "Chrome probably rendered an error or blank page"
            % (name, frac * 100, min_ink * 100),
            FATAL,
        )
    sys.stdout.write("   %-26s %dx%d  ink %.0f%%\n" % (name, w, h, frac * 100))


def decode_png_header(path):
    d = open(path, "rb").read(33)
    if d[:8] != b"\x89PNG\r\n\x1a\n":
        die("%s is not a PNG" % os.path.basename(path))
    w, h, depth, ctype, _, _, il = struct.unpack(">IIBBBBB", d[16:29])
    return w, h, depth, ctype


if __name__ == "__main__":
    main()
