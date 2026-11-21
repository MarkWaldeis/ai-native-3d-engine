"""Read-only external/custom-property reference inventory; run with autoexec disabled."""
import json
import sys
import bpy


def strings(value):
    if isinstance(value, str):
        yield value
    elif hasattr(value, "values"):
        for child in value.values():
            yield from strings(child)
    elif isinstance(value, (list, tuple)):
        for child in value:
            yield from strings(child)


paths = []
libraries = []
for filename in sys.argv[sys.argv.index("--") + 1:]:
    bpy.ops.wm.open_mainfile(filepath=filename, load_ui=False, use_scripts=False)
    paths.extend(bpy.utils.blend_paths(absolute=True, packed=False))
    libraries.extend(bpy.path.abspath(library.filepath, library=library.parent) for library in bpy.data.libraries)
    for block in bpy.data.user_map():
        paths.extend(strings(block))
print("ASSET_GC_REFERENCES=" + json.dumps({'paths': paths, 'libraries': libraries}))
