"""Blender-Artist payload. Executed only through blender_exec over MCP HTTP."""
import bpy
import json
import math
import random
import importlib.util
from mathutils import Vector

random.seed(17)
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)


def material(name, color, emission=0.0, metallic=0.0):
    mat = bpy.data.materials.new(name)
    mat.diffuse_color = (*color, 1)
    mat.use_nodes = True
    node = mat.node_tree.nodes.get("Principled BSDF")
    node.inputs["Base Color"].default_value = (*color, 1)
    node.inputs["Roughness"].default_value = 0.65
    node.inputs["Metallic"].default_value = metallic
    node.inputs["Emission Color"].default_value = (*color, 1)
    node.inputs["Emission Strength"].default_value = emission
    return mat


moss = material("Moos · Salbei", (0.16, 0.43, 0.29))
grass = material("Gras · Minze", (0.34, 0.66, 0.37))
rock = material("Fels · Pfirsich", (0.53, 0.33, 0.25))
rock_light = material("Fels · Sandstein", (0.76, 0.54, 0.35))
gold = material("Portal · Messing", (0.86, 0.53, 0.13), metallic=0.6)
cyan = material("Kristall · Himmel", (0.13, 0.75, 0.86), emission=0.45, metallic=0.2)
violet = material("Kristall · Lavendel", (0.56, 0.29, 0.86), emission=0.3)
pink = material("Blüten · Koralle", (0.97, 0.37, 0.40))
cream = material("Wächter · Vanille", (0.96, 0.83, 0.57))
ink = material("Augen · Mitternacht", (0.025, 0.07, 0.09))
cloud = material("Wolken · Porzellan", (0.82, 0.91, 0.92))


def finish(obj, mat):
    obj.data.materials.append(mat)
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    return obj


def ico(location, scale, mat, subdivisions=1):
    bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=subdivisions, radius=1, location=location)
    obj = bpy.context.object
    obj.scale = scale
    return finish(obj, mat)


def cylinder(location, radius, depth, mat, vertices=12, radius_top=None):
    bpy.ops.mesh.primitive_cone_add(vertices=vertices, radius1=radius,
                                  radius2=radius if radius_top is None else radius_top,
                                  depth=depth, location=location)
    return finish(bpy.context.object, mat)


def torus(location, major, minor, mat, rotation=(0, 0, 0)):
    bpy.ops.mesh.primitive_torus_add(major_radius=major, minor_radius=minor,
                                   major_segments=40, minor_segments=8,
                                   location=location, rotation=rotation)
    return finish(bpy.context.object, mat)


def entity(name, parts, location=(0, 0, 0), tags=("floating",), collider=None, body=None, script=None, center=False):
    bpy.ops.object.select_all(action="DESELECT")
    for part in parts:
        part.select_set(True)
    bpy.context.view_layer.objects.active = parts[0]
    bpy.ops.object.join()
    obj = bpy.context.object
    bpy.context.scene.cursor.location = (0, 0, 0)
    bpy.ops.object.origin_set(type="ORIGIN_CURSOR")
    if center:
        bounds = [Vector(corner) for corner in obj.bound_box]
        offset = (Vector(tuple(min(v[i] for v in bounds) for i in range(3))) +
                  Vector(tuple(max(v[i] for v in bounds) for i in range(3)))) * 0.5
        for vertex in obj.data.vertices:
            vertex.co -= offset
    obj.name = name
    obj.location = location
    obj["ai_uid"] = "sky-garden-" + name
    obj["ai_tags"] = json.dumps(list(tags))
    if collider:
        obj["ai_collider"] = json.dumps(collider)
    if body:
        obj["ai_rigidbody"] = json.dumps(body)
    if script:
        obj["ai_script"] = script
    return obj


for index, x in enumerate((0, 3.5, 7, 10.5)):
    radius = 1.42 if index in (0, 3) else 1.25
    top = cylinder((0, 0, 0), radius, 0.30, moss, vertices=16)
    bevel = top.modifiers.new("Weiche Rasenkante", "BEVEL")
    bevel.width, bevel.segments = 0.10, 3
    bpy.context.view_layer.objects.active = top
    bpy.ops.object.modifier_apply(modifier=bevel.name)
    entity("Insel-%d" % index, [top], (x, 0, 0), tags=("ground",),
           collider={"shape": "box", "half_extents": [1.3 if index in (0, 3) else 1.15, 0.15, 1.15]},
           body={"type": "static", "friction": 0.7})
    parts = [cylinder((0, 0, -0.65), radius * 0.38, 1.1, rock, vertices=7, radius_top=radius * 0.94),
             cylinder((0, 0, -0.22), radius * 0.90, 0.15, rock_light, vertices=12)]
    for n in range(6):
        angle = n * math.tau / 6
        parts.append(ico((math.cos(angle) * radius * 0.7, math.sin(angle) * radius * 0.7, -0.5),
                         (0.32, 0.32, 0.55), rock_light if n % 2 else rock))
    entity("Felswurzel-%d" % index, parts, (x, 0, 0))
    plants = []
    for n in range(8):
        angle = n * math.tau / 8
        px, py = math.cos(angle) * radius * 0.79, math.sin(angle) * radius * 0.79
        # Keep the front of the route open; plants stay behind the player.
        if py < -0.4:
            continue
        plants.append(ico((px, py, 0.34), (0.17, 0.13, 0.32), grass))
        plants.append(ico((px + 0.09, py, 0.38), (0.12, 0.13, 0.29), moss))
        if n % 2 == 0:
            plants.append(ico((px, py, 0.66), (0.15, 0.15, 0.11), pink))
    entity("Inselblüten-%d" % index, plants, (x, 0, 0))

