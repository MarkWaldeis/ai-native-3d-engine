"""Keep the no-import Rhai terrain helper copies in sync."""
from pathlib import Path
import unittest


SCRIPTS = Path(__file__).resolve().parents[2] / 'examples/open_world_shooter/scripts'


class TerrainHelpersTest(unittest.TestCase):
    def test_scripts_start_with_identical_helpers(self):
        helpers = (SCRIPTS / '_terrain.rhai.part').read_text()
        for name in ['player', 'soldier', 'drone', 'barrel']:
            with self.subTest(script=name):
                self.assertTrue((SCRIPTS / (name + '.rhai')).read_text().startswith(helpers))


if __name__ == '__main__':
    unittest.main()
