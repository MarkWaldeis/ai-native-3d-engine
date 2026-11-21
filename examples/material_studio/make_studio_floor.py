"""Generate a baked, tileable slate texture and the studio's offline floor GLB."""

import json
import math
import struct
import zlib
from pathlib import Path


def texture(size=128):
    def chunk(kind, contents):
        return (struct.pack(">I", len(contents)) + kind + contents
                + struct.pack(">I", zlib.crc32(kind + contents)))

    rows = bytearray()
    for y in range(size):
        rows.append(0)  # PNG filter: none
        for x in range(size):
            u, v = x / size, y / size
            edge = min(x, y, size - 1 - x, size - 1 - y)
            grit = math.sin(math.tau * (5*u + 3*v)) + math.sin(math.tau * (17*u - 13*v))
            lining = 3 * math.cos(math.tau * (31*u + 19*v))
            tone = 35 if edge < 1 else (47 if edge < 2 else round(57 + 3*grit + lining))
            rows.extend((tone - 6, tone, tone + 8))
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(rows, level=9))
        + chunk(b"IEND", b"")
    )


def build_floor():
    x, y, z = 6.5, 0.2, 5.5
    faces = [
        ((0,1,0),  ((-x,y,z), (x,y,z), (x,y,-z), (-x,y,-z)), (18,15)),
        ((0,-1,0), ((-x,-y,-z), (x,-y,-z), (x,-y,z), (-x,-y,z)), (18,15)),
        ((0,0,1),  ((-x,-y,z), (x,-y,z), (x,y,z), (-x,y,z)), (18,1)),
        ((0,0,-1), ((x,-y,-z), (-x,-y,-z), (-x,y,-z), (x,y,-z)), (18,1)),
        ((1,0,0),  ((x,-y,z), (x,-y,-z), (x,y,-z), (x,y,z)), (15,1)),
        ((-1,0,0), ((-x,-y,-z), (-x,-y,z), (-x,y,z), (-x,y,-z)), (15,1)),
    ]
    positions, normals, uvs, indices = [], [], [], []
    for normal, points, (u, v) in faces:
        start = len(positions) // 3
        positions.extend(coordinate for point in points for coordinate in point)
        normals.extend(normal * 4)
        uvs.extend((0,0, u,0, u,v, 0,v))
        indices.extend(start + i for i in (0,1,2,0,2,3))

    blob, views = bytearray(), []
    for fmt, data in (("f", positions), ("f", normals), ("f", uvs), ("H", indices), (None, texture())):
        packed = struct.pack(f"<{len(data)}{fmt}", *data) if fmt else data
        views.append({"buffer": 0, "byteOffset": len(blob), "byteLength": len(packed)})
        blob.extend(packed)
        blob.extend(bytes(-len(blob) % 4))

    gltf = {
        "asset": {"version": "2.0"},
        "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2}, "indices": 3, "material": 0}]}],
        "materials": [{"pbrMetallicRoughness": {
            "baseColorTexture": {"index": 0}, "metallicFactor": 0.27, "roughnessFactor": 0.48,
        }}],
        "textures": [{"source": 0, "sampler": 0}],
        "samplers": [{"magFilter": 9729, "minFilter": 9987}],
        "images": [{"bufferView": 4, "mimeType": "image/png"}],
        "buffers": [{"byteLength": len(blob)}],
        "bufferViews": views,
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 24, "type": "VEC3", "min": [-x,-y,-z], "max": [x,y,z]},
            {"bufferView": 1, "componentType": 5126, "count": 24, "type": "VEC3"},
            {"bufferView": 2, "componentType": 5126, "count": 24, "type": "VEC2"},
            {"bufferView": 3, "componentType": 5123, "count": 36, "type": "SCALAR"},
        ],
    }
    descriptor = json.dumps(gltf, separators=(",", ":")).encode()
    descriptor += b" " * (-len(descriptor) % 4)
    chunks = (struct.pack("<I4s", len(descriptor), b"JSON") + descriptor
              + struct.pack("<I4s", len(blob), b"BIN\0") + blob)
    return struct.pack("<4sII", b"glTF", 2, len(chunks) + 12) + chunks


if __name__ == "__main__":
    Path(__file__).with_name("slate-floor.glb").write_bytes(build_floor())