# Small sculpted trees and dangling stepping-stone shards give the islands silhouettes.
for n, x in enumerate((0, 7, 10.5)):
    parts = [cylinder((0, 0, 0.55), 0.09, 0.8, rock_light, vertices=7)]
    parts += [ico((dx, dy, z), scale, grass if i % 2 else moss, 2)
              for i, (dx, dy, z, scale) in enumerate((
                  (0, 0, 1.1, (0.65, 0.48, 0.55)),
                  (-0.34, 0, 0.9, (0.42, 0.4, 0.43)),
                  (0.33, 0.03, 1.35, (0.46, 0.36, 0.43))))]
    entity("Wolkenbaum-%d" % n, parts, (x - 0.5, 0.83, 0.15))

for i, x in enumerate((3.5, 7, 10.25)):
    parts = [cylinder((0, 0, 0), 0.26, 0.60, cyan if i % 2 == 0 else violet,
                      vertices=6, radius_top=0),
             cylinder((0, 0, -0.30), 0, 0.3, cyan if i % 2 == 0 else violet,
                      vertices=6, radius_top=0.26),
             torus((0, 0, -0.48), 0.31, 0.025, gold)]
    entity("Kristall-%d" % (i + 1), parts, (x, 0, 1.15), tags=("crystal", "floating"),
           script="examples/sky_garden/scripts/crystal.rhai")

portal_parts = [torus((0, 0, 0), 0.86, 0.12, gold, (math.pi / 2, 0, 0)),
                torus((0, -0.02, 0), 0.69, 0.055, cyan, (math.pi / 2, 0, 0))]
for n in range(8):
    angle = n * math.tau / 8
    portal_parts.append(ico((math.cos(angle) * 0.86, -0.08, math.sin(angle) * 0.86),
                            (0.10, 0.12, 0.10), cyan))
portal_parts.append(cylinder((0, 0, -1.0), 0.80, 0.20, rock_light, vertices=12))
entity("Portal", portal_parts, (11, 0, 1.3), tags=("portal", "floating"))

player_parts = [ico((0, 0, 0), (0.34, 0.28, 0.39), cream, 3),
                ico((0, 0.03, -0.15), (0.35, 0.29, 0.23), moss, 2)]
for sign in (-1, 1):
    player_parts += [ico((sign * 0.13, -0.252, 0.10), (0.065, 0.045, 0.085), ink, 2),
                     ico((sign * 0.14, -0.09, -0.36), (0.15, 0.21, 0.09), rock_light, 2),
                     ico((sign * 0.37, 0, -0.02), (0.13, 0.15, 0.12), cream, 2)]
player_parts += [cylinder((0, 0, 0.48), 0.033, 0.28, moss, 8),
                 ico((0.13, 0, 0.63), (0.24, 0.09, 0.11), grass, 2),
                 ico((-0.10, 0, 0.57), (0.19, 0.08, 0.10), moss, 2),
                 torus((0, 0, 0.25), 0.26, 0.055, pink)]
player = entity("Milo", player_parts, (0, 0, 0.81), tags=("player",), center=True,
                collider={"shape": "capsule", "radius": 0.35, "half_height": 0.25},
                body={"type": "dynamic", "mass": 1, "friction": 0.0},
                script="examples/sky_garden/scripts/player.rhai")
player["ai_lock_rotation"] = True
player["ai_friction_combine"] = "min"
player["ai_ui"] = json.dumps({"title": "Himmelsgarten", "score_label": "Kristalle", "hud_keys": ["score"],
    "collectible_tag": "crystal", "action_label": "SPRINGEN", "sky_color": "#bed9dc",
    "mission": "Drei Kristalle. Vier Inseln. Dein Weg nach Hause.",
    "intro_title": "Ein Sprung ins Grüne.",
    "intro_text": "Hilf Milo durch den Himmelsgarten. Sammle drei Kristalle, springe über die Lücken und erreiche das leuchtende Portal. Nach einem Sturz geht es am letzten Kristall weiter.",
    "win_text": "Alle Kristalle leuchten. Milo hat den Weg nach Hause gefunden!", "win_badge": "DER GARTEN LEUCHTET WIEDER",
    "badge": "HIMMELSGARTEN · EIN KLEINES ABENTEUER"}, ensure_ascii=False)

for n, (x, y, z, scale) in enumerate(((-4, 5, -2.8, 1.7), (4, 7, -3.3, 2.2),
                                     (13, 6, -2.6, 1.8), (2, -5, -4.5, 1.5),
                                     (10, -6, -4, 2.0))):
    parts = [ico((dx * scale, 0, dz * scale), (sx * scale, 0.55 * scale, sz * scale), cloud, 2)
             for dx, dz, sx, sz in ((-0.6, 0, 0.8, 0.45), (0, 0.2, 0.85, 0.62), (0.7, 0, 0.75, 0.4))]
    entity("Wolkeninsel-%d" % n, parts, (x, y, z))

recipe = Path(PROJECT_ROOT) / "rigged_character.py"
if not recipe.is_file():
    recipe = Path(PROJECT_ROOT) / "tools/sky_garden/rigged_character.py"
spec = importlib.util.spec_from_file_location("milo_builder", recipe)
builder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(builder)
player_props = dict(player.items())
bpy.data.objects.remove(player, do_unlink=True)
player = builder.build(clear=False)
player.find_armature().location = (0, 0, .81)
for key, value in player_props.items():
    player[key] = value

bpy.ops.wm.save_as_mainfile(filepath=str(Path(PROJECT_ROOT) / "examples/sky_garden/sky_garden.blend"))
result = {"meshes": len([o for o in bpy.context.scene.objects if o.type == "MESH"]), "player": player.name}
