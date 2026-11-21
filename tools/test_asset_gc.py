import importlib.util
import json
from pathlib import Path
import struct
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('asset_gc', Path(__file__).with_name('asset_gc.py'))
gc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gc)


def glb():
    data = json.dumps({'asset': {'version': '2.0'}}).encode()
    data += b' ' * (-len(data) % 4)
    return struct.pack('<4sIIII', b'glTF', 2, 20 + len(data), len(data), 0x4E4F534A) + data


class AssetGC(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='asset-gc-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'assets').mkdir()
        (self.root / 'scene.json').write_text('{}')

    def asset(self, name):
        path = self.root / 'assets' / name
        path.write_bytes(glb())
        return path

    def test_dry_default_and_explicit_removal(self):
        dead, live = self.asset('unused.glb'), self.asset('live.glb')
        (self.root / 'scene.json').write_text(json.dumps({'prefabs': {'tree': {'path': 'assets/live.glb'}}}))
        report = gc.collect(self.root)
        self.assertTrue(report['dry_run'])
        self.assertEqual(report['unreferenced'], ['assets/unused.glb'])
        self.assertTrue(dead.exists())
        report = gc.collect(self.root, apply=True)
        self.assertEqual(report['removed'], ['assets/unused.glb'])
        self.assertTrue(live.exists())

    def test_persisted_history_and_non_glb_assets_retained(self):
        self.asset('history.glb')
        (self.root / '.ai-engine').mkdir()
        (self.root / '.ai-engine' / 'old.json').write_text(json.dumps({'world': {'snapshots': [{'path': 'assets/history.glb'}]}}))
        source = self.root / 'assets' / 'artist.py'
        source.write_text('source = True')
        self.assertEqual(gc.collect(self.root, apply=True)['removed'], [])
        self.assertTrue(source.exists())

    def test_empty(self):
        self.assertEqual(gc.collect(self.root)['unreferenced'], [])

    def test_dynamic_inline_and_external_code_conservatively_retain_assets(self):
        live=self.asset('generated-name.glb')
        (self.root/'scene.json').write_text(json.dumps({'script':{'source':'fn on_tick(ctx,dt){ctx.spawn_entity(#{kind:"gl"+"tf",path: "assets/"+ctx.state_get("model")+".glb"});}'}}))
        self.assertEqual(gc.collect(self.root,apply=True)['removed'],[])
        (self.root/'scene.json').write_text('{}')
        (self.root/'build.py').write_text('name = "generated-" + "name.glb"')
        self.assertEqual(gc.collect(self.root,apply=True)['removed'],[])
        self.assertTrue(live.exists())

    def test_manifest_keys_and_alternate_scene_extensions_protect_assets(self):
        live=self.asset('live.glb')
        (self.root/'backup.scene').write_text('{"assets/live.glb": {"enabled":true}}')
        self.assertEqual(gc.collect(self.root,apply=True)['removed'],[])
        self.assertTrue(live.exists())

    def test_unknown_binary_inventory_retains_assets(self):
        live=self.asset('live.glb')
        (self.root/'unknown.database').write_bytes(b'\xff\xfe\x00')
        self.assertTrue(gc.collect(self.root)['retained_all_glbs'])
        self.assertEqual(gc.collect(self.root,apply=True)['removed'],[])
        self.assertTrue(live.exists())

    def test_references_in_build_named_directories_are_not_discarded(self):
        live = self.asset('old-save.glb')
        (self.root / 'target').mkdir()
        (self.root / 'target' / 'save.json').write_text(json.dumps({'path': 'assets/old-save.glb'}))
        self.assertEqual(gc.collect(self.root, apply=True)['removed'], [])
        self.assertTrue(live.exists())

    def test_invalid_references_and_assets_fail_closed(self):
        dead = self.asset('unused.glb')
        (self.root / 'broken.json').write_text('{')
        with self.assertRaises(ValueError):
            gc.collect(self.root, apply=True)
        self.assertTrue(dead.exists())
        (self.root / 'broken.json').unlink()
        dead.write_bytes(b'not gltf')
        with self.assertRaises(ValueError):
            gc.collect(self.root, apply=True)
        self.assertTrue(dead.exists())

    @unittest.skipIf(gc.os.name == 'nt', 'symlink privilege unavailable on Windows runners')
    def test_symlink_and_live_writer_refused(self):
        self.asset('unused.glb')
        alias = self.root / 'assets' / 'alias.glb'
        alias.symlink_to(self.root / 'assets' / 'unused.glb')
        with self.assertRaises(ValueError):
            gc.collect(self.root, apply=True)
        alias.unlink()
        with gc.writer_lock(self.root):
            with self.assertRaises(OSError):
                gc.collect(self.root, apply=True)

    def test_blender_sources_fail_closed_without_inventory(self):
        source = self.root / 'character.blend1'
        source.write_bytes(b'not a blend')
        dead = self.asset('unused.glb')
        with self.assertRaises((OSError, gc.subprocess.SubprocessError, ValueError)):
            gc.collect(self.root, apply=True, blender='definitely-unavailable-blender')
        self.assertTrue(source.exists())
        self.assertTrue(dead.exists())

    def test_real_blender_dependency_inventory_preserves_source(self):
        blender = gc.shutil.which('blender')
        if not blender and Path('/Applications/Blender.app/Contents/MacOS/Blender').is_file():
            blender = '/Applications/Blender.app/Contents/MacOS/Blender'
        if not blender:
            self.skipTest('real Blender unavailable; dependency-audit integration is local-only')
        source = self.root / 'forest.blend'
        live, dead = self.asset('live.glb'), self.asset('unused.glb')
        keyed=self.asset('keyed.glb')
        code = f"import bpy; bpy.context.object['runtime_asset']='assets/live.glb'; bpy.context.object['assets/keyed.glb']=True; bpy.ops.wm.save_as_mainfile(filepath={str(source)!r})"
        gc.subprocess.run([blender, '--background', '--factory-startup', '--python-expr', code], check=True, capture_output=True, timeout=120)
        before = source.read_bytes()
        report = gc.collect(self.root, apply=True, blender=blender)
        self.assertEqual(report['removed'], ['assets/unused.glb'])
        self.assertEqual(report['blender_sources_protected'], 1)
        self.assertTrue(live.exists())
        self.assertTrue(keyed.exists())
        self.assertFalse(dead.exists())
        self.assertEqual(source.read_bytes(), before)
        dead=self.asset('script-generated.glb')
        code=f"import bpy; bpy.ops.wm.open_mainfile(filepath={str(source)!r},use_scripts=False); bpy.data.texts.new('generator').write('model = chr(120) + chr(121)'); bpy.ops.wm.save_as_mainfile(filepath={str(source)!r})"
        gc.subprocess.run([blender,'--background','--factory-startup','--disable-autoexec','--python-expr',code],check=True,capture_output=True,timeout=120)
        before=source.read_bytes()
        report=gc.collect(self.root,apply=True,blender=blender)
        self.assertTrue(report['retained_all_glbs'])
        self.assertTrue(dead.exists())
        self.assertEqual(source.read_bytes(),before)


if __name__ == '__main__':
    unittest.main()
