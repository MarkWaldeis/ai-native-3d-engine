"""Blender bridge payload; PROJECT_ROOT and REPO_ROOT are explicit inputs."""
import importlib.util
import json
import math
from pathlib import Path
import random
import sys
import bpy

sys.dont_write_bytecode = True
project = Path(PROJECT_ROOT)
bpy.ops.object.select_all(action='SELECT')
bpy.ops.object.delete(use_global=False)


def material(name, color):
    mat = bpy.data.materials.new(name)
    mat.diffuse_color = (*color, 1)
    mat.use_nodes = True
    mat.node_tree.nodes['Principled BSDF'].inputs['Base Color'].default_value = (*color, 1)
    return mat


earth = material('Waldboden', (.15, .25, .13))
bark = material('Rinde', (.22, .12, .065))
leaf = material('Tannengrün', (.13, .37, .19))
stone = material('Granit', (.34, .4, .36))
gold = material('Bernstein', (.95, .66, .13))
trail = material('Pfadsteine', (.62, .51, .29))


def finish(obj, name, mat, location=(0, 0, 0), tags=('floating',)):
    obj.name = name
    obj.data.materials.append(mat)
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    obj.location = (location[0], -location[2], location[1])
    obj['ai_uid'] = 'wald-' + name
    obj['ai_tags'] = json.dumps(list(tags))
    return obj


bpy.ops.mesh.primitive_cube_add(size=1)
floor = bpy.context.object
floor.scale = (512, 512, .25)
finish(floor, 'Waldboden', earth, (0, -.125, 0), ('ground',))
floor['ai_collider'] = json.dumps({'shape': 'box', 'size': [512, .25, 512]})
floor['ai_rigidbody'] = json.dumps({'type': 'static'})

spec = importlib.util.spec_from_file_location('wald_rig', Path(REPO_ROOT) / 'tools/sky_garden/rigged_character.py')
rig_module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rig_module)
player = rig_module.build(clear=False)
player.name = 'Forstläuferin'
player['ai_uid'] = 'wald-player'
player['ai_tags'] = '["player"]'
player['ai_script'] = 'scripts/player.rhai'
player['ai_collider'] = json.dumps({'shape': 'capsule', 'radius': .35, 'half_height': .3})
player['ai_rigidbody'] = json.dumps({'type': 'dynamic', 'mass': 1, 'friction': 0})
player['ai_lock_rotation'] = True
player['ai_friction_combine'] = 'min'
rig = player.parent
rig.name = 'Forstläuferin-Rig'
rig['ai_uid'] = 'wald-player-rig'
rig.location = (0, 0, .75)
player['ai_ui'] = json.dumps({'title': 'Waldwächter', 'score_label': 'Relikte', 'collectible_tag': 'relic', 'hud_keys': ['score', 'Erwischt'], 'action_label': 'SPRINGEN', 'sky_color': '#b3c9c0', 'mission': 'Fünf Relikte. Ein Waldpfad. Bleib den Wächtern fern.', 'intro_title': 'Der Wald erinnert sich.', 'intro_text': 'Sammle fünf goldene Relikte entlang der Wegsteine. Die Wächter patrouillieren nördlich des Pfades. Halte Abstand! Nach dem dritten Relikt schützt dich ein Wald-Anker. Erwischt? Du kehrst dorthin zurück, ohne Relikte zu verlieren. Joystick oder WASD/Pfeile; Springen mit Aktion/Leertaste.', 'win_text': 'Alle fünf Relikte sind geborgen. Der Wald ist wieder im Gleichgewicht!', 'win_badge': 'HÜTERIN DES WALDES', 'badge': 'WALDWÄCHTER · EIN WALDABENTEUER'}, ensure_ascii=False)
player['ai_world'] = json.dumps({'chunk_size': 128, 'radius': 2, 'unload_radius': 3, 'camera_distance': 18})

guard_mat = material('Wächtermantel', (.6, .24, .12))
for index, x in enumerate((0, 16, 32)):
    guard_rig = rig.copy()
    guard_rig.data = rig.data.copy()
    bpy.context.scene.collection.objects.link(guard_rig)
    guard_rig.name = 'Wächter-Rig-' + str(index)
    guard_rig['ai_uid'] = 'wald-guard-rig-' + str(index)
    guard_rig.location = (x, -6, .65)
    guard = player.copy()
    guard.data = player.data.copy()
    bpy.context.scene.collection.objects.link(guard)
    guard.parent = guard_rig
    guard.name = 'Wächter-' + str(index)
    guard['ai_uid'] = 'wald-guard-' + str(index)
    for key in ('ai_ui', 'ai_world', 'ai_collider', 'ai_rigidbody', 'ai_lock_rotation', 'ai_friction_combine'):
        del guard[key]
    guard['ai_tags'] = '["guard", "npc", "floating"]'
    guard['ai_script'] = 'scripts/guard.rhai'
    for modifier in guard.modifiers:
        if modifier.type == 'ARMATURE':
            modifier.object = guard_rig
    for slot, mat in enumerate(guard.data.materials):
        if mat.name == 'coat':
            guard.data.materials[slot] = guard_mat

