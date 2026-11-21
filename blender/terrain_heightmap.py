"""Run in Blender's Text Editor, or --background --python ... -- [options].

Create a sculpt-friendly displaced grid, or bake an existing active mesh with
--bake-active. Blender XY maps to engine XZ; Blender Z is engine height.
Writes a true grayscale16 PNG (not an 8-bit display/color-managed render) and
a terrain_import MCP argument file. No external Python packages required.
"""
import argparse
import json
from pathlib import Path
import struct
import sys
import zlib

import bpy
from mathutils import Vector
from mathutils.bvhtree import BVHTree


def png16(path, rows, resolution):
    def chunk(kind, data):
        return (struct.pack(">I", len(data)) + kind + data
                + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF))
    raw = b"".join(b"\0" + struct.pack(">" + "H" * resolution, *row) for row in rows)
    path.write_bytes(b"\x89PNG\r\n\x1a\n"
                     + chunk(b"IHDR", struct.pack(">IIBBBBB", resolution, resolution, 16, 0, 0, 0, 0))
                     + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", default="terrain.png")
    parser.add_argument("--size", type=float, default=44000000 ** 0.5)
    parser.add_argument("--height-scale", type=float, default=200)
    parser.add_argument("--resolution", type=int, default=513)
    parser.add_argument("--grid", type=int, default=257)
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--bake-active", action="store_true")
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else [])
    if args.size <= 0 or args.height_scale <= 0 or min(args.grid, args.resolution) < 2:
        parser.error("size/height must be positive and grid/resolution >= 2")
    if not args.bake_active:
        bpy.ops.mesh.primitive_grid_add(x_subdivisions=args.grid, y_subdivisions=args.grid,
                                       size=args.size, location=(args.size / 2, args.size / 2, 0))
        obj = bpy.context.object
        obj.name = "Terrain"
        texture = bpy.data.textures.new("Terrain noise", type="CLOUDS")
        texture.noise_scale = args.size / (6 + (args.seed % 1000) / 1000)
        modifier = obj.modifiers.new("Editable terrain displacement", "DISPLACE")
        modifier.texture = texture
        modifier.texture_coords = "GLOBAL"
        modifier.strength = args.height_scale
        modifier.mid_level = 0
    obj = bpy.context.object
    if obj is None or obj.type != "MESH":
        raise ValueError("Select a terrain mesh before --bake-active")
    depsgraph = bpy.context.evaluated_depsgraph_get()
    depsgraph.update()
    tree = BVHTree.FromObject(obj, depsgraph)
    inverse = obj.matrix_world.inverted()
    direction = (inverse.to_3x3() @ Vector((0, 0, -1))).normalized()
    rows = []
    missing = 0
    edge_margin = args.size * 1e-6
    for j in range(args.resolution):
        row = []
        for i in range(args.resolution):
            x = args.size * i / (args.resolution - 1)
            y = args.size * j / (args.resolution - 1)
            # Stay inside the float32 grid boundary after world/local conversion.
            x = max(edge_margin, min(args.size - edge_margin, x))
            y = max(edge_margin, min(args.size - edge_margin, y))
            origin = inverse @ Vector((x, y, args.height_scale * 100))
            hit, _, _, _ = tree.ray_cast(origin, direction)
            if hit is None:
                missing += 1
                height = 0
            else:
                height = (obj.matrix_world @ hit).z
            row.append(round(max(0, min(1, height / args.height_scale)) * 65535))
        rows.append(row)
    output = Path(args.out).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    png16(output, rows, args.resolution)
    request = {"heightmap": str(output), "out_dir": str(output.with_suffix("")),
               "size_m": [args.size, args.size], "height_scale": args.height_scale,
               "tile_m": 256, "samples": 129, "seed": args.seed}
    output.with_suffix(".import.json").write_text(json.dumps(request, indent=2), encoding="utf-8")
    print(f"Baked {output}; {missing} uncovered samples set to zero. "
          "Call terrain_import with the .import.json arguments inside the project root.")


if __name__ == "__main__":
    main()
