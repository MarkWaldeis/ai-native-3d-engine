bl_info = {
    "name": "AI Engine Bridge",
    "author": "AI Engine",
    "version": (0, 1, 0),
    "blender": (4, 2, 0),
    "category": "Import-Export",
    "description": "Trusted-local JSON TCP bridge for AI Engine",
}

import contextlib
import hashlib
import struct
import re
from array import array
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
    for obj in list(bpy.data.objects) + list(bpy.data.collections):
        uid = obj.get("ai_uid")
        if not isinstance(uid, str) or not uid or uid in seen:
            uid = uuid.uuid4().hex
            obj["ai_uid"] = uid
        seen.add(uid)


def _props(obj):
    return {key: _json_value(obj[key]) for key in obj.keys() if key.startswith("ai_")}


def _decoded_prop(value):
    if isinstance(value, str):
        try:
            return json.loads(value)
        except ValueError:
            pass
    return value


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


def _canonical_glb(data):
    size, kind = struct.unpack_from("<II", data, 12)
    if kind != 0x4E4F534A:
        raise ValueError("GLB missing JSON chunk")
    document = json.loads(data[20:20 + size])
    for category in ("nodes", "meshes", "materials", "textures", "images", "accessors", "bufferViews", "buffers"):
        for item in document.get(category, []):
            item.pop("name", None)
    encoded = json.dumps(document, sort_keys=True, separators=(",", ":")).encode()
    encoded += b" " * (-len(encoded) % 4)
    chunks = struct.pack("<II", len(encoded), kind) + encoded + data[20 + size:]
    return struct.pack("<III", 0x46546C67, 2, 12 + len(chunks)) + chunks


def _mesh_key(mesh):
    digest = hashlib.sha256()
    formats = {"FLOAT": ("f", "value", 1), "INT": ("i", "value", 1), "BOOLEAN": ("b", "value", 1),
               "FLOAT_VECTOR": ("f", "vector", 3), "FLOAT2": ("f", "vector", 2),
               "FLOAT_COLOR": ("f", "color", 4), "BYTE_COLOR": ("f", "color", 4)}
    for attribute in sorted(mesh.attributes, key=lambda a: a.name):
        if attribute.data_type not in formats:
            return None
        dtype, field, width = formats[attribute.data_type]
        values = array(dtype, [0]) * (len(attribute.data) * width)
        attribute.data.foreach_get(field, values)
        digest.update(f"{attribute.name}:{attribute.domain}:{attribute.data_type}:{len(values)}".encode())
        digest.update(values.tobytes())
    normals = array("f", [0]) * (len(mesh.corner_normals) * 3)
    mesh.corner_normals.foreach_get("vector", normals)
    digest.update(normals.tobytes())
    digest.update(repr([(tuple(p.vertices), p.material_index, p.use_smooth) for p in mesh.polygons]).encode())
    digest.update(repr([m.as_pointer() if m else 0 for m in mesh.materials]).encode())
    return digest.hexdigest()


def _export_meshes(request, supplied=None):
    directory = Path(request["dir"]).expanduser().resolve()
    directory.mkdir(parents=True, exist_ok=True)
    wanted = request.get("objects")
    if wanted is not None and (not isinstance(wanted, list) or not all(isinstance(v, str) for v in wanted)):
        raise ValueError("objects must be an array of object names or ai_uid strings")
    meshes = supplied if supplied is not None else [obj for obj in bpy.context.scene.objects if obj.type == "MESH"]
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
            cache = request.setdefault("_asset_cache", {})
            key = _mesh_key(mesh)
            path = cache.get(key) if key else None
            if path is None:
                temporary = directory / f"export-{uuid.uuid4().hex}.glb"
                try:
                    bpy.ops.export_scene.gltf(filepath=str(temporary), export_format="GLB", use_selection=True, export_yup=True, export_animations=False)
                    data = _canonical_glb(temporary.read_bytes())
                    path = directory / f"{hashlib.sha256(data).hexdigest()}.glb"
                    if not path.exists():
                        temporary.write_bytes(data)
                        os.replace(temporary, path)
                    if key:
                        cache[key] = path
                finally:
                    temporary.unlink(missing_ok=True)
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


