#!/usr/bin/env python3
"""Generate packaging/guitar-fx.png (256x256) without third-party libraries."""
import math, struct, zlib, pathlib
N = 256
rows = []
for y in range(N):
    row = bytearray([0])
    for x in range(N):
        dx, dy = x - N / 2 + .5, y - N / 2 + .5
        r = math.hypot(dx, dy)
        # rounded-square background
        qx, qy = abs(dx) - 100, abs(dy) - 100
        d = math.hypot(max(qx, 0), max(qy, 0)) + min(max(qx, qy), 0) - 28
        a = 255 if d < 0 else 0
        col = (24, 27, 36)
        # orange ring
        if 70 < r < 86: col = (255, 150, 40)
        # waveform inside the ring
        wy = 38 * math.sin((dx + 64) / 128 * 2 * math.pi * 1.5) * (1 - (dx / 70) ** 2) if abs(dx) < 70 else 99
        if abs(dy - wy) < 5 and abs(dx) < 66: col = (120, 220, 255)
        row += bytes([*col, a])
    rows.append(bytes(row))
raw = b"".join(rows)
def chunk(t, d): return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", N, N, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
pathlib.Path(__file__).with_name("guitar-fx.png").write_bytes(png)
