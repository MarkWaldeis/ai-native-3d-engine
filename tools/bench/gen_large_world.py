#!/usr/bin/env python3
"""Generate the deterministic, unstreamed 44 km² web-player baseline."""
import argparse
from collections import Counter
import json
import math
from pathlib import Path
import random

WORLD_SIZE = 6633.0
GRID = 64
TILE = WORLD_SIZE / GRID


def generate(seed=1, density=1.0):
    rng = random.Random(seed)
    phases = [rng.uniform(-math.pi, math.pi) for _ in range(3)]

    def height(x, z):
        return 32 + 17 * math.sin(x / 930 + phases[0]) * math.cos(z / 1100 + phases[1]) + 10 * math.sin(x / 510 + z / 730 + phases[2])

    def ground(x, z):
        ix = min(GRID - 1, max(0, int((x + WORLD_SIZE / 2) / TILE)))
        iz = min(GRID - 1, max(0, int((z + WORLD_SIZE / 2) / TILE)))
        return height((ix + .5) * TILE - WORLD_SIZE / 2, (iz + .5) * TILE - WORLD_SIZE / 2)

    entities = []

    def add(name, kind, position, color, **fields):
        entity = dict(id=len(entities) + 1, name=name, kind=kind,
                      transform=dict(position=[round(v, 5) for v in position]), color=color, **fields)
        entities.append(entity)
        return entity

    for iz in range(GRID):
        for ix in range(GRID):
            x, z = (ix + .5) * TILE - WORLD_SIZE / 2, (iz + .5) * TILE - WORLD_SIZE / 2
            top = height(x, z)
            add(f"ground-{ix}-{iz}", "box", [x, (top - 8) / 2, z],
                [rng.uniform(.18, .32), rng.uniform(.30, .46), rng.uniform(.10, .19), 1],
                size=[TILE, top + 8, TILE], tags=["ground"],
                collider={"shape": "box"}, rigidbody={"type": "static"})

    def point():
        return rng.uniform(-WORLD_SIZE / 2 + 12, WORLD_SIZE / 2 - 12), rng.uniform(-WORLD_SIZE / 2 + 12, WORLD_SIZE / 2 - 12)

    for i in range(round(4000 * density)):
        x, z = point()
        y, trunk = ground(x, z), rng.uniform(4, 8)
        add(f"trunk-{i}", "cylinder", [x, y + trunk / 2, z], [.28, .16, .07, 1], radius=.65, height=trunk, segments=6)
        add(f"crown-{i}", "sphere", [x, y + trunk + 1.5, z], [.10, rng.uniform(.25, .45), .12, 1], radius=rng.uniform(2.5, 4.5), segments=8)
    for i in range(round(2400 * density)):
        x, z = point()
        radius = rng.uniform(1, 4)
        fields = dict(radius=radius, segments=8) if i % 2 else dict(size=[radius * 2, radius * 1.5, radius * 2])
        add(f"rock-{i}", "sphere" if i % 2 else "box", [x, ground(x, z) + radius * (1 if i % 2 else .75), z], [.38, .36, .32, 1], **fields)
    villages = [(-2400, -1900), (-1700, 2100), (0, -800), (1900, -2100), (2300, 1800), (100, 2600)]
    for i in range(round(500 * density)):
        vx, vz = villages[i % len(villages)]
        x, z = vx + rng.uniform(-240, 240), vz + rng.uniform(-240, 240)
        h = rng.uniform(4, 16)
        add(f"building-{i}", "box", [x, ground(x, z) + h / 2, z], [.48, .43, .35, 1], size=[rng.uniform(8, 20), h, rng.uniform(8, 20)])

    script = '''fn terrain(x, z) {
    let tx = ((x + 3316.5) / 103.640625).floor() * 103.640625 - 3264.6796875;
    let tz = ((z + 3316.5) / 103.640625).floor() * 103.640625 - 3264.6796875;
    return 32.0 + 17.0 * sin(tx / 930.0 + PHASE0) * cos(tz / 1100.0 + PHASE1) + 10.0 * sin(tx / 510.0 + tz / 730.0 + PHASE2);
}
fn on_start(ctx) { ctx.state_set("_distance", 0.0); }
fn on_tick(ctx, dt) {
    let d = (ctx.state_get("_distance") + 56.56854249 * dt) % 10400.0;
    ctx.state_set("_distance", d);
    let x = -2600.0; let z = -2600.0;
    if d < 5200.0 { x += d; } else { x = 7800.0 - d; }
    z = x;
    ctx.set_position([x, terrain(x, z) + 8.0, z]);
}'''
    for i, phase in enumerate(phases):
        script = script.replace(f"PHASE{i}", repr(phase))
    start_height = ground(-2600, -2600) + 8
    add("benchmark-camera-target", "sphere", [-2600, start_height, -2600], [1, .6, .1, 1],
        radius=1, segments=8, tags=["player"], script={"source": script},
        props={"ai_ui": {"title": "Benchmark 44 km²", "mission": "Deterministic flight · 6633 × 6633 m", "intro_title": "Benchmark 44 km²", "intro_text": "Start the fixed flight tour.", "hud_keys": []}})
    return dict(name="large_world_bench", camera=dict(position=[-2593, start_height + 6, -2591], target=[-2600, start_height, -2600], fov_deg=55), light_dir=[-.4, 1, .5], ambient=.4, entities=entities)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--out", type=Path, required=True, help="Output scene.json path")
    parser.add_argument("--density", type=float, default=1.0)
    args = parser.parse_args()
    if not math.isfinite(args.density) or args.density < 0:
        parser.error("--density must be finite and nonnegative")
    scene = generate(args.seed, args.density)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(scene, separators=(",", ":"), allow_nan=False) + "\n", encoding="utf-8")
    print(f"World: {WORLD_SIZE:g} × {WORLD_SIZE:g} m ({WORLD_SIZE ** 2 / 1e6:.6f} km²)")
    print(f"Entities: {len(scene['entities'])}; kinds: {dict(Counter(e['kind'] for e in scene['entities']))}")
    print(f"Ground colliders: {GRID * GRID}; props: {len(scene['entities']) - GRID * GRID - 1}; player: 1")
    print(f"Scene: {args.out} ({args.out.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
