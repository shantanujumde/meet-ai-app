#!/usr/bin/env python3
"""Make the Windows and Linux tray icons from the macOS template glyph (TUR-58).

The template (`src-tauri/icons/meet-aiTemplate@2x.png`) is black with an alpha
channel; macOS recolours it, Windows and Linux do not. So each variant here is
the same glyph and the same alpha, filled with one existing colour: the brand
palette (`design-system/meet-ai/brand/tools/geometry.mjs`) for idle, and the
app's recording red for recording. No new artwork.

Standard library only (zlib), so it runs on any machine with Python 3:

    python3 scripts/make-tray-icons.py
"""

import struct
import zlib
from pathlib import Path

ICONS = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"
SOURCE = ICONS / "meet-aiTemplate@2x.png"

# name -> fill colour
VARIANTS = {
    "tray-chalk.png": (0xF4, 0xF5, 0xF7),  # idle, dark Windows taskbar
    "tray-ink.png": (0x16, 0x18, 0x1D),  # idle, light Windows taskbar
    "tray-ember.png": (0xFF, 0x8A, 0x3C),  # idle on Linux: readable on light and dark panels
    # recording, every non-macOS tray: --status-recording, i.e. --sys-red-dark
    # #ff453a (design-system/meet-ai/tokens.css), the dark variant since the
    # Windows 11 default taskbar and most Linux panels are dark
    "tray-recording.png": (0xFF, 0x45, 0x3A),
}


def read_rgba(path: Path) -> tuple[int, int, list[bytearray]]:
    data = path.read_bytes()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
    pos, idat, width, height = 8, b"", 0, 0
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        kind = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + length]
        if kind == b"IHDR":
            width, height, depth, colour, _, _, interlace = struct.unpack(">IIBBBBB", body)
            assert (depth, colour, interlace) == (8, 6, 0), "expected 8-bit RGBA, not interlaced"
        elif kind == b"IDAT":
            idat += body
        pos += 12 + length
    raw = zlib.decompress(idat)
    stride, bpp = width * 4, 4
    rows, prev = [], bytearray(stride)
    for y in range(height):
        start = y * (stride + 1)
        kind, line = raw[start], bytearray(raw[start + 1 : start + 1 + stride])
        for x in range(stride):
            a = line[x - bpp] if x >= bpp else 0
            b = prev[x]
            c = prev[x - bpp] if x >= bpp else 0
            if kind == 1:
                line[x] = (line[x] + a) & 0xFF
            elif kind == 2:
                line[x] = (line[x] + b) & 0xFF
            elif kind == 3:
                line[x] = (line[x] + (a + b) // 2) & 0xFF
            elif kind == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pred = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[x] = (line[x] + pred) & 0xFF
        rows.append(line)
        prev = line
    return width, height, rows


def write_rgba(path: Path, width: int, height: int, rows: list[bytearray]) -> None:
    def chunk(kind: bytes, body: bytes) -> bytes:
        crc = zlib.crc32(kind + body) & 0xFFFFFFFF
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", crc)

    raw = b"".join(b"\x00" + bytes(row) for row in rows)
    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw, 9))
    png += chunk(b"IEND", b"")
    path.write_bytes(png)


def main() -> None:
    width, height, rows = read_rgba(SOURCE)
    for name, (r, g, b) in VARIANTS.items():
        out = []
        for row in rows:
            line = bytearray(row)
            for x in range(0, len(line), 4):
                line[x : x + 3] = bytes((r, g, b))
            out.append(line)
        write_rgba(ICONS / name, width, height, out)
        print(f"wrote {ICONS / name}")


if __name__ == "__main__":
    main()
