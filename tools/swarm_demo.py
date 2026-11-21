#!/usr/bin/env python3
"""Reproducible four-role Blender-first demo; all authoring uses MCP HTTP.

stdlib only. Run from any directory after `cargo build`.
"""
import argparse
import base64
import concurrent.futures
import hashlib
import itertools
import json
from pathlib import Path
import socket
import subprocess
import threading
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
RECIPE = ROOT / "tools/sky_garden"
EXAMPLE = ROOT / "examples/sky_garden"


def validations():
    jumps = [event for tick in (12, 76, 140) for event in (
        {"tick": tick, "action": "action", "value": 1},
        {"tick": tick + 1, "action": "action", "value": 0})]
    win = {"ticks": 210, "inputs": [{"tick": 0, "action": "move_x", "value": 1},
        {"tick": 198, "action": "move_x", "value": 0}] + jumps,
        "asserts": [{"state": "score", "equals": 3}, {"state": "won", "equals": True},
                    {"state": "Sprünge", "equals": 3}, {"state": "Rettungen", "equals": 0},
                    {"entities_with_tag": "crystal", "count": 0},
                    {"entity": "Milo", "y_greater_than": 1.0, "at_tick": 40}]}
    fall = {"ticks": 155, "inputs": [{"tick": 0, "action": "move_z", "value": 1},
            {"tick": 65, "action": "move_z", "value": 0}],
            "asserts": [{"state": "Rettungen", "equals": 1},
                        {"entity": "Milo", "position_near": [0, 0.75, 0], "tolerance": 0.2},
                        {"state": "won", "equals": False}]}
    return {"win": win, "fall": fall}


