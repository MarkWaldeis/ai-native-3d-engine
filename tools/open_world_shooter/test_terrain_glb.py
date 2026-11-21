"""Fallback terrain uses core glTF float normals, without quantization extensions."""
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch

from gen_world import Scene, write_fallback_terrain


class TerrainGlbTest(unittest.TestCase):
    def test_generated_normals_are_float_vec3(self):
        scene = Scene({})
        with tempfile.TemporaryDirectory() as directory, patch('gen_world.HALF', 256.0):
            write_fallback_terrain(Path(directory), scene)
            chunks = [chunk.read_bytes() for chunk in Path(directory).glob('*.glb')]
        self.assertEqual(len(scene.entities), 1)
        self.assertIn('terrain_fallback', scene.entities[0]['tags'])
        self.assertTrue(chunks)
        for data in chunks:
            size = struct.unpack_from('<I', data, 12)[0]
            document = json.loads(data[20:20 + size])
            primitive = document['meshes'][0]['primitives'][0]
            normal = document['accessors'][primitive['attributes']['NORMAL']]
            view = document['bufferViews'][normal['bufferView']]
            with self.subTest():
                self.assertEqual(normal['componentType'], 5126)
                self.assertEqual(normal['type'], 'VEC3')
                self.assertEqual(view['byteLength'], normal['count'] * 12)
                self.assertFalse(normal.get('normalized', False))


if __name__ == '__main__':
    unittest.main()
