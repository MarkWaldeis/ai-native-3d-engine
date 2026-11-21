import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from gen_large_world import generate, GRID, WORLD_SIZE


class GeneratorTests(unittest.TestCase):
    def test_default_scene_is_deterministic_and_covers_world(self):
        scene = generate()
        self.assertEqual(scene, generate())
        self.assertNotEqual(scene, generate(seed=2))
        self.assertEqual(scene["name"], "large_world_bench")
        self.assertEqual(len(scene["entities"]), 14997)
        self.assertEqual(len({e["id"] for e in scene["entities"]}), 14997)
        terrain = scene["entities"][:GRID * GRID]
        self.assertTrue(all(e["tags"] == ["ground"] and e["rigidbody"]["type"] == "static" for e in terrain))
        self.assertAlmostEqual(min(e["transform"]["position"][0] - e["size"][0] / 2 for e in terrain), -WORLD_SIZE / 2, places=4)
        self.assertAlmostEqual(max(e["transform"]["position"][2] + e["size"][2] / 2 for e in terrain), WORLD_SIZE / 2, places=4)
        self.assertTrue(all("collider" not in e for e in scene["entities"][GRID * GRID:]))

    def test_zero_density_retains_terrain_and_player(self):
        self.assertEqual(len(generate(density=0)["entities"]), 4097)
        self.assertEqual(len(generate(density=.5)["entities"]), 9547)

    def test_cli_output_and_invalid_density(self):
        script = Path(__file__).with_name("gen_large_world.py")
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "scene.json"
            result = subprocess.run([sys.executable, str(script), "--out", str(output), "--density", "0"], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(len(json.loads(output.read_text())["entities"]), 4097)
            for density in ["-1", "nan", "inf"]:
                result = subprocess.run([sys.executable, str(script), "--out", str(output), "--density", density], capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("finite and nonnegative", result.stderr)


if __name__ == "__main__":
    unittest.main()
