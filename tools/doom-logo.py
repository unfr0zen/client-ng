#!/usr/bin/env python3
"""Writes Doom's title graphic (the WAD's M_DOOM patch, in PLAYPAL's
colours) as a PNG: doom-logo.py WAD PNG. The art stays in the WAD, which
is never committed; the build makes the picture from it."""
import struct, sys, zlib


def lumps(wad):
    _, count, at = struct.unpack_from("<4sii", wad)
    for i in range(count):
        pos, size, name = struct.unpack_from("<ii8s", wad, at + i * 16)
        yield name.rstrip(b"\0").decode(), wad[pos:pos + size]


def main():
    wad = open(sys.argv[1], "rb").read()
    found = dict(lumps(wad))
    palette, patch = found["PLAYPAL"], found["M_DOOM"]
    w, h = struct.unpack_from("<hh", patch)
    rgba = bytearray(w * h * 4)
    for x in range(w):
        at = struct.unpack_from("<I", patch, 8 + x * 4)[0]
        while patch[at] != 0xFF:
            top, n = patch[at], patch[at + 1]
            for i in range(n):
                c = patch[at + 3 + i]
                o = ((top + i) * w + x) * 4
                rgba[o:o + 4] = bytes(palette[c * 3:c * 3 + 3]) + b"\xff"
            at += n + 4
    raw = b"".join(b"\0" + bytes(rgba[y * w * 4:(y + 1) * w * 4]) for y in range(h))
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)) \
        + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
    open(sys.argv[2], "wb").write(png)
    print(f"{sys.argv[2]}: {w}x{h}")


main()
