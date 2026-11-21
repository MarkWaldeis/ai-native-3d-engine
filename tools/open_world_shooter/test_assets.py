"""Standard-library checks for generated self-contained glTF assets."""
import json
from pathlib import Path
import struct
import unittest


ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / 'examples/open_world_shooter/assets'


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
                    self.assertTrue(all('uri' not in b for b in doc['buffers']))
                    self.assertNotIn('images', doc)
                    primitives = [p for m in doc['meshes'] for p in m['primitives']]
                    count = sum(doc['accessors'][p['indices']]['count'] // 3 for p in primitives)
                    counts.append(count)
                    self.assertGreater(count, 0)
                    self.assertTrue(all(p.get('mode', 4) == 4 for p in primitives))
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


if __name__ == '__main__':
    unittest.main()
