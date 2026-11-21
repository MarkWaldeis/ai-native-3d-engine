#!/usr/bin/env python3
"""Deterministic generator for the Rio Verde open-world shooter (44 km²).

Writes examples/open_world_shooter/{scene.json, terrain/, terrain_fallback/}.
Run from the repo root:  python3 tools/open_world_shooter/gen_world.py
"""
import argparse
import json
import math
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from terrain_fn import HALF, WATER_Y, height, river_x  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
GAME = "examples/open_world_shooter"
ASSETS = f"{GAME}/assets"
TILE_M = 256.0
SAMPLES = 33
FALLBACK_CHUNK = 512.0
FALLBACK_SAMPLES = 33

LZ = (-2700.0, -2700.0)
ALPHA = (-300.0, -2100.0)
TOWN = (900.0, -400.0)
BRAVO = (-1700.0, 1200.0)
CHARLIE = (2200.0, -1900.0)
CITADEL = (2200.0, 2200.0)
BRIDGE_S_Z = -1700.0
BRIDGE_N_Z = 900.0


def bridge_point(z):
    return (river_x(z), z)


ROADS = [
    [LZ, (-2350, -2450), (-1900, -1950), (bridge_point(BRIDGE_S_Z)[0] - 70, BRIDGE_S_Z),
     (bridge_point(BRIDGE_S_Z)[0] + 70, BRIDGE_S_Z), (-800, -1850), ALPHA],
    [ALPHA, (100, -1500), (500, -900), TOWN],
    [TOWN, (1500, -900), (1900, -1500), CHARLIE],
    [TOWN, (1300, 300), (1700, 1100), (2000, 1700), (2200, 1960)],
    [TOWN, (300, 300), (-300, 800), (bridge_point(BRIDGE_N_Z)[0] + 70, BRIDGE_N_Z),
     (bridge_point(BRIDGE_N_Z)[0] - 70, BRIDGE_N_Z), (-1100, 1000), BRAVO],
]


def load_manifest():
    path = ROOT / ASSETS / "manifest.json"
    return json.loads(path.read_text()) if path.exists() else {}


class Scene:
    def __init__(self, manifest):
        self.entities = []
        self.manifest = manifest

    def add(self, name, kind, pos, **fields):
        transform = {"position": [round(v, 3) for v in pos]}
        for key in ("rotation_deg", "scale"):
            if key in fields:
                transform[key] = [round(v, 3) for v in fields.pop(key)]
        e = {"id": len(self.entities) + 1, "name": name, "kind": kind, "transform": transform}
        e.update(fields)
        self.entities.append(e)
        return e

    def asset(self, name, label, x, z, yaw=0.0, scale=1.0, y=None, lift=0.0, collider=None, tags=None, **fields):
        info = self.manifest.get(label, {})
        path = info.get("high", f"{ASSETS}/{label}.glb")
        props = fields.pop("props", {})
        if info.get("low"):
            props["web_lods"] = {"1": info["low"], "2": info["low"]}
        e = self.add(name, "gltf", [x, (height(x, z) if y is None else y) + lift, z], path=path,
                     rotation_deg=[0.0, yaw, 0.0], scale=[scale] * 3, **fields)
        if props:
            e["props"] = props
        if tags:
            e["tags"] = tags
        if collider:
            e["collider"] = collider
            e.setdefault("rigidbody", {"type": "static"})
        return e


def seg_dist(px, pz, a, b):
    ax, az = a
    bx, bz = b
    dx, dz = bx - ax, bz - az
    t = max(0.0, min(1.0, ((px - ax) * dx + (pz - az) * dz) / (dx * dx + dz * dz)))
    return math.hypot(px - ax - t * dx, pz - az - t * dz)


def road_dist(x, z):
    return min(seg_dist(x, z, r[i], r[i + 1]) for r in ROADS for i in range(len(r) - 1))


