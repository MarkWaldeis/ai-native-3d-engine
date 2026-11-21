import json
import struct
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from gen_large_world import generate, TerrainSampler, GRID, WORLD_SIZE


class GeneratorTests(unittest.TestCase):
    def terrain_fixture(self, directory):
        path = Path(directory) / "terrain.json"
        path.write_text(json.dumps(dict(config=dict(size_m=[WORLD_SIZE] * 2, tile_m=WORLD_SIZE, samples=2),
                                       grid=[1, 1], height_range=[100, 300],
                                       tiles=[dict(x=0, z=0, file="tile.f32")])))
        heights = [100.0] * 16
        heights[10] = 300.0
        (path.parent / "tile.f32").write_bytes(struct.pack("<16f", *heights))
        return path

    def test_streamed_terrain_preserves_density_and_places_props_and_flight(self):
        with tempfile.TemporaryDirectory() as directory:
            path = self.terrain_fixture(directory)
            scene = generate(terrain_manifest=path)
            entities = scene["entities"]
            self.assertEqual(len(entities), 14998)
            self.assertEqual(len({e["id"] for e in entities}), 14998)
            self.assertEqual(sum("collider" in e for e in entities), 4096)
            self.assertEqual(sum(e["name"].startswith("trunk-") for e in entities), 4000)
            terrain = entities[-1]
            self.assertEqual(terrain["props"]["ai_terrain"], str(path))
            self.assertEqual(terrain["transform"]["position"], [-WORLD_SIZE / 2, 0, -WORLD_SIZE / 2])
            self.assertEqual(entities[0]["transform"]["position"][1] + entities[0]["size"][1] / 2, 90)
            tree = entities[GRID * GRID]
            x, y, z = tree["transform"]["position"]
            self.assertAlmostEqual(y - tree["height"] / 2, TerrainSampler(path).sample(x, z), places=4)
            self.assertIn("ctx.terrain_height(x, z) + 8.0", entities[-2]["script"]["source"])
            self.assertEqual(len(generate(density=0, terrain_manifest=path)["entities"]), 4098)

    def test_native_sampling_triangle_clamping_and_invalid_files(self):
        with tempfile.TemporaryDirectory() as directory:
            path = self.terrain_fixture(directory)
            sampler = TerrainSampler(path)
            self.assertEqual(sampler.sample(-WORLD_SIZE, -WORLD_SIZE), 100)
            self.assertEqual(sampler.sample(WORLD_SIZE, WORLD_SIZE), 300)
            self.assertEqual(sampler.sample(0, 0), 100)
            self.assertAlmostEqual(sampler.sample(WORLD_SIZE / 4, WORLD_SIZE / 4), 200)
            (path.parent / "tile.f32").write_bytes(b"")
            with self.assertRaisesRegex(ValueError, "tile length"):
                TerrainSampler(path).sample(0, 0)
            with self.assertRaises(FileNotFoundError):
                TerrainSampler(path.parent / "missing.json")

    def test_low_terrain_foundations_have_positive_sizes_and_stay_below_surface(self):
        with tempfile.TemporaryDirectory() as directory:
            path = self.terrain_fixture(directory)
            manifest = json.loads(path.read_text())
            for minimum in [2, 0, -100]:
                with self.subTest(minimum=minimum):
                    manifest["height_range"][0] = minimum
                    path.write_text(json.dumps(manifest))
                    foundations = generate(density=0, terrain_manifest=path)["entities"][:GRID * GRID]
                    for foundation in foundations:
                        self.assertTrue(all(size > 0 for size in foundation["size"]))
                        self.assertEqual(foundation["transform"]["position"][1] + foundation["size"][1] / 2, minimum - 10)

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
