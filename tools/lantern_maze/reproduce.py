"""Replay the published M10 recipe in a NEW project; no AI inference is claimed."""
import argparse
import base64
import datetime
import json
import os
from pathlib import Path
import socket
import signal
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("project", type=Path)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/ai-engine")
    parser.add_argument("--work", type=Path, default=Path.home() / ("m10-replay-" + str(time.time_ns())))
    parser.add_argument("--skip-native-shot", action="store_true")
    args = parser.parse_args()
    project, work, binary = args.project.resolve(), args.work.resolve(), args.binary.resolve()
    work.mkdir(parents=True, exist_ok=False)
    start = time.time()
    subprocess.run([str(binary), "init", str(project), "--template", "empty"], check=True)
    port = free_port()
    run = {"started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(), "start_epoch": start,
           "project": str(project), "url": f"http://127.0.0.1:{port}/mcp", "model_topology": "scripted recipe replay; no model inference"}
    (work / "run.json").write_text(json.dumps(run, indent=2))
    os.environ["M10_WORK"] = str(work)
    from mcp import Client
    from validation import validations
    env = {**os.environ, "AI_ENGINE_BLENDER_PORT": str(free_port())}
    observer = None
    with (work / "server.log").open("w") as output:
        server = subprocess.Popen([str(binary), "--project", str(project), "--serve", f"127.0.0.1:{port}"], env=env, stdout=output, stderr=output, start_new_session=os.name == "posix")
        try:
            for _ in range(100):
                if server.poll() is not None:
                    raise RuntimeError("server exited; inspect server.log")
                try:
                    lead = Client("Lead")
                    lead.call("get_revision")
                    break
                except OSError:
                    time.sleep(.1)
            else:
                raise RuntimeError("HTTP readiness timeout")
            tasks = {}
            for key, title, deps in [("scaffold", "Blender-Grundlage", []), ("art", "Labyrinth modellieren", ["scaffold"]),
                    ("code", "Rhai-Spielregeln", ["scaffold"]), ("qa", "Akzeptanz prüfen", ["art", "code"]),
                    ("finish", "Tor fertig ausrichten", ["art"]), ("observer", "Parallele QA-Beobachtung", [])]:
                tasks[key] = lead.call("task_create", {"title": title, "depends_on": [tasks[dep] for dep in deps]})["id"]
            artist, code, qa = (Client(role) for role in ("Artist", "Gameplay", "QA"))
            watcher = Client("QA-Watcher")
            watch_lease = watcher.call("task_claim", {"id": tasks["observer"], "ttl_s": 3600})["lease"]
            with (work / "qa-worker.log").open("w") as log:
                observer = subprocess.Popen([sys.executable, str(ROOT / "tools/lantern_maze/mcp.py"), "qa-loop", "QA-Watcher"], env=os.environ.copy(), stdout=log, stderr=log)
            def claim(client, key):
                return client.call("task_claim", {"id": tasks[key], "ttl_s": 3600})["lease"]
            def complete(client, key, lease):
                client.call("task_complete", {"id": tasks[key], "lease": lease})
            recipe = (ROOT / "tools/lantern_maze/artist.py").read_text()
            lease = claim(artist, "scaffold")
            artist.call("blender_start")
            artist.call("blender_exec", {"code": f"PROJECT_ROOT={str(project)!r}\nSTAGE='scaffold'\n" + recipe})
            artist.call("blender_sync")
            scene = lead.call("get_scene_state")
            player = next(e["id"] for e in scene["entities"] if "player" in e["tags"])
            complete(artist, "scaffold", lease)
            code_lease = claim(code, "code")
            code.call("claim_entities", {"ids": [player], "ttl_s": 600})
            source = (ROOT / "examples/lantern_maze/scripts/player.rhai").read_text()
            code.call("write_script", {"ids": [player], "path": "scripts/player.rhai", "source": source, "expected_revision": code.call("get_revision")})
            stale = code.call("get_revision")
            art_lease = claim(artist, "art")
            artist.call("blender_exec", {"code": f"PROJECT_ROOT={str(project)!r}\nSTAGE='maze'\n" + recipe})
            info = artist.call("blender_scene_info")
            artist.call("blender_sync", {"objects": [e["name"] for e in info["objects"] if e["name"] not in ("Lumi", "Fundament")]})
            artist.call("set_transform", {"id": player, "position": [0, 1, 0]}, expect_error=True)
            code.call("set_color", {"id": player, "color": [1, 0, 0, 1], "expected_revision": stale}, expect_error=True)
            before, revision = lead.call("get_scene_state"), lead.call("get_revision")
            camera = {"tool": "set_camera", "arguments": {"position": [0, 12, 12], "target": [0, 0, 0], "fov_deg": 52}}
            lead.call("apply_batch", {"mode": "atomic", "expected_revision": revision, "commands": [camera, {"tool": "remove_entity", "arguments": {"id": 999999}}]}, expect_error=True)
            assert lead.call("get_scene_state") == before and lead.call("get_revision") == revision
            lead.call("apply_batch", {"mode": "atomic", "expected_revision": revision, "commands": [camera]})
            code.call("release_entities", {"ids": [player]})
            complete(code, "code", code_lease)
            complete(artist, "art", art_lease)
            qa_lease = claim(qa, "qa")
            bar = next(e["id"] for e in lead.call("get_scene_state")["entities"] if "gate_bar" in e["tags"])
            death = subprocess.run([sys.executable, str(ROOT / "tools/lantern_maze/mcp.py"), "die", "Ausfall-Worker", str(tasks["finish"]), str(bar)], env=os.environ.copy(), check=False)
            assert death.returncode == 23
            marker = json.loads((work / "death.json").read_text())
            takeover = Client("QA-Uebernahme")
            while next(t for t in takeover.call("task_list") if t["id"] == tasks["finish"])["status"] != "expired":
                time.sleep(.05)
            new_lease = claim(takeover, "finish")
            takeover.call("claim_entities", {"ids": [bar], "ttl_s": 120})
            takeover.call("apply_batch", {"mode": "atomic", "expected_revision": takeover.call("get_revision"), "commands": [{"tool": "set_transform", "arguments": {"id": bar, "position": [4.8, .8, -3.5]}}]})
            Client("Ausfall-Worker").call("task_complete", {"id": tasks["finish"], "lease": marker["lease"]}, expect_error=True)
            takeover.call("release_entities", {"ids": [bar]})
            complete(takeover, "finish", new_lease)
            recovery = time.time() - marker["death_epoch"]
            stable = lead.call("get_revision")
            code.call("claim_entities", {"ids": [player], "ttl_s": 120})
            code.call("write_script", {"ids": [player], "path": "scripts/recovery_probe.rhai", "source": 'fn on_start(ctx){ctx.state_set("score",99);ctx.state_set("won",true);} '})
            probe = {"ticks": 1, "asserts": [{"state": "score", "equals": 0}, {"state": "won", "equals": False}]}
            assert not qa.call("simulate", probe)["passed"]
            code.call("rollback", {"revision": stable})
            assert qa.call("simulate", probe)["passed"]
            code.call("release_entities", {"ids": [player]})
            specs = validations()
            (project / "validation.json").write_text(json.dumps(specs, ensure_ascii=False, indent=2) + "\n")
            assert not [f for f in qa.call("validate_scene") if f["severity"] == "error"]
            for name, spec in specs.items():
                report = qa.call("simulate", spec)
                (work / (name + ".json")).write_text(json.dumps(report, ensure_ascii=False, indent=2))
                assert report["passed"], name
            artist.call("claim_entities", {"ids": [player], "ttl_s": 120})
            artist.call("blender_push_props", {"ids": [player]})
            artist.call("blender_sync", {"objects": ["Lumi"]})
            artist.call("release_entities", {"ids": [player]})
            artist.call("blender_exec", {"code": f"import bpy\nbpy.ops.wm.save_as_mainfile(filepath={str(project / 'lantern_maze.blend')!r})"})
            if not args.skip_native_shot:
                shot = qa.call("get_screenshot", {"views": ["current", "top", "iso"], "width": 1200, "height": 800})
                for item in shot.get("content", []):
                    if item["type"] == "image":
                        (work / "native-qa.png").write_bytes(base64.b64decode(item["data"]))
            lead.call("save_scene", {"path": "scene.json", "bundle_assets": True})
            qa.call("export_web", {"scene": "scene.json", "out_dir": "web"})
            complete(qa, "qa", qa_lease)
            (work / "stop-qa").touch()
            observer.wait(timeout=10)
            watcher.call("task_complete", {"id": tasks["observer"], "lease": watch_lease})
            evidence = {"wall_s": time.time() - start, "recovery_s": recovery, "scene_journal": lead.call("get_changes_since", {"revision": 0}),
                "task_journal": lead.call("get_task_changes_since", {"sequence": 0}), "tasks": lead.call("task_list")}
            (work / "evidence.json").write_text(json.dumps(evidence, ensure_ascii=False, indent=2))
            print(f"PASS recipe replay: {len(specs)} native QA cases; actual worker exit23, lease takeover, late worker rejected, rollback, atomic rejection; export={project / 'web'}")
            print(f"Measured recipe replay (not AI design time): wall={evidence['wall_s']:.2f}s; recovery={recovery:.2f}s; evidence={work}")
        finally:
            (work / "stop-qa").touch()
            if observer and observer.poll() is None:
                observer.terminate()
                observer.wait(timeout=10)
            if os.name == "posix":
                os.killpg(server.pid, signal.SIGTERM)
            else:
                server.terminate()
            server.wait(timeout=10)


if __name__ == "__main__":
    main()