def _export(request):
    if request.get("whole_scene"):
        return _export_meshes(request)
    scene_objects = list(bpy.context.scene.objects)
    instances = [o for o in scene_objects if o.instance_type == "COLLECTION" and o.instance_collection]
    wanted = request.get("objects")
    if wanted is not None and (not isinstance(wanted, list) or not all(isinstance(v, str) for v in wanted)):
        raise ValueError("objects must be an array of names or ai_uid strings")
    definitions = {}
    source_objects = set()

    def collection_nodes(collection, prefix="", parent="root", stack=()):
        if collection in stack:
            raise ValueError("cyclic collection instance")
        objects = list(collection.all_objects)
        nodes = []
        for obj in objects:
            source_objects.add(obj)
            if re.search(r"_LOD[12]$", obj.name):
                continue
            key = prefix + obj["ai_uid"]
            local = obj.parent.matrix_world.inverted_safe() @ obj.matrix_world if obj.parent in objects else obj.matrix_world
            nodes.append((key, prefix + obj.parent["ai_uid"] if obj.parent in objects else parent, obj, local))
            if obj.instance_type == "COLLECTION" and obj.instance_collection:
                nested = key + "/collection"
                nodes.append((nested, key, None, Matrix.Translation(-obj.instance_collection.instance_offset)))
                nodes.extend(collection_nodes(obj.instance_collection, nested + "/", nested, stack + (collection,)))
        return nodes

    for instance in instances:
        collection = instance.instance_collection
        name = "blender:" + collection["ai_uid"]
        if name not in definitions:
            definitions[name] = [("root", None, None, Matrix.Translation(-collection.instance_offset))] + collection_nodes(collection)
    instances = [o for o in instances if o not in source_objects]
    ordinary = [o for o in scene_objects if o.type == "MESH" and o not in source_objects and not re.search(r"_LOD[12]$", o.name)]
    candidates = ordinary + instances
    if wanted is not None:
        missing = set(wanted) - {v for o in candidates for v in (o.name, o["ai_uid"])}
        if missing:
            raise ValueError(f"unknown mesh objects or collection instances: {sorted(missing)}")
    selected = lambda o: wanted is None or o.name in wanted or o["ai_uid"] in wanted
    needed = {"blender:" + o.instance_collection["ai_uid"] for o in instances if selected(o)}
    objects = set(o for o in ordinary if selected(o))
    for name in needed:
        objects.update(obj for _, _, obj, _ in definitions[name] if obj is not None and obj.type == "MESH")
    lods = {}
    for obj in list(objects):
        match = re.fullmatch(r"(.*)_LOD0", obj.name)
        if match:
            for level in (1, 2):
                lod = bpy.data.objects.get(f"{match[1]}_LOD{level}")
                if lod:
                    if lod.type != "MESH":
                        raise ValueError("LOD objects must be meshes")
                    if any(abs(obj.matrix_world[i][j] - lod.matrix_world[i][j]) > 1e-5 for i in range(4) for j in range(4)):
                        raise ValueError("LOD transforms must match LOD0; apply/bake transforms first")
                    objects.add(lod)
                    lods.setdefault(obj, {})[str(level)] = lod
    exported = _export_meshes({"dir": request["dir"]}, sorted(objects, key=lambda o: o.name))["objects"]
    by_uid = {item["ai_uid"]: item for item in exported}
    for obj, levels in lods.items():
        by_uid[obj["ai_uid"]]["props"]["web_lods"] = {level: by_uid[lod["ai_uid"]]["path"] for level, lod in levels.items()}
    prefabs = []
    for name in sorted(needed):
        nodes = []
        for key, parent, obj, local in definitions[name]:
            transform = _transform(_BASIS @ local @ _BASIS.inverted())
            entity = {"id": 0, "name": obj.name if obj else name, "kind": "box", "size": [.001] * 3, "visible": False, "transform": transform, "props": _props(obj) if obj else {}}
            if obj is not None and obj.type == "MESH":
                item = by_uid[obj["ai_uid"]]
                entity.update(kind="gltf", path=item["path"], visible=True, color=item["color"], props=item["props"])
                entity.pop("size")
            nodes.append({"key": key, "parent": parent, "entity": entity})
        prefabs.append({"name": name, "nodes": nodes})
    all_uids = [o["ai_uid"] for o in ordinary]
    for instance in instances:
        all_uids.extend(instance["ai_uid"] if key == "root" else instance["ai_uid"] + ":" + key for key, _, _, _ in definitions["blender:" + instance.instance_collection["ai_uid"]])
    return {"objects": [by_uid[o["ai_uid"]] for o in ordinary if selected(o)], "prefabs": prefabs,
            "instances": [{"ai_uid": o["ai_uid"], "name": o.name, "prefab": "blender:" + o.instance_collection["ai_uid"], "transform": _transform(_BASIS @ o.matrix_world @ _BASIS.inverted()), "props": _props(o)} for o in instances if selected(o)],
            "all_mesh_uids": all_uids}


