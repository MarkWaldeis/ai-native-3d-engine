"""Keep the authored landing-zone jungle deterministic and close enough to see."""
import math
import unittest

from gen_world import LZ, Scene, landing_jungle, load_manifest, road_dist


class LandingJungleTest(unittest.TestCase):
    def test_deterministic_clearing_edge_and_clear_road(self):
        first, second = Scene(load_manifest()), Scene(load_manifest())
        landing_jungle(first)
        landing_jungle(second)
        self.assertEqual(first.entities, second.entities)
        trees = [entity for entity in first.entities if 'tree' in entity.get('tags', [])]
        self.assertGreaterEqual(len(trees), 18)
        self.assertLess(len(first.entities), 90)
        for tree in trees:
            x, _, z = tree['transform']['position']
            self.assertGreaterEqual(math.hypot(x - LZ[0], z - LZ[1]), 29.99)
            self.assertLessEqual(math.hypot(x - LZ[0], z - LZ[1]), 40.01)
            self.assertGreaterEqual(road_dist(x, z), 9)
            self.assertEqual(tree['props']['web_lods']['1'], tree['props']['web_lods']['2'])


if __name__ == '__main__':
    unittest.main()
