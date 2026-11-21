"""Generate the original studio lighting map (no external assets)."""

from math import exp, frexp, pi
from pathlib import Path

WIDTH, HEIGHT = 256, 128
PANELS = [
    (-1.6, 0.46, 0.32, 0.16, (14, 9, 5)),
    (0.52, 0.62, 0.23, 0.25, (6, 10, 18)),
    (2.45, 0.36, 0.50, 0.10, (6, 9, 8)),
    (-0.42, 1.10, 0.76, 0.17, (3, 3.4, 4)),
]


def radiance(phi, elevation):
    color = [0.027, 0.040, 0.065] if elevation >= 0 else [0.018, 0.023, 0.030]
    for center, vertical, dx, dy, hue in PANELS:
        angle = (phi - center + pi) % (2 * pi) - pi
        distance = (angle / dx) ** 4 + ((elevation - vertical) / dy) ** 4
        weight = exp(-distance)
        for i, value in enumerate(hue):
            color[i] += value * weight
    peak = max(color)
    _, exponent = frexp(peak)
    scale = 255 / (2 ** exponent)
    return tuple(round(component * scale) for component in color) + (exponent + 128,)


def make_map(file):
    file.write(f"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y {HEIGHT} +X {WIDTH}\n".encode())
    for y in range(HEIGHT):
        elevation = (0.5 - (y + 0.5) / HEIGHT) * pi
        row = [radiance(((x + 0.5) / WIDTH - 0.5) * 2 * pi, elevation) for x in range(WIDTH)]
        file.write(bytes((2, 2, WIDTH >> 8, WIDTH & 255)))
        for channel in zip(*row):
            for i in range(0, WIDTH, 128):
                packet = channel[i:i + 128]
                file.write(bytes((len(packet), *packet)))


if __name__ == "__main__":
    with Path(__file__).with_name("studio.hdr").open("wb") as output:
        make_map(output)
