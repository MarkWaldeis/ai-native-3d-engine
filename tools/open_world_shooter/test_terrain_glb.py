"""Fallback terrain uses core glTF float normals, without quantization extensions."""
import json
from pathlib import Path
import struct
import unittest


class TerrainGlbTest(unittest.TestCase):
    def test_generated_normals_are_float_vec3(self):
        root = Path(__file__).resolve().parents[2]
        chunks = list((root / 'examples/open_world_shooter/terrain_fallback').glob('*.glb'))
        self.assertTrue(chunks)
        for chunk in chunks:
            data = chunk.read_bytes()
            size = struct.unpack_from('<I', data, 12)[0]
            document = json.loads(data[20:20 + size])
            primitive = document['meshes'][0]['primitives'][0]
            normal = document['accessors'][primitive['attributes']['NORMAL']]
            view = document['bufferViews'][normal['bufferView']]
            with self.subTest(chunk=chunk.name):
                self.assertEqual(normal['componentType'], 5126)
                self.assertEqual(normal['type'], 'VEC3')
                self.assertEqual(view['byteLength'], normal['count'] * 12)
                self.assertFalse(normal.get('normalized', False))


if __name__ == '__main__':
    unittest.main()