# ---------------------------------------------------------------- terrain files
def write_terrain(out):
    out.mkdir(parents=True, exist_ok=True)
    grid = math.ceil(2 * HALF / TILE_M)
    spacing = TILE_M / (SAMPLES - 1)
    tiles, lo, hi = [], 1e9, -1e9
    for tz in range(grid):
        for tx in range(grid):
            values = []
            for j in range(-1, SAMPLES + 1):
                for i in range(-1, SAMPLES + 1):
                    gx = (tx * (SAMPLES - 1) + i) * spacing - HALF
                    gz = (tz * (SAMPLES - 1) + j) * spacing - HALF
                    h = height(gx, gz)
                    values.append(h)
                    if 0 <= i < SAMPLES and 0 <= j < SAMPLES:
                        lo, hi = min(lo, h), max(hi, h)
            name = f"tile_{tx}_{tz}.f32"
            (out / name).write_bytes(struct.pack(f"<{len(values)}f", *values))
            tiles.append({"x": tx, "z": tz, "file": name})
    manifest = {"version": 1,
                "config": {"size_m": [2 * HALF, 2 * HALF], "tile_m": TILE_M, "samples": SAMPLES,
                           "height_scale": 0.0, "seed": 7},
                "grid": [grid, grid], "height_range": [round(lo, 3), round(hi, 3)], "tiles": tiles}
    (out / "terrain.json").write_text(json.dumps(manifest, indent=1))


def ground_color(x, z, h, slope):
    if h < WATER_Y + 1.2:
        return (0.42, 0.36, 0.24)  # river mud/sand
    if road_dist(x, z) < 6.0:
        return (0.40, 0.31, 0.20)
    if slope > 0.55:
        return (0.36, 0.34, 0.30)  # rock
    if h > 72.0 and math.hypot(x - CITADEL[0], z - CITADEL[1]) < 260:
        return (0.38, 0.36, 0.28)
    n = 0.5 + 0.5 * math.sin(x / 23.0) * math.sin(z / 29.0)
    return (0.10 + 0.06 * n, 0.24 + 0.08 * n, 0.07 + 0.03 * n)


def glb_bytes(positions, normals, colors, indices):
    def pad(b, fill=b"\0"):
        return b + fill * ((4 - len(b) % 4) % 4)
    pos = struct.pack(f"<{len(positions) * 3}f", *[c for p in positions for c in p])
    nor = struct.pack(f"<{len(normals) * 3}f", *[c for n in normals for c in n])
    col = struct.pack(f"<{len(colors) * 4}B", *[v for c in colors for v in (round(c[0] * 255), round(c[1] * 255), round(c[2] * 255), 255)])
    idx = pad(struct.pack(f"<{len(indices)}H", *indices))
    blob = pos + nor + col + idx
    mins = [min(p[i] for p in positions) for i in range(3)]
    maxs = [max(p[i] for p in positions) for i in range(3)]
    o1, o2, o3 = len(pos), len(pos) + len(nor), len(pos) + len(nor) + len(col)
    gltf = {
        "asset": {"version": "2.0", "generator": "gen_world.py"},
        "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
        "materials": [{"pbrMetallicRoughness": {"baseColorFactor": [1, 1, 1, 1], "metallicFactor": 0.0, "roughnessFactor": 0.95}}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1, "COLOR_0": 2}, "indices": 3, "material": 0}]}],
        "buffers": [{"byteLength": len(blob)}],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": len(pos), "target": 34962},
            {"buffer": 0, "byteOffset": o1, "byteLength": len(nor), "target": 34962},
            {"buffer": 0, "byteOffset": o2, "byteLength": len(col), "target": 34962},
            {"buffer": 0, "byteOffset": o3, "byteLength": len(indices) * 2, "target": 34963},
        ],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": len(positions), "type": "VEC3", "min": mins, "max": maxs},
            {"bufferView": 1, "componentType": 5126, "count": len(normals), "type": "VEC3"},
            {"bufferView": 2, "componentType": 5121, "normalized": True, "count": len(colors), "type": "VEC4"},
            {"bufferView": 3, "componentType": 5123, "count": len(indices), "type": "SCALAR"},
        ],
    }
    js = pad(json.dumps(gltf, separators=(",", ":")).encode(), b" ")
    total = 12 + 8 + len(js) + 8 + len(blob)
    return (struct.pack("<III", 0x46546C67, 2, total) + struct.pack("<II", len(js), 0x4E4F534A) + js
            + struct.pack("<II", len(blob), 0x004E4942) + blob)


