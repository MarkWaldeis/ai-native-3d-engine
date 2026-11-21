"""Execute with blender_exec; render via render_comparison(path)."""
import math
import threading

import bpy
from mathutils import Vector

assert threading.current_thread() is threading.main_thread()
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)


def material(name, color):
    mat = bpy.data.materials.new(name)
    mat.diffuse_color = color
    if bpy.app.version < (5, 0, 0):
        mat.use_nodes = True
    mat.node_tree.nodes.get("Principled BSDF").inputs["Base Color"].default_value = color
    return mat


red = material("Terracotta", (0.8, 0.12, 0.08, 1))
gold = material("Gold", (0.9, 0.48, 0.06, 1))
gray = material("Floor", (0.26, 0.31, 0.36, 1))
bpy.ops.mesh.primitive_monkey_add(location=(-2, 0, 1.05))
monkey = bpy.context.object
monkey.name = "BridgeMonkey"
monkey.rotation_euler = (0, 0, math.radians(-25))
monkey.data.materials.append(red)
monkey["ai_role"] = "landmark"
monkey["ai_gameplay"] = {"collider": "mesh", "health": 100}
bpy.ops.mesh.primitive_cube_add(size=2, location=(2, -1, 0.8))
cube = bpy.context.object
cube.name = "BridgeCube"
cube.rotation_euler = (0, 0, math.radians(35))
cube.scale = (0.65, 1.1, 0.8)
cube.data.materials.append(gold)
cube["ai_role"] = "movable"
bpy.ops.mesh.primitive_plane_add(size=14)
floor = bpy.context.object
floor.name = "BridgeFloor"
floor.data.materials.append(gray)


def render_comparison(path):
    scene = bpy.context.scene
    bpy.ops.object.camera_add(location=(7, -9, 5))
    camera = bpy.context.object
    camera.name = "ComparisonCamera"
    camera.rotation_euler = (Vector((0, 0, 1)) - camera.location).to_track_quat("-Z", "Y").to_euler()
    camera.data.sensor_fit = "VERTICAL"
    camera.data.lens = camera.data.sensor_height / (2 * math.tan(math.radians(45) / 2))
    scene.camera = camera
    scene.render.engine = "BLENDER_WORKBENCH"
    scene.render.resolution_x = 960
    scene.render.resolution_y = 540
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.render.filepath = path
    scene.display.shading.light = "STUDIO"
    scene.display.shading.color_type = "MATERIAL"
    scene.display.shading.show_shadows = False
    scene.display.shading.show_cavity = False
    scene.display.shading.show_object_outline = False
    scene.display.shading.background_type = "WORLD"
    scene.world.color = (0.04, 0.05, 0.08)
    bpy.ops.render.render(write_still=True)


result = {"objects": [monkey.name, cube.name, floor.name]}