def _dispatch(request):
    command = request.get("command")
    project_id = os.environ.get("AI_ENGINE_PROJECT_ID", "")
    if command == "ping":
        return {"version": bpy.app.version_string, "background": bpy.app.background, "bridge_protocol": 1, "project_id": project_id}
    if project_id and request.get("project_id") != project_id:
        raise ValueError("Blender project identity mismatch; command refused")
    if command == "exec":
        code = request.get("code")
        if not isinstance(code, str):
            raise ValueError("exec requires string 'code'")
        output = io.StringIO()
        namespace = {"bpy": bpy, "__name__": "__bridge_exec__"}
        with contextlib.redirect_stdout(output):
            exec(compile(code, "<ai-engine-bridge>", "exec"), namespace)
        return {"stdout": output.getvalue(), "result": _json_value(namespace.get("result"))}
    if command != "push_props":
        _ensure_uids()
    bpy.context.view_layer.update()
    if command == "push_props":
        staged = []
        markers = []
        seen = set()
        for item in request["objects"]:
            uid = item["ai_uid"]
            matches = [obj for obj in bpy.context.scene.objects if obj.get("ai_uid") == uid]
            if len(matches) != 1 or uid in seen:
                raise ValueError(f"unknown or duplicate ai_uid {uid}")
            seen.add(uid)
            obj = matches[0]
            current = _props(obj)
            deleted = current.get("ai_deleted_props", [])
            deleted = json.loads(deleted) if isinstance(deleted, str) else deleted
            if not isinstance(deleted, list) or not all(isinstance(key, str) for key in deleted):
                raise ValueError("ai_deleted_props must be a JSON array of property names")
            for key, value in item["props"].items():
                if not key.startswith("ai_") or key in ("ai_uid", "ai_deleted_props"):
                    raise ValueError(f"invalid gameplay property {key}")
                encoded = json.dumps(value) if isinstance(value, (dict, list)) or value is None else value
                actual = None if key in deleted else current.get(key)
                if ((key in current or key in deleted)
                        and _decoded_prop(actual) != _decoded_prop(item["expected"].get(key))
                        and _decoded_prop(actual) != _decoded_prop(value)):
                    raise ValueError(f"Blender gameplay conflict on {uid} {key}")
                staged.append((obj, key, encoded))
            markers.append((obj, [key for key in deleted if key not in item["props"]]))
        for obj, key, value in staged:
            obj[key] = value
        for obj, deleted in markers:
            if deleted:
                obj["ai_deleted_props"] = json.dumps(deleted)
            elif "ai_deleted_props" in obj:
                del obj["ai_deleted_props"]
        return {"ok": True, "uids": sorted(seen)}
    if command == "scene_info":
        return {"objects": [_object_info(obj) for obj in bpy.context.scene.objects]}
    if command == "export":
        return _export(request)
    raise ValueError(f"unknown command {command!r}; valid commands: ping, exec, scene_info, export, push_props")


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