class Swarm:
    def __init__(self, url):
        self.url = url
        self.ids = itertools.count(1)
        self.requests = []
        self.lock = threading.Lock()
        self.stop = threading.Event()
        self.tasks_ready = threading.Event()
        self.tasks = {}
        self.reports = {}

    def call(self, agent, tool, args=None):
        args = args or {}
        payload = {"jsonrpc": "2.0", "id": next(self.ids), "method": "tools/call",
                   "params": {"name": tool, "arguments": args}}
        req = urllib.request.Request(self.url, json.dumps(payload).encode(),
              {"Content-Type": "application/json", "X-Agent-Id": agent})
        with urllib.request.urlopen(req, timeout=240) as response:
            response = json.load(response)
        result = response.get("result", {})
        with self.lock:
            safe = {k: ({"sha256": hashlib.sha256(v.encode()).hexdigest(), "bytes": len(v.encode())}
                        if k in ("code", "source") else v) for k, v in args.items()}
            self.requests.append({"agent": agent, "tool": tool, "arguments": safe,
                                  "ok": "error" not in response and not result.get("isError", False)})
        if "error" in response or result.get("isError"):
            raise RuntimeError(f"{agent}/{tool}: {response}")
        texts = [part["text"] for part in result.get("content", []) if part["type"] == "text"]
        return json.loads(texts[0]) if texts else result

    def wait_task(self, agent, key):
        while not self.tasks_ready.wait(0.1):
            if self.stop.is_set():
                raise RuntimeError("another agent failed before planning")
        task_id = self.tasks[key]
        while not self.stop.is_set():
            tasks = self.call(agent, "task_list")
            task = next(t for t in tasks if t["id"] == task_id)
            completed = {t["id"] for t in tasks if t["status"] == "completed"}
            if all(dep in completed for dep in task["depends_on"]):
                self.call(agent, "task_claim", {"id": task_id})
                self.call(agent, "get_changes_since", {"revision": 0})
                return task_id
            self.stop.wait(0.3)
        raise RuntimeError("another agent failed")

    def complete(self, agent, task_id, note):
        self.call(agent, "task_complete", {"id": task_id, "note": note})

    def artist(self):
        agent = "Blender-Artist"
        task = self.wait_task(agent, "art")
        self.call(agent, "blender_start")
        code = "from pathlib import Path\nPROJECT_ROOT=" + repr(str(ROOT)) + "\n" + (RECIPE / "artist.py").read_text()
        self.call(agent, "blender_exec", {"code": code})
        self.call(agent, "blender_sync", {"prune": True})
        self.complete(agent, task, "Vier modellierte Inseln, Milo, Kristalle, Portal und Wolken synchronisiert.")

    def gameplay(self):
        agent = "Gameplay-Skripter"
        sources = {name: (RECIPE / (name + ".rhai")).read_text() for name in ("player", "crystal")}
        task = self.wait_task(agent, "code")
        scene = self.call(agent, "get_scene_state")
        for tag, name in (("player", "player"), ("crystal", "crystal")):
            ids = [e["id"] for e in scene["entities"] if tag in e["tags"]]
            self.call(agent, "claim_entities", {"ids": ids, "ttl_s": 240})
            try:
                self.call(agent, "write_script", {"ids": ids, "path": f"examples/sky_garden/scripts/{name}.rhai", "source": sources[name]})
            finally:
                self.call(agent, "release_entities", {"ids": ids})
        self.complete(agent, task, "Bewegung, kontaktbasiertes Springen, Kristalle, Portal, Checkpoint-Rettung.")

    def qa(self):
        agent = "QA"
        task = self.wait_task(agent, "qa")
        (EXAMPLE / "validation.json").write_text(json.dumps(validations(), ensure_ascii=False, indent=2) + "\n")
        findings = self.call(agent, "validate_scene")
        self.reports["findings"] = findings
        if any(f["severity"] == "error" for f in findings):
            raise RuntimeError(f"QA scene errors: {findings}")
        for name, args in validations().items():
            report = self.call(agent, "simulate", args)
            self.reports[name] = report
            if not report["passed"]:
                raise RuntimeError(f"QA {name}: {json.dumps(report, ensure_ascii=False)}")
        result = self.call(agent, "get_screenshot", {"views": ["current", "front", "top", "iso"], "overlay_ids": True, "width": 1600, "height": 1000})
        png = next(p["data"] for p in result["content"] if p["type"] == "image")
        (ROOT / "docs/sky_garden_contact.png").write_bytes(base64.b64decode(png))
        self.complete(agent, task, "Szenenprüfung, Sprünge/Sieg und Sturz-Rettung per simulate bestanden.")

    def lead(self, out):
        agent = "Lead"
        art = self.call(agent, "task_create", {"title": "Blender-Welt modellieren"})["id"]
        code = self.call(agent, "task_create", {"title": "Spielregeln implementieren", "depends_on": [art]})["id"]
        qa = self.call(agent, "task_create", {"title": "Spiel prüfen", "depends_on": [code]})["id"]
        delivery = self.call(agent, "task_create", {"title": "Spiel ausliefern", "depends_on": [qa]})["id"]
        self.tasks = {"art": art, "code": code, "qa": qa, "delivery": delivery}
        self.call(agent, "set_camera", {"position": [5.5, 7.7, 15], "target": [5.4, 0, 0], "fov_deg": 52})
        self.tasks_ready.set()
        task = self.wait_task(agent, "delivery")
        self.call(agent, "save_scene", {"path": "examples/sky_garden/scene.json", "bundle_assets": True})
        self.call(agent, "export_web", {"scene": "examples/sky_garden/scene.json", "out_dir": str(out)})
        self.complete(agent, task, "Portables Projekt und selbstständiger Web-Export erzeugt.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/ai-engine")
    parser.add_argument("--out-dir", type=Path, default=ROOT / f"target/swarm-site-{time.time_ns()}/sky_garden")
    args = parser.parse_args()
    if not args.binary.is_file():
        parser.error("ai-engine fehlt; zuerst cargo build ausführen")
    (EXAMPLE / "scripts").mkdir(parents=True, exist_ok=True)
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    swarm = Swarm(f"http://127.0.0.1:{port}/mcp")
    log = ROOT / "target/swarm-demo.log"
    with log.open("w") as output:
        process = subprocess.Popen([str(args.binary.resolve()), "--serve", f"127.0.0.1:{port}"], cwd=ROOT, stdout=output, stderr=output)
        try:
            for attempt in range(100):
                if process.poll() is not None:
                    raise RuntimeError(f"Server stopped; see {log}")
                try:
                    swarm.call("Lead", "get_revision")
                    break
                except urllib.error.URLError:
                    swarm.stop.wait(0.1)
            else:
                raise RuntimeError("HTTP server startup timed out")
            def run(role):
                try:
                    return role()
                except Exception:
                    swarm.stop.set()
                    raise
            with concurrent.futures.ThreadPoolExecutor(max_workers=4, thread_name_prefix="swarm") as pool:
                futures = [pool.submit(run, role) for role in (lambda: swarm.lead(args.out_dir), swarm.artist, swarm.gameplay, swarm.qa)]
                for future in concurrent.futures.as_completed(futures):
                    future.result()
            print("PASS four HTTP agents: Blender → scripts → QA → save_scene/export_web")
            print("WEB_EXPORT=" + str(args.out_dir))
        finally:
            if process.poll() is None:
                try:
                    journal = swarm.call("Lead", "get_changes_since", {"revision": 0})
                    journal["tasks"] = swarm.call("Lead", "task_list")
                    journal["requests"] = swarm.requests
                    (ROOT / "docs/swarm_journal.json").write_text(json.dumps(journal, ensure_ascii=False, indent=2) + "\n")
                    (ROOT / "docs/sky_garden_qa.json").write_text(json.dumps(swarm.reports, ensure_ascii=False, indent=2) + "\n")
                finally:
                    process.terminate()
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()


if __name__ == "__main__":
    main()
