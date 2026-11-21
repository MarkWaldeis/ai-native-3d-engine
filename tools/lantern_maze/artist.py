"""Live Blender payload. Set STAGE to scaffold or maze before execution."""
import bpy
import json
import math
from pathlib import Path


def material(name, color, metal=0):
    mat = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    mat.diffuse_color = (*color, 1)
    mat.use_nodes = True
    node = mat.node_tree.nodes.get("Principled BSDF")
    node.inputs["Base Color"].default_value = (*color, 1)
    node.inputs["Roughness"].default_value = .65
    node.inputs["Metallic"].default_value = metal
    return mat


def finish(obj, mat):
    obj.data.materials.append(mat)
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    return obj


def box(location, size, mat, bevel=0):
    bpy.ops.mesh.primitive_cube_add(size=1, location=location)
    obj = bpy.context.object
    obj.scale = size
    finish(obj, mat)
    if bevel:
        modifier = obj.modifiers.new("Weiche Kanten", "BEVEL")
        modifier.width, modifier.segments = bevel, 2
        bpy.ops.object.modifier_apply(modifier=modifier.name)
    return obj


def sphere(location, scale, mat):
    bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=2, radius=1, location=location)
    obj = bpy.context.object
    obj.scale = scale
    return finish(obj, mat)


def cylinder(location, radius, height, mat, top=None):
    bpy.ops.mesh.primitive_cone_add(vertices=12, radius1=radius, radius2=radius if top is None else top, depth=height, location=location)
    return finish(bpy.context.object, mat)


def entity(name, parts, position, tags=("floating",), collider=None, body=None):
    bpy.ops.object.select_all(action="DESELECT")
    for obj in parts:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = parts[0]
    if len(parts) > 1:
        bpy.ops.object.join()
    obj = bpy.context.object
    bpy.context.scene.cursor.location = (0, 0, 0)
    bpy.ops.object.origin_set(type="ORIGIN_CURSOR")
    obj.name = name
    obj.location = (position[0], -position[2], position[1])
    obj["ai_uid"] = "lantern-maze-" + name
    obj["ai_tags"] = json.dumps(list(tags))
    if collider:
        obj["ai_collider"] = json.dumps(collider)
    if body:
        obj["ai_rigidbody"] = json.dumps(body)
    return obj


stone = material("Mauer · Abendblau", (.20, .30, .39))
trim = material("Kante · Schiefer", (.32, .46, .49))
sand = material("Boden · Sand", (.49, .43, .33))
path = material("Weg · Muschelkalk", (.73, .65, .47))
brass = material("Laternen · Messing", (.76, .46, .12), .5)
dark = material("Glas · Ungelöscht", (.055, .12, .19), .15)
cyan = material("Wanderer · Türkis", (.12, .72, .77), .1)
ink = material("Augen · Nacht", (.025, .04, .06))
green = material("Ausgang · Jade", (.16, .73, .39))

if STAGE == "scaffold":
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    entity("Fundament", [box((0, 0, 0), (12, 10, .4), sand, .08)], [0, -.2, 0], ("ground",),
           {"shape": "box", "half_extents": [6, .2, 5]}, {"type": "static", "friction": .7})
    player = entity("Lumi", [sphere((0, 0, 0), (.28, .26, .32), cyan),
                    sphere((-.10, -.235, .08), (.045, .035, .06), ink),
                    sphere((.10, -.235, .08), (.045, .035, .06), ink),
                    cylinder((0, 0, .30), .18, .11, brass)], [-4.5, .4, 3.3], ("player",),
                    {"shape": "sphere", "radius": .32}, {"type": "dynamic", "mass": 1, "friction": 0})
    player["ai_lock_rotation"] = True
    player["ai_friction_combine"] = "min"
    player["ai_ui"] = json.dumps({"title": "Laternen-Labyrinth", "score_label": "Laternen", "score_target": 4,
        "hud_keys": ["score"], "collectible_tag": "lantern", "objective_label": "Nächste Laterne",
        "action_label": "Anzünden", "sky_color": "#344f69", "intro_title": "Licht weist den Weg",
        "intro_text": "Folge den hellen Wegsteinen zu vier dunklen Laternen. Geh nah heran und tippe auf Anzünden oder drücke die Leertaste. Erst dann öffnet sich das Tor. Der Pfeil zeigt die Luftlinie.",
        "mission": "Vier Laternen anzünden · dann zum grünen Ausgang", "win_text": "Alle vier Laternen leuchten. Das Labyrinth ist erwacht!",
        "win_badge": "LICHT GEFUNDEN", "badge": "ABENDSPAZIERGANG"}, ensure_ascii=False)
