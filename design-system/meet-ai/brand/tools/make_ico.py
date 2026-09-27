#!/usr/bin/env python3
"""Build a multi-resolution .ico from PNGs.

Windows Vista and later accept PNG-compressed entries inside the ICO
container, so each source PNG is embedded verbatim — no BMP conversion, no
palette quantisation, alpha preserved.

    make_ico.py out.ico 16:a.png 32:b.png ...
"""
import struct
import sys


def main(argv):
    out = argv[1]
    entries = []
    for spec in argv[2:]:
        size, path = spec.split(":", 1)
        with open(path, "rb") as fh:
            entries.append((int(size), fh.read()))
    entries.sort(key=lambda e: e[0])

    header = struct.pack("<HHH", 0, 1, len(entries))  # reserved, type=icon, count
    offset = len(header) + 16 * len(entries)
    directory, blobs = b"", b""
    for size, blob in entries:
        dim = 0 if size >= 256 else size  # 0 means 256 in the ICO directory
        directory += struct.pack(
            "<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset
        )
        blobs += blob
        offset += len(blob)

    with open(out, "wb") as fh:
        fh.write(header + directory + blobs)
    print(f"{out}: {len(entries)} sizes, {len(header) + len(directory) + len(blobs)} bytes")


if __name__ == "__main__":
    main(sys.argv)
