bl_info = {
    "name": "AI Engine Bridge",
    "author": "AI Engine",
    "version": (0, 1, 0),
    "blender": (4, 2, 0),
    "category": "Import-Export",
    "description": "Trusted-local JSON TCP bridge for AI Engine",
}

import contextlib
import io
import json
import math
import os
from pathlib import Path
import queue
import socketserver
import threading
import time
import traceback
import uuid

import bpy
from mathutils import Matrix

_previous = bpy.app.driver_namespace.get("ai_engine_bridge_unregister")
if _previous:
    _previous()

_server = None
_thread = None
_requests = queue.Queue()
_BASIS = Matrix(((1, 0, 0, 0), (0, 0, 1, 0), (0, -1, 0, 0), (0, 0, 0, 1)))


def _json_value(value):
    if hasattr(value, "to_dict"):
        return {key: _json_value(val) for key, val in value.to_dict().items()}
    if hasattr(value, "to_list"):
        return [_json_value(val) for val in value.to_list()]
    if isinstance(value, dict):
        return {key: _json_value(val) for key, val in value.items()}
    if isinstance(value, (list, tuple)):
        return [_json_value(val) for val in value]
    if value is None or isinstance(value, (str, int, float, bool)):
        return value
    raise TypeError(f"value is not JSON serializable: {type(value).__name__}")


def _ensure_uids():
    seen = set()
    for obj in bpy.context.scene.objects:
        uid = obj.get("ai_uid")
        if not isinstance(uid, str) or not uid or uid in seen:
            uid = uuid.uuid4().hex
            obj["ai_uid"] = uid
        seen.add(uid)


def _props(obj):
    return {key: _json_value(obj[key]) for key in obj.keys() if key.startswith("ai_")}


def _transform(matrix):
    loc, rot, scale = matrix.decompose()
    return {"position": list(loc), "rotation_deg": [math.degrees(v) for v in rot.to_euler("XYZ")], "scale": list(scale)}


def _object_info(obj):
    return {"name": obj.name, "type": obj.type, "ai_uid": obj["ai_uid"],
            "location": list(obj.location), "rotation": list(obj.rotation_euler),
            "scale": list(obj.scale), "world_transform": _transform(obj.matrix_world), "props": _props(obj)}


def _color(obj):
    mat = obj.active_material
    if mat is None:
        return list(obj.color)
    if mat.node_tree is not None:
        for node in mat.node_tree.nodes:
            if node.type == "BSDF_PRINCIPLED":
                return list(node.inputs["Base Color"].default_value)
    return list(mat.diffuse_color)


def _export(request):
    directory = Path(request["dir"]).expanduser().resolve()
    directory.mkdir(parents=True, exist_ok=True)
    wanted = request.get("objects")
    if wanted is not None and (not isinstance(wanted, list) or not all(isinstance(v, str) for v in wanted)):
        raise ValueError("objects must be an array of object names or ai_uid strings")
    meshes = [obj for obj in bpy.context.scene.objects if obj.type == "MESH"]
    all_mesh_uids = [obj["ai_uid"] for obj in meshes]
    if wanted is not None:
        missing = set(wanted) - {value for obj in meshes for value in (obj.name, obj["ai_uid"])}
        if missing:
            raise ValueError(f"unknown mesh objects: {sorted(missing)}; use scene_info")
        meshes = [obj for obj in meshes if obj.name in wanted or obj["ai_uid"] in wanted]
    selected = list(bpy.context.selected_objects)
    active = bpy.context.view_layer.objects.active
    mode = bpy.context.object.mode if bpy.context.object else "OBJECT"
    manifest = []
    duplicates = []
    try:
        if mode != "OBJECT":
            bpy.ops.object.mode_set(mode="OBJECT")
        bpy.ops.object.select_all(action="DESELECT")
        if request.get("whole_scene", False):
            path = directory / f"scene-{uuid.uuid4().hex}.glb"
            bpy.ops.export_scene.gltf(filepath=str(path), export_format="GLB", export_yup=True)
            return {"path": str(path), "objects": [
                {"ai_uid": obj["ai_uid"], "name": obj.name, "path": str(path),
                 "transform": _transform(Matrix.Identity(4)),
                 "world_transform": _transform(_BASIS @ obj.matrix_world @ _BASIS.inverted()),
                 "props": _props(obj), "color": _color(obj)} for obj in meshes],
                "baked_world_transform": True}
        for obj in meshes:
            # Export evaluated local geometry at identity; the manifest supplies
            # C * matrix_world * C^-1 exactly once in the engine's Y-up frame.
            evaluated = obj.evaluated_get(bpy.context.evaluated_depsgraph_get())
            mesh = bpy.data.meshes.new_from_object(evaluated)
            duplicate = bpy.data.objects.new("ai_export", mesh)
            bpy.context.collection.objects.link(duplicate)
            duplicates.append((duplicate, mesh))
            duplicate.matrix_world = Matrix.Identity(4)
            duplicate.select_set(True)
            bpy.context.view_layer.objects.active = duplicate
            converted = _BASIS @ obj.matrix_world @ _BASIS.inverted()
            # Bake residual shear into the mesh: Entity stores TRS only.
            loc, rot, scale = converted.decompose()
            trs = Matrix.LocRotScale(loc, rot, scale)
            residual = _BASIS.inverted() @ (trs.inverted_safe() @ converted) @ _BASIS
            mesh.transform(residual)
            path = directory / f"{obj['ai_uid']}-{uuid.uuid4().hex}.glb"
            bpy.ops.export_scene.gltf(filepath=str(path), export_format="GLB", use_selection=True, export_yup=True)
            duplicate.select_set(False)
            manifest.append({"ai_uid": obj["ai_uid"], "name": obj.name, "path": str(path),
                             "transform": _transform(trs), "props": _props(obj), "color": _color(obj)})
        return {"objects": manifest, "all_mesh_uids": all_mesh_uids}
    finally:
        for duplicate, mesh in duplicates:
            bpy.data.objects.remove(duplicate, do_unlink=True)
            bpy.data.meshes.remove(mesh)
        bpy.ops.object.select_all(action="DESELECT")
        for obj in selected:
            obj.select_set(True)
        bpy.context.view_layer.objects.active = active
        if active and mode != "OBJECT":
            bpy.ops.object.mode_set(mode=mode)