def write_fallback_terrain(out, scene):
    """Visual-only terrain chunks for players without web terrain streaming."""
    out.mkdir(parents=True, exist_ok=True)
    count = math.ceil(2 * HALF / FALLBACK_CHUNK)
    n = FALLBACK_SAMPLES
    step = FALLBACK_CHUNK / (n - 1)
    for cz in range(count):
        for cx in range(count):
            ox, oz = cx * FALLBACK_CHUNK - HALF, cz * FALLBACK_CHUNK - HALF
            pos, nor, col, idx = [], [], [], []
            for j in range(n):
                for i in range(n):
                    x, z = ox + i * step, oz + j * step
                    h = height(x, z)
                    dx = height(x + 2, z) - height(x - 2, z)
                    dz = height(x, z + 2) - height(x, z - 2)
                    nx, ny, nz = -dx / 4.0, 1.0, -dz / 4.0
                    ln = math.sqrt(nx * nx + ny * ny + nz * nz)
                    pos.append((i * step, h, j * step))
                    nor.append((nx / ln, ny / ln, nz / ln))
                    col.append(ground_color(x, z, h, 1.0 - ny / ln))
                    if i + 1 < n and j + 1 < n:
                        a = j * n + i
                        idx += [a, a + n, a + 1, a + 1, a + n, a + n + 1]
            name = f"chunk_{cx}_{cz}.glb"
            (out / name).write_bytes(glb_bytes(pos, nor, col, idx))
            scene.add(f"ground-{cx}-{cz}", "gltf", [ox, 0.0, oz], path=f"{GAME}/terrain_fallback/{name}",
                      tags=["terrain_fallback", "floating"])


# ---------------------------------------------------------------- world content
def landing_jungle(scene):
    rng = random.Random(4401)
    for i in range(28):
        angle = i * math.tau / 28 + rng.uniform(-0.05, 0.05)
        radius = rng.uniform(30, 40)
        x, z = LZ[0] + math.cos(angle) * radius, LZ[1] + math.sin(angle) * radius
        if road_dist(x, z) < 9:
            continue
        scene.asset(f"lz-tree-{i}", "tree_palm" if i % 3 == 0 else "tree_broadleaf",
                    x, z, rng.uniform(0, 360), rng.uniform(0.85, 1.15), lift=-0.2, tags=["tree"])
        for j in range(2):
            bx, bz = x + rng.uniform(-3, 3), z + rng.uniform(-3, 3)
            scene.asset(f"lz-undergrowth-{i}-{j}", "bush" if j == 0 else "fern",
                        bx, bz, rng.uniform(0, 360), rng.uniform(1, 1.5), lift=-0.1)


