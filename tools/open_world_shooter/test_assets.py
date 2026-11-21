"""Standard-library checks for generated self-contained glTF assets."""
import json
import math
from pathlib import Path
import struct
import unittest


ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / 'examples/open_world_shooter/assets'
MODEL_SIZES = {
    'soldier': (.76738, 1.835, .9948),
    'citadel_keep': (30.2, 22, 30.325),
    'fortress_wall': (21, 8, 3.8),
    'fortress_tower': (8.2, 15.65, 8.2),
    'barracks': (6.2856, 4.14101, 12.12),
    'house_shack': (7.30462, 5.1, 6.925),
    'house_ruin': (8, 3.47976, 6),
    'watchtower': (5.10462, 9.05, 5),
    'bunker': (6.3, 3.05, 5.3),
}
VEGETATION_BUDGETS = {
    # X/Z half-extents, height, high/low triangle and high primitive budgets.
    'tree_palm': (3.37909, 3.41383, 10.34475, 3200, 693, 4),
    'tree_broadleaf': (6.59979, 6.37377, 14.18711, 1150, 185, 4),
    'tree_kapok': (11.00072, 10.77709, 23.82228, 1600, 186, 4),
    'bush': (1.24013, 1.09355, 1.46445, 1050, 219, 3),
    'fern': (.86726, .88064, .71288, 1100, 244, 2),
}


class AssetsTest(unittest.TestCase):
    def test_exported_library(self):
        manifest = json.loads((ASSETS / 'manifest.json').read_text())
        self.assertEqual(len(manifest), 28)
        total = 0
        for name, entry in manifest.items():
            counts = []
            for key in ['high', 'low']:
                if entry[key] is None:
                    continue
                with self.subTest(name=name, lod=key):
                    data = (ROOT / entry[key]).read_bytes()
                    total += len(data)
                    magic, version, length = struct.unpack_from('<III', data)
                    self.assertEqual((magic, version, length), (0x46546C67, 2, len(data)))
                    size, kind = struct.unpack_from('<II', data, 12)
                    self.assertEqual(kind, 0x4E4F534A)
                    doc = json.loads(data[20:20 + size])
                    binary = 28 + size
                    self.assertTrue(all('uri' not in b for b in doc['buffers']))
                    self.assertNotIn('images', doc)
                    primitives = [p for m in doc['meshes'] for p in m['primitives']]
                    count = sum(doc['accessors'][p['indices']]['count'] // 3 for p in primitives)
                    counts.append(count)
                    self.assertGreater(count, 0)
                    self.assertTrue(all(p.get('mode', 4) == 4 for p in primitives))
                    if name in VEGETATION_BUDGETS:
                        x, z, height, high_limit, low_count, primitive_limit = VEGETATION_BUDGETS[name]
                        if key == 'high':
                            self.assertLessEqual(count, high_limit)
                            self.assertLessEqual(len(primitives), primitive_limit)
                            expected = [[-x, 0, -z], [x, height, z]]
                            self.assertEqual(entry['bounds'], expected)
                            positions = [doc['accessors'][p['attributes']['POSITION']] for p in primitives]
                            for bound, select, values in zip(('min', 'max'), (min, max), expected):
                                for axis, target in enumerate(values):
                                    self.assertAlmostEqual(select(a[bound][axis] for a in positions), target, delta=1e-5)
                        else:
                            self.assertEqual(count, low_count)
                    for index in {p['attributes']['NORMAL'] for p in primitives}:
                        accessor = doc['accessors'][index]
                        self.assertEqual(accessor['componentType'], 5126)
                        self.assertEqual(accessor['type'], 'VEC3')
                        view = doc['bufferViews'][accessor['bufferView']]
                        start = binary + view.get('byteOffset', 0) + accessor.get('byteOffset', 0)
                        for i in range(accessor['count']):
                            normal = struct.unpack_from('<3f', data, start + i * view.get('byteStride', 12))
                            length = math.sqrt(sum(n*n for n in normal))
                            self.assertTrue(math.isfinite(length))
                            self.assertAlmostEqual(length, 1, delta=1e-5)
            self.assertEqual(counts[0], entry['tris'])
            if len(counts) == 2:
                self.assertGreaterEqual(counts[1] / counts[0], .10)
                self.assertLessEqual(counts[1] / counts[0], .25)
            for lo, hi in zip(*entry['bounds']):
                self.assertLess(lo, hi)
            if name not in ['bridge', 'rifle_viewmodel']:
                self.assertAlmostEqual(entry['bounds'][0][1], 0, places=4)
        self.assertLess(total, 12 * 1024 * 1024)
        self.assertGreaterEqual(manifest['soldier']['tris'], 1500)
        self.assertEqual(manifest['bridge']['bounds'][0][1], -4)

    def test_model_collision_envelopes_and_detail_budgets(self):
        manifest = json.loads((ASSETS / 'manifest.json').read_text())
        for name, original_size in MODEL_SIZES.items():
            with self.subTest(name=name):
                lo, hi = manifest[name]['bounds']
                for axis, size in enumerate(original_size):
                    self.assertAlmostEqual(hi[axis] - lo[axis], size, delta=size * .03)
                    if axis != 1:
                        self.assertAlmostEqual((lo[axis] + hi[axis]) / 2, 0, delta=size * .015)
                if name == 'soldier':
                    self.assertGreaterEqual(manifest[name]['tris'], 6000)
                    self.assertLessEqual(manifest[name]['tris'], 9000)
                else:
                    self.assertLessEqual(manifest[name]['tris'], 10000)


if __name__ == '__main__':
    unittest.main()
