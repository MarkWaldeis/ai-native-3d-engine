"""Analytic height field of the Rio Verde map.

Mirrored 1:1 in examples/open_world_shooter/scripts/*.rhai (fn ground_height)
as the fallback when ctx.terrain_height is unavailable. Keep both in sync.
"""
import math

HALF = 3316.625  # sqrt(44e6) / 2 -> 44 km²
WATER_Y = 0.6

# (x, z, radius, level, falloff)
PLATEAUS = [
    (-2700.0, -2700.0, 70.0, 30.0, 160.0),   # LZ Echo
    (-300.0, -2100.0, 90.0, 26.0, 180.0),    # Outpost Alpha
    (900.0, -400.0, 380.0, 24.0, 260.0),     # San Aurelio (ruined town)
    (-1700.0, 1200.0, 90.0, 40.0, 200.0),    # Outpost Bravo (radio tower)
    (2200.0, -1900.0, 90.0, 34.0, 180.0),    # Outpost Charlie
    (2200.0, 2200.0, 210.0, 78.0, 300.0),    # Citadel mesa
]


def smoothstep(e0, e1, x):
    t = min(1.0, max(0.0, (x - e0) / (e1 - e0)))
    return t * t * (3.0 - 2.0 * t)


def river_x(z):
    return -900.0 + 420.0 * math.sin(z / 900.0) + 160.0 * math.sin(z / 370.0 + 1.0)


def height(x, z):
    h = (32.0 + 16.0 * math.sin(x / 760.0 + 0.7) * math.cos(z / 910.0 - 0.3)
         + 8.0 * math.sin((x - z) / 430.0 + 2.1)
         + 3.5 * math.sin(x / 131.0 + 1.0) * math.sin(z / 157.0 + 0.4)
         + 1.2 * math.sin(x / 37.0) * math.sin(z / 41.0))
    for cx, cz, r, level, fall in PLATEAUS:
        d = math.hypot(x - cx, z - cz)
        if d < r + fall:
            w = smoothstep(r + fall, r, d)
            h = h + (level - h) * w
    d = abs(x - river_x(z))
    if d < 200.0:
        w = smoothstep(200.0, 22.0, d)
        h = h + (-3.0 - h) * w
    edge = max(abs(x), abs(z))
    if edge > HALF - 520.0:
        h += 110.0 * smoothstep(HALF - 520.0, HALF, edge)
    return h