elif STAGE == "maze":
    walls = [("West", -5.9, 0, .2, 10), ("Ost", 5.9, 0, .2, 10),
             ("Nord", 0, -4.9, 12, .2), ("Süd", 0, 4.9, 12, .2),
             ("Gang-A", -2, 1.5, .32, 5), ("Gang-B", 1.2, -1.5, .32, 5),
             ("Gang-C", 3.6, 1.8, .32, 4.4)]
    for name, x, z, width, depth in walls:
        entity("Mauer-" + name, [box((0, 0, 0), (width, depth, .9), stone, .045),
            box((0, 0, .45), (width + .07, depth + .07, .08), trim, .025)], [x, .45, z], ("wall",),
            {"shape": "box", "half_extents": [width/2, .5, depth/2]}, {"type": "static", "friction": 0})
    for index, (x, z) in enumerate([(-4.9, -3.3), (-.8, -3.3), (-.8, 3.3), (2.5, 3.3)], 1):
        entity("Laternenfuß-" + str(index), [cylinder((0, 0, .08), .26, .16, stone),
            cylinder((0, 0, .45), .07, .7, brass), cylinder((0, 0, .80), .21, .10, brass),
            cylinder((0, 0, 1.18), .30, .22, brass, 0),
            *[cylinder((dx, dy, .97), .025, .36, brass) for dx, dy in [(-.16, -.16), (-.16, .16), (.16, -.16), (.16, .16)]]], [x, 0, z])
        entity("Laterne-" + str(index), [sphere((0, 0, 0), (.16, .16, .18), dark)], [x, .98, z], ("lantern", "floating"))
    entity("Torrahmen", [box((-.9, 0, .8), (.25, .35, 1.6), stone, .05),
        box((.9, 0, .8), (.25, .35, 1.6), stone, .05), box((0, 0, 1.7), (2.1, .4, .25), brass, .06)], [4.8, 0, -3.5])
    entity("Tor-Riegel", [box((x, 0, 0), (.075, .12, 1.5), brass, .025) for x in [-.6, -.3, 0, .3, .6]] +
        [box((0, 0, height), (1.5, .12, .08), brass) for height in [-.6, .6]], [4.8, .8, -3.5], ("gate_bar",),
        {"shape": "box", "half_extents": [.75, .8, .06]}, {"type": "static", "friction": 0})
    entity("Ausgang", [cylinder((0, 0, 0), .53, .06, green)], [4.8, .04, -4.1], ("goal", "floating"))
    points = [(-4.5, 3.3), (-4.5, -3), (-.5, -3), (-.5, 3), (2.5, 3), (2.5, -2), (4.8, -2), (4.8, -4.1)]
    stones = []
    for (x1, z1), (x2, z2) in zip(points, points[1:]):
        length = math.hypot(x2-x1, z2-z1)
        for i in range(max(1, int(length/.8))):
            fraction = (i+.5)/max(1, int(length/.8))
            stones.append(box((x1+(x2-x1)*fraction, -(z1+(z2-z1)*fraction), .012), (.30, .30, .022), path, .04))
    entity("Wegsteine", stones, [0, 0, 0])
    bpy.ops.wm.save_as_mainfile(filepath=str(Path(PROJECT_ROOT) / "lantern_maze.blend"))
else:
    raise ValueError("STAGE must be scaffold or maze")
print(json.dumps({"stage": STAGE, "objects": [obj.name for obj in bpy.context.scene.objects]}))
