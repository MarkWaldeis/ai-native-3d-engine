"""Generate tileable slate wall maps for the offline material studio."""

import math
import struct
import zlib
from pathlib import Path

SIZE = 128


def png(pixels):
    def chunk(kind, data):
        return (struct.pack(">I", len(data)) + kind + data
                + struct.pack(">I", zlib.crc32(kind + data)))

    rows = b"".join(b"\0" + bytes(pixels[y * SIZE * 3:(y + 1) * SIZE * 3]) for y in range(SIZE))
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows, level=9))
            + chunk(b"IEND", b""))


def surface(x, y):
    x, y = x % SIZE, y % SIZE
    edge = min(x, y, SIZE - 1 - x, SIZE - 1 - y)
    bevel = max(0, min(1, (edge - 1) / 6))
    bevel = bevel**2 * (3 - 2 * bevel)
    u, v = x / SIZE, y / SIZE
    grain = (0.55 * math.sin(math.tau * (3*u + 2*v))
             + 0.29 * math.sin(math.tau * (10*u - 7*v))
             + 0.16 * math.cos(math.tau * (29*u + 41*v)))
    return bevel, grain


def height(x, y):
    bevel, grain = surface(x, y)
    return bevel * (1 + 0.05 * grain)


def generate():
    color, normal, orm = [], [], []
    for y in range(SIZE):
        for x in range(SIZE):
            bevel, grain = surface(x, y)
            tone = round(38 + bevel * (89 + 12 * grain))
            color.extend((tone - 9, tone, tone + 8))
            nx = 4 * (height(x-1, y) - height(x+1, y))
            ny = 4 * (height(x, y+1) - height(x, y-1))
            length = math.sqrt(nx*nx + ny*ny + 1)
            normal.extend(round(127.5 * (1 + n / length)) for n in (nx, ny, 1))
            orm.extend((round(148 + 107 * bevel), round(214 - bevel * (38 + 19 * grain)), 0))
    for name, pixels in [("color", color), ("normal", normal), ("orm", orm)]:
        Path(__file__).with_name(f"wall-{name}.png").write_bytes(png(pixels))


if __name__ == "__main__":
    generate()