def build(scene, rng, density, with_terrain_entity):
    E = scene
    # Player and world settings.
    player_ui = {
        "title": "Operation Rio Verde",
        "mission": "Kämpfe dich durch 44 km² Dschungel bis zur Zitadelle.",
        "intro_title": "Operation Rio Verde",
        "intro_text": "Landezone Echo. Überquere den Rio Verde, säubere die Außenposten, sichere die Akte in San Aurelio, zerstöre den Funkturm und stürme die Zitadelle.",
        "win_text": "Zitadelle eingenommen – Mission erfüllt!",
        "badge": "Rio Verde", "win_badge": "Sieg",
        "hud_keys": ["health", "ammo", "ammo_reserve", "kills", "objective"],
        "sky_color": "#8cadb8",
    }
    sx, sz = LZ[0] + 12, LZ[1] + 12
    E.add("player", "cylinder", [sx, height(sx, sz) + 0.9, sz], radius=0.4, height=1.8, segments=12,
          color=[0.2, 0.3, 0.2, 1], tags=["player", "always_active"],
          collider={"shape": "capsule", "radius": 0.4, "half_height": 0.5},
          rigidbody={"type": "dynamic", "mass": 80}, script=f"{GAME}/scripts/player.rhai",
          props={
              "ai_lock_rotation": True,
              "ai_fps": {"eye_height": 0.75, "move_speed": 5.5, "sprint_speed": 9.0, "jump_speed": 5.0, "mouse_sens": 0.0022},
              "ai_viewmodel": f"{ASSETS}/rifle_viewmodel.glb",
              "ai_graphics": {"sky": "tropical", "time_of_day": 16.5,
                              "foliage": {"trees": 0, "distance": 112},
                              "clouds": {"coverage": 0.48}, "haze": {"density": 0.002, "height": 18, "falloff": 0.045},
                              "shadow_extent": {"low": 24, "balanced": 64, "ultra": 144},
                              "shadow_map_size": {"low": 512, "balanced": 1024, "ultra": 4096},
                              "water": {"level": 0.6, "color": [0.18, 0.32, 0.28]}, "fog_density": 0.0011},
              "ai_world": {"chunk_size": 128, "stream_radius": 640, "draw_distance": 600, "lod_distances": [70, 220], "camera_distance": 36,
                           "sim_radius": 260, "sim_hysteresis": 30},
              "ai_ui": player_ui,
          })
    if with_terrain_entity:
        E.add("terrain", "plane", [-HALF, 0.0, -HALF], size=1.0, color=[0.16, 0.3, 0.1, 1],
              tags=["ground", "terrain"], props={"ai_terrain": f"{GAME}/terrain/terrain.json", "ai_terrain_radius": 300})
    E.add("river-water", "plane", [0.0, WATER_Y, 0.0], size=2 * HALF, color=[0.16, 0.34, 0.33, 1],
          tags=["water", "floating"])

    # Roads: flat strips following the ground.
    for r_i, road in enumerate(ROADS):
        for i in range(len(road) - 1):
            (ax, az), (bx, bz) = road[i], road[i + 1]
            length = math.hypot(bx - ax, bz - az)
            steps = max(1, round(length / 18.0))
            yaw = math.degrees(math.atan2(bx - ax, bz - az))
            for s in range(steps):
                t = (s + 0.5) / steps
                x, z = ax + (bx - ax) * t, az + (bz - az) * t
                y = max(height(x, z), WATER_Y + 3.6 if abs(x - river_x(z)) < 60 else -99)
                E.add(f"road-{r_i}-{i}-{s}", "box", [x, y - 0.22, z], size=[6.5, 0.5, length / steps + 1.2],
                      rotation_deg=[0, yaw, 0], color=[0.36, 0.28, 0.18, 1], tags=["road", "floating"])

    # Bridges over the Rio Verde.
    for label, bz in (("south", BRIDGE_S_Z), ("north", BRIDGE_N_Z)):
        bx = river_x(bz)
        for k in (-1, 0, 1):
            E.asset(f"bridge-{label}-{k}", "bridge", bx + k * 40.0, bz, yaw=90.0, y=WATER_Y,
                    tags=["bridge"])
        deck = WATER_Y + 4.0
        E.add(f"bridge-{label}-deck", "box", [bx, deck - 0.15, bz], size=[120.0, 0.3, 6.8],
              collider={"shape": "box"}, tags=["bridge", "ground"], visible=False)
        for side in (-1, 1):
            bank = height(bx + side * 100.0, bz)
            angle = side * math.atan2(bank - deck, 40.0)
            E.add(f"bridge-{label}-ramp-{side}", "box",
                  [bx + side * 80.0 + math.sin(angle) * 0.15,
                   (deck + bank) / 2.0 - math.cos(angle) * 0.15, bz],
                  size=[math.hypot(40.0, bank - deck) + 0.4, 0.3, 6.8],
                  rotation_deg=[0.0, 0.0, math.degrees(angle)],
                  collider={"shape": "box"}, color=[0.36, 0.28, 0.18, 1], tags=["bridge", "ground"])
        if label == "south":
            E.add("bridge-marker", "sphere", [bx, WATER_Y + 4.5, bz], radius=0.3, segments=6,
                  color=[1, 0.8, 0.2, 1], tags=["marker", "floating"])

    covers = []

    def cover(name, label, x, z, yaw=0.0):
        E.asset(name, label, x, z, yaw, collider={"shape": "mesh"}, tags=["cover"])
        covers.append((x, z))

    enemies = []

    def soldier(name, x, z, group, extra_tags=()):
        e = E.asset(name, "soldier", x, z, rng.uniform(0, 360), lift=0.0,
                    tags=["enemy", "soldier", f"grp_{group}", *extra_tags],
                    script=f"{GAME}/scripts/soldier.rhai",
                    collider={"shape": "capsule", "radius": 0.4, "half_height": 0.5},
                    rigidbody={"type": "kinematic", "mass": 80})
        enemies.append(e)

    def drone(name, x, z, group):
        e = E.asset(name, "drone", x, z, 0.0, lift=14.0, tags=["enemy", "drone", f"grp_{group}"],
                    script=f"{GAME}/scripts/drone.rhai",
                    collider={"shape": "sphere", "radius": 0.7}, rigidbody={"type": "kinematic", "mass": 6})
        enemies.append(e)

    def pickup(kind, x, z):
        label = "medkit" if kind == "health" else "ammo_box"
        E.asset(f"pickup-{kind}-{len(E.entities)}", label, x, z, rng.uniform(0, 360), lift=0.05,
                tags=["pickup", f"pickup_{kind}", "trigger"])

    def outpost(label, center, soldiers, drones, tower=False):
        cx, cz = center
        E.asset(f"{label}-flag", "flag_pole", cx, cz, 0.0)
        for k, ang in enumerate((45, 225)):
            a = math.radians(ang)
            E.asset(f"{label}-watchtower-{k}", "watchtower", cx + 55 * math.cos(a), cz + 55 * math.sin(a),
                    ang, collider={"shape": "mesh"})
        E.asset(f"{label}-barracks", "barracks", cx - 22, cz + 14, 90.0, collider={"shape": "mesh"})
        E.asset(f"{label}-bunker", "bunker", cx + 30, cz - 26, 200.0, collider={"shape": "mesh"})
        for k in range(10):
            a = k / 10 * math.tau
            cover(f"{label}-sandbags-{k}", "sandbags", cx + 42 * math.cos(a), cz + 42 * math.sin(a), -math.degrees(a) + 90)
        for k in range(8):
            cover(f"{label}-crate-{k}", "crate", cx + rng.uniform(-25, 25), cz + rng.uniform(-25, 25), rng.uniform(0, 90))
        for k in range(5):
            E.asset(f"{label}-barrel-{k}", "barrel", cx + rng.uniform(-28, 28), cz + rng.uniform(-28, 28), 0.0)
        for k in range(soldiers):
            a = k / soldiers * math.tau + 0.3
            soldier(f"{label}-soldier-{k}", cx + 20 * math.cos(a), cz + 20 * math.sin(a), label)
        for k in range(drones):
            drone(f"{label}-drone-{k}", cx + 30 * (k + 1), cz - 10, label)
        pickup("ammo", cx + 6, cz + 4)
        pickup("health", cx - 6, cz - 3)
        if tower:
            E.asset("radio-tower", "radio_tower", cx + 8, cz - 8, 30.0, collider={"shape": "mesh"},
                    tags=["objective_target", "damageable"], props={"ai_health": 300})

    # Landing zone Echo.
    E.asset("lz-jeep", "jeep_wreck", LZ[0] - 8, LZ[1] + 4, 35.0, collider={"shape": "mesh"})
    for k in range(4):
        cover(f"lz-crate-{k}", "crate", LZ[0] + 5 + k * 1.4, LZ[1] - 6, 0.0)
    pickup("ammo", LZ[0] + 4, LZ[1] + 8)
    pickup("health", LZ[0] + 8, LZ[1] + 4)

    outpost("alpha", ALPHA, 6, 0)
    outpost("bravo", BRAVO, 6, 1, tower=True)
    outpost("charlie", CHARLIE, 5, 1)

    # San Aurelio, the ruined town.
    tx, tz = TOWN
    for gx in range(-3, 4):
        for gz in range(-3, 4):
            if gx == 0 or gz == 0 or rng.random() < 0.2:
                continue  # main streets and empty lots
            x, z = tx + gx * 36 + rng.uniform(-4, 4), tz + gz * 36 + rng.uniform(-4, 4)
            label = "house_ruin" if rng.random() < 0.65 else "house_shack"
            E.asset(f"town-{label}-{gx}-{gz}", label, x, z, rng.choice((0, 90, 180, 270)) + rng.uniform(-6, 6),
                    collider={"shape": "mesh"}, tags=["building"])
    for k in range(7):
        x, z = tx + rng.uniform(-110, 110), tz + rng.choice((-1, 1)) * rng.uniform(2, 6)
        E.asset(f"town-jeep-{k}", "jeep_wreck", x, z, rng.uniform(0, 360), collider={"shape": "mesh"}, tags=["cover"])
        covers.append((x, z))
    for k in range(14):
        cover(f"town-sandbags-{k}", "sandbags", tx + rng.uniform(-120, 120), tz + rng.uniform(-120, 120), rng.uniform(0, 360))
    E.asset("intel", "intel_case", tx + 4, tz + 3, 0.0, tags=["intel", "trigger"])
    for k in range(10):
        a = k / 10 * math.tau
        soldier(f"town-soldier-{k}", tx + 70 * math.cos(a), tz + 70 * math.sin(a), "town")
    drone("town-drone-0", tx + 40, tz - 40, "town")
    for k in range(4):
        pickup("ammo" if k % 2 else "health", tx + rng.uniform(-90, 90), tz + rng.uniform(-90, 90))

    # The Citadel on the mesa.
    cx, cz = CITADEL
    top = height(cx, cz)
    half = 70.0
    for side in range(4):
        for k in range(7):
            off = -half + 10 + k * 20
            if side == 0 and k == 3:
                continue  # south gate
            if side == 0:
                x, z, yaw = cx + off, cz - half, 0
            elif side == 1:
                x, z, yaw = cx + off, cz + half, 0
            elif side == 2:
                x, z, yaw = cx - half, cz + off, 90
            else:
                x, z, yaw = cx + half, cz + off, 90
            E.asset(f"citadel-wall-{side}-{k}", "fortress_wall", x, z, yaw, y=top, collider={"shape": "mesh"},
                    tags=["building"])
    for k, (dx, dz) in enumerate(((-1, -1), (1, -1), (-1, 1), (1, 1))):
        E.asset(f"citadel-tower-{k}", "fortress_tower", cx + dx * half, cz + dz * half, 0, y=top,
                collider={"shape": "mesh"}, tags=["building"])
    E.asset("citadel-keep", "citadel_keep", cx, cz + 20, 0, y=top, collider={"shape": "mesh"}, tags=["building"])
    for k in range(12):
        cover(f"citadel-sandbags-{k}", "sandbags", cx + rng.uniform(-55, 55), cz - rng.uniform(-10, 40), rng.uniform(0, 360))
    for k in range(10):
        soldier(f"citadel-soldier-{k}", cx + rng.uniform(-55, 55), cz + rng.uniform(-55, -10), "citadel")
    for k in range(4):
        soldier(f"citadel-gate-{k}", cx - 12 + k * 8, cz - half - 18, "citadel")
    soldier("commander", cx, cz - 6, "citadel", extra_tags=("commander",))
    for k in range(3):
        drone(f"citadel-drone-{k}", cx - 50 + k * 50, cz - 30, "citadel")
    for k in range(3):
        pickup("ammo", cx - 30 + k * 30, cz - half - 30)
    pickup("health", cx + 20, cz - half - 26)

    # Roadside patrols and scattered pickups.
    for k in range(18):
        road = ROADS[k % len(ROADS)]
        i = rng.randrange(len(road) - 1)
        (ax, az), (bx, bz) = road[i], road[i + 1]
        t = rng.uniform(0.25, 0.75)
        x, z = ax + (bx - ax) * t + rng.uniform(-12, 12), az + (bz - az) * t + rng.uniform(-12, 12)
        if math.hypot(x - LZ[0], z - LZ[1]) < 400:
            continue
        soldier(f"patrol-soldier-{k}", x, z, "patrol")
        if k % 3 == 0:
            pickup("ammo" if k % 2 else "health", x + 5, z + 5)

    # Jungle scatter (deterministic jittered grid).
    keep_out = [(LZ, 45), (ALPHA, 80), (BRAVO, 80), (CHARLIE, 80), (TOWN, 175), (CITADEL, 120)]
    spacing = 40.0 / math.sqrt(density)
    n = int(2 * HALF / spacing)
    tree_i = rock_i = bush_i = 0
    for gz in range(n):
        for gx in range(n):
            x = -HALF + (gx + rng.random()) * spacing
            z = -HALF + (gz + rng.random()) * spacing
            roll = rng.random()
            if abs(x) > HALF - 40 or abs(z) > HALF - 40:
                continue
            if any(math.hypot(x - p[0], z - p[1]) < r for p, r in keep_out):
                continue
            river_d = abs(x - river_x(z))
            if river_d < 28 or road_dist(x, z) < 8:
                continue
            h = height(x, z)
            yaw = rng.uniform(0, 360)
            if roll < 0.55:
                kind = "tree_palm" if (river_d < 120 or rng.random() < 0.25) else ("tree_kapok" if rng.random() < 0.08 else "tree_broadleaf")
                s = rng.uniform(0.8, 1.25)
                E.asset(f"tree-{tree_i}", kind, x, z, yaw, s, lift=-0.2,
                        collider={"shape": "capsule", "radius": 0.45 if kind != "tree_kapok" else 1.4},
                        tags=["tree"])
                tree_i += 1
            elif roll < 0.83:
                kind = "bush" if rng.random() < 0.6 else "fern"
                E.asset(f"{kind}-{bush_i}", kind, x, z, yaw, rng.uniform(0.8, 1.5), lift=-0.1)
                bush_i += 1
            elif roll < 0.93 or h > 70:
                kind = "rock_a" if rng.random() < 0.6 else "rock_b"
                E.asset(f"rock-{rock_i}", kind, x, z, yaw, rng.uniform(0.6, 1.8), lift=-0.4,
                        collider={"shape": "mesh"}, tags=["cover"] if rock_i % 4 == 0 else None)
                rock_i += 1
    landing_jungle(E)
    return len(enemies), len(covers)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seed", type=int, default=44)
    parser.add_argument("--density", type=float, default=0.48)
    parser.add_argument("--no-fallback-terrain", action="store_true",
                        help="skip visual terrain chunks (use once web terrain streaming renders ai_terrain)")
    parser.add_argument("--no-terrain-entity", action="store_true", help="omit the ai_terrain entity")
    parser.add_argument("--skip-terrain-files", action="store_true", help="reuse existing terrain tiles")
    args = parser.parse_args()
    game = ROOT / GAME
    scene = Scene(load_manifest())
    rng = random.Random(args.seed)
    enemies, covers = build(scene, rng, args.density, not args.no_terrain_entity)
    if not args.skip_terrain_files:
        import shutil
        shutil.rmtree(game / "terrain", ignore_errors=True)
        write_terrain(game / "terrain")
    import shutil
    shutil.rmtree(game / "terrain_fallback", ignore_errors=True)
    if not args.no_fallback_terrain:
        write_fallback_terrain(game / "terrain_fallback", scene)
    doc = {
        "name": "Operation Rio Verde",
        "camera": {"position": [LZ[0] + 30, height(*LZ) + 12, LZ[1] + 30], "target": [LZ[0] + 12, height(*LZ) + 2, LZ[1] + 12], "fov_deg": 70},
        "light_dir": [-0.45, 1.0, 0.35],
        "ambient": 0.45,
        "entities": scene.entities,
    }
    (game / "scene.json").write_text(json.dumps(doc, separators=(",", ":")) + "\n")
    print(f"entities={len(scene.entities)} enemies={enemies} covers={covers}")


if __name__ == "__main__":
    main()