for kind in ('Tanne', 'Fels'):
    collection = bpy.data.collections.new(kind)
    collection['ai_uid'] = 'wald-' + kind
    for level, vertices in enumerate((12, 8, 4)):
        parts = []
        if kind == 'Tanne':
            bpy.ops.mesh.primitive_cone_add(vertices=vertices, radius1=.3, radius2=.2, depth=3, location=(0, 0, 1.5))
            trunk = bpy.context.object
            trunk.data.materials.append(bark)
            parts.append(trunk)
            for z, radius in ((2.5, 1.7), (3.6, 1.3), (4.6, .85)):
                bpy.ops.mesh.primitive_cone_add(vertices=vertices, radius1=radius, radius2=0, depth=2.8, location=(0, 0, z))
                obj = bpy.context.object
                obj.data.materials.append(leaf)
                parts.append(obj)
        else:
            bpy.ops.mesh.primitive_uv_sphere_add(segments=vertices, ring_count=max(3, vertices // 2), radius=1, location=(0, 0, .5))
            obj = bpy.context.object
            obj.scale = (1.2, .85, .8)
            obj.data.materials.append(stone)
            parts.append(obj)
        bpy.ops.object.select_all(action='DESELECT')
        for obj in parts:
            obj.select_set(True)
        bpy.context.view_layer.objects.active = parts[0]
        bpy.ops.object.join()
        obj = bpy.context.object
        bpy.context.scene.cursor.location = (0, 0, 0)
        bpy.ops.object.origin_set(type='ORIGIN_CURSOR')
        bpy.ops.object.transform_apply(location=False, rotation=True, scale=True)
        obj.name = kind + '_LOD' + str(level)
        obj['ai_uid'] = 'wald-' + obj.name
        obj['ai_tags'] = json.dumps(['scenery', 'floating', 'tree' if kind == 'Tanne' else 'rock'])
        for parent in list(obj.users_collection):
            parent.objects.unlink(obj)
        collection.objects.link(obj)
    rng = random.Random(71023 if kind == 'Tanne' else 71024)
    for index in range(500 if kind == 'Tanne' else 200):
        x, z = rng.uniform(-250, 250), rng.uniform(-250, 250)
        while -10 < x < 54 and -6 < z < 12:
            x, z = rng.uniform(-250, 250), rng.uniform(-250, 250)
        if kind == 'Tanne' and index < 100:
            x, z = (index % 25) * 3 - 12, (-10, -7, 13, 16)[index // 25]
        obj = bpy.data.objects.new(kind + '-' + str(index), None)
        obj.instance_type, obj.instance_collection = 'COLLECTION', collection
        obj['ai_uid'] = 'wald-instance-' + kind + '-' + str(index)
        bpy.context.scene.collection.objects.link(obj)
        obj.location = (x, -z, 0)
        obj.rotation_euler.z = rng.uniform(-math.pi, math.pi)
        size = rng.uniform(.8, 1.4)
        obj.scale = (size, size, size)

for index in range(5):
    bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=2, radius=.48)
    finish(bpy.context.object, 'Relikt-' + str(index + 1), gold, ((index + 1)*8, .9, 0), ('relic', 'collectible', 'floating'))
for index in range(23):
    bpy.ops.mesh.primitive_cube_add(size=1)
    obj = bpy.context.object
    obj.scale = (.8, .45, .08)
    finish(obj, 'Wegstein-' + str(index), trail, (index*2, .04, 0))
bpy.ops.mesh.primitive_torus_add(major_radius=1.2, minor_radius=.12)
finish(bpy.context.object, 'Wald-Anker', gold, (24, .18, 0), ('checkpoint', 'floating'))
bpy.context.scene['waldwaechter_seed'] = 71023
bpy.ops.wm.save_as_mainfile(filepath=str(project / 'waldwaechter.blend'))
result = {'collection_instances': 700, 'tree_instances': 500, 'rock_instances': 200, 'world_meters': 512, 'seed': 71023, 'rigged_actors': 4}