def _dispatch(request):
    command = request.get("command")
    if command == "ping":
        return {"version": bpy.app.version_string, "background": bpy.app.background}
    if command == "exec":
        code = request.get("code")
        if not isinstance(code, str):
            raise ValueError("exec requires string 'code'")
        output = io.StringIO()
        namespace = {"bpy": bpy, "__name__": "__bridge_exec__"}
        with contextlib.redirect_stdout(output):
            exec(compile(code, "<ai-engine-bridge>", "exec"), namespace)
        return {"stdout": output.getvalue(), "result": _json_value(namespace.get("result"))}
    _ensure_uids()
    bpy.context.view_layer.update()
    if command == "scene_info":
        return {"objects": [_object_info(obj) for obj in bpy.context.scene.objects]}
    if command == "export":
        return _export(request)
    raise ValueError(f"unknown command {command!r}; valid commands: ping, exec, scene_info, export")


def _drain():
    for _ in range(16):
        try:
            request, response = _requests.get_nowait()
        except queue.Empty:
            break
        try:
            response.put({"ok": True, "result": _dispatch(request)})
        except Exception:
            response.put({"ok": False, "error": traceback.format_exc()})
    return 0.02 if _server is not None else None


class _Handler(socketserver.StreamRequestHandler):
    def handle(self):
        self.connection.settimeout(125)
        try:
            request = json.loads(self.rfile.readline(8 * 1024 * 1024))
            response = queue.Queue(maxsize=1)
            _requests.put((request, response))
            payload = response.get(timeout=120)
        except Exception as error:
            payload = {"ok": False, "error": str(error)}
        self.wfile.write((json.dumps(payload, allow_nan=False) + "\n").encode())


class _Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


def register():
    global _server, _thread
    if _server is not None:
        return
    previous = bpy.app.driver_namespace.get("ai_engine_bridge_unregister")
    if previous:
        previous()
    port = int(os.environ.get("AI_ENGINE_BLENDER_PORT", "9876"))
    _server = _Server(("127.0.0.1", port), _Handler)
    _thread = threading.Thread(target=_server.serve_forever, daemon=True)
    _thread.start()
    bpy.app.timers.register(_drain, persistent=True)
    bpy.app.driver_namespace["ai_engine_bridge_unregister"] = unregister
    print(f"AI Engine bridge listening on 127.0.0.1:{port}", flush=True)


def unregister():
    global _server, _thread
    if bpy.app.timers.is_registered(_drain):
        bpy.app.timers.unregister(_drain)
    if _server:
        _server.shutdown()
        _server.server_close()
        _server = None
    if _thread:
        _thread.join(timeout=2)
        _thread = None
    bpy.app.driver_namespace.pop("ai_engine_bridge_unregister", None)


if __name__ == "__main__":
    register()
    if bpy.app.background:
        try:
            # Background scripts have no UI event loop to drive app timers.
            while _server is not None:
                _drain()
                time.sleep(0.02)
        except KeyboardInterrupt:
            unregister()
