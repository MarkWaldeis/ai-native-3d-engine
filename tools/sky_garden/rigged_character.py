"""Run inside Blender: builds an in-place skinned Milo with three reusable actions."""
import argparse
import importlib.util
import json
import math
from pathlib import Path
import sys

import bpy
from mathutils import Matrix, Vector


def build(clear=True):
    if clear:
        bpy.ops.object.select_all(action="SELECT")
        bpy.ops.object.delete(use_global=False)
    bpy.ops.object.armature_add()
    rig = bpy.context.object
    rig.name = "MiloRig"
    rig["ai_uid"] = "milo-rig"
    bpy.ops.object.mode_set(mode="EDIT")
    rig.data.edit_bones.remove(rig.data.edit_bones[0])
    bones = {
        "root": ((0, 0, -.2), (0, 0, .1), None),
        "head": ((0, 0, .25), (0, 0, .65), "root"),
        "arm_l": ((-.24, 0, .23), (-.42, 0, -.15), "root"),
        "arm_r": ((.24, 0, .23), (.42, 0, -.15), "root"),
        "leg_l": ((-.14, 0, -.15), (-.14, 0, -.6), "root"),
        "leg_r": ((.14, 0, -.15), (.14, 0, -.6), "root"),
    }
    for name, (head, tail, parent) in bones.items():
        bone = rig.data.edit_bones.new(name)
        bone.head, bone.tail = head, tail
        if parent:
            bone.parent = rig.data.edit_bones[parent]
    bpy.ops.object.mode_set(mode="OBJECT")
    palette = {}
    for name, color in {"coat": (.11, .48, .42, 1), "skin": (.92, .66, .4, 1), "boots": (.12, .16, .22, 1), "eyes": (.025, .035, .04, 1)}.items():
        material = bpy.data.materials.new(name)
        material.diffuse_color = color
        palette[name] = material
    pieces = []
    for name, center, size, bone, material in [
        ("coat", (0, 0, .02), (.48, .3, .49), "root", "coat"),
        ("head", (0, 0, .49), (.4, .32, .36), "head", "skin"),
        ("arm_l", (-.34, 0, .02), (.15, .22, .45), "arm_l", "coat"),
        ("arm_r", (.34, 0, .02), (.15, .22, .45), "arm_r", "coat"),
        ("leg_l", (-.14, 0, -.4), (.18, .27, .43), "leg_l", "boots"),
        ("leg_r", (.14, 0, -.4), (.18, .27, .43), "leg_r", "boots"),
        ("eye_l", (-.09, -.166, .52), (.045, .025, .055), "head", "eyes"),
        ("eye_r", (.09, -.166, .52), (.045, .025, .055), "head", "eyes"),
    ]:
        bpy.ops.mesh.primitive_cube_add(size=1, location=center)
        obj = bpy.context.object
        obj.name = name
        obj.dimensions = size
        bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
        group = obj.vertex_groups.new(name=bone)
        group.add(list(range(len(obj.data.vertices))), 1.0, "REPLACE")
        obj.data.materials.append(palette[material])
        pieces.append(obj)
    bpy.ops.object.select_all(action="DESELECT")
    for obj in pieces:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = pieces[0]
    bpy.ops.object.join()
    mesh = bpy.context.object
    mesh.name = "Milo"
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    bounds = [Vector(corner) for corner in mesh.bound_box]
    center = Vector(tuple((min(v[i] for v in bounds) + max(v[i] for v in bounds)) * .5 for i in range(3)))
    mesh.data.transform(Matrix.Translation(-center))
    mesh.data.update()
    mesh.select_set(False)
    rig.select_set(True)
    bpy.context.view_layer.objects.active = rig
    bpy.ops.object.mode_set(mode="EDIT")
    for bone in rig.data.edit_bones:
        bone.head -= center
        bone.tail -= center
    bpy.ops.object.mode_set(mode="OBJECT")
    mesh["ai_uid"] = "sky-garden-Milo"
    transitions = []
    for state in ["idle", "walk"]:
        transitions.append({"from":state,"to":"jump","conditions":[{"parameter":"grounded","op":"eq","value":0}]})
    for source, target, op in [("idle","walk","gt"),("walk","idle","le"),("jump","walk","gt"),("jump","idle","le")]:
        conditions = [{"parameter":"speed","op":op,"value":.1}]
        if source == "jump":
            conditions.insert(0, {"parameter":"grounded","op":"eq","value":1})
        transitions.append({"from":source,"to":target,"conditions":conditions})
    mesh["ai_animator"] = json.dumps({"initial":"idle","parameters":{"speed":0,"grounded":1},
        "states":{"idle":{"clip":"idle"},"walk":{"clip":"walk"},"jump":{"clip":"jump","looping":False}},"transitions":transitions})
    mesh.parent = rig
    modifier = mesh.modifiers.new("MiloSkin", "ARMATURE")
    modifier.object = rig
    rig.animation_data_create()
    for name, end in [("idle", 49), ("walk", 25), ("jump", 21)]:
        rig.animation_data.action = bpy.data.actions.new(name)
        rig.animation_data.action.use_fake_user = True
        for frame in range(1, end + 1):
            phase = (frame - 1) / (end - 1)
            for bone in rig.pose.bones:
                bone.rotation_mode = "XYZ"
                bone.rotation_euler = (0, 0, 0)
                bone.location = (0, 0, 0)
            if name == "idle":
                rig.pose.bones["head"].rotation_euler.y = .07 * math.sin(phase * math.tau)
            elif name == "walk":
                for bone, sign in [("leg_l", 1), ("leg_r", -1), ("arm_l", -1), ("arm_r", 1)]:
                    rig.pose.bones[bone].rotation_euler.x = sign * .55 * math.sin(phase * math.tau)
            else:
                lift = math.sin(phase * math.pi)
                rig.pose.bones["arm_l"].rotation_euler.y = -.9 * lift
                rig.pose.bones["arm_r"].rotation_euler.y = .9 * lift
                rig.pose.bones["leg_l"].rotation_euler.x = .35 * lift
                rig.pose.bones["leg_r"].rotation_euler.x = .35 * lift
            for bone in rig.pose.bones:
                bone.keyframe_insert(data_path="rotation_euler", frame=frame)
    rig.animation_data.action = bpy.data.actions["idle"]
    bpy.context.scene.render.fps = 24
    bpy.context.scene.frame_set(1)
    return mesh


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", required=True)
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1:])
    directory = Path(args.out).resolve()
    directory.mkdir(parents=True, exist_ok=True)
    mesh = build()
    bpy.ops.wm.save_as_mainfile(filepath=str(directory / "milo.blend"))
    bridge = Path(__file__).resolve().parents[2] / "blender" / "ai_engine_bridge.py"
    spec = importlib.util.spec_from_file_location("rig_export", bridge)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    result = module._export_meshes({"dir": str(directory)}, [mesh])
    print("ANIMATED_EXPORT " + json.dumps(result), flush=True)
