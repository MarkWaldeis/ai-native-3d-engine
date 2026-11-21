"""Stdlib MCP client; concurrent M10 observation and failure workers."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import time
import urllib.request

WORK = Path(os.environ.get("M10_WORK", str(Path.home() / "m10-work")))
ROOT = Path(__file__).resolve().parents[2]


class Client:
    def __init__(self, agent):
        self.agent = agent
        self.run = json.loads((WORK / "run.json").read_text())
        cache = WORK / (agent + ".session.json")
        self.headers = {"Content-Type": "application/json", "Accept": "application/json, text/event-stream", "X-Agent-Id": agent}
        if cache.exists():
            self.headers.update(json.loads(cache.read_text()))
        else:
            payload = {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "m10-" + agent, "version": "1"}}}
            with urllib.request.urlopen(urllib.request.Request(self.run["url"], json.dumps(payload).encode(), self.headers), timeout=240) as response:
                json.load(response)
                session = {"Mcp-Session-Id": response.headers["Mcp-Session-Id"], "MCP-Protocol-Version": "2025-06-18"}
            self.headers.update(session)
            self.request({"jsonrpc": "2.0", "method": "notifications/initialized"})
            cache.write_text(json.dumps(session))

    def request(self, payload):
        with urllib.request.urlopen(urllib.request.Request(self.run["url"], json.dumps(payload).encode(), self.headers), timeout=240) as response:
            return json.load(response) if response.status != 202 else None

    def call(self, tool, args=None, expect_error=False):
        args = args or {}
        start = time.monotonic()
        epoch = time.time()
        response = self.request({"jsonrpc": "2.0", "id": time.time_ns(), "method": "tools/call", "params": {"name": tool, "arguments": args}})
        result = response.get("result", {})
        failed = "error" in response or result.get("isError", False)
        safe = {k: {"sha256": hashlib.sha256(v.encode()).hexdigest(), "bytes": len(v.encode())} if k in ("source", "code") else v for k, v in args.items()}
        entry = {"agent": self.agent, "tool": tool, "arguments": safe, "start_s": epoch - self.run["start_epoch"], "duration_s": time.monotonic() - start, "ok": not failed, "expected_error": expect_error}
        fd = os.open(WORK / "requests.ndjson", os.O_APPEND | os.O_CREAT | os.O_WRONLY, 0o600)
        try:
            os.write(fd, (json.dumps(entry, ensure_ascii=False) + "\n").encode())
        finally:
            os.close(fd)
        if failed:
            if expect_error:
                return response
            raise RuntimeError(f"{self.agent}/{tool}: {response}")
        if expect_error:
            raise AssertionError(f"{tool}: expected rejection but succeeded")
        texts = [item["text"] for item in result.get("content", []) if item["type"] == "text"]
        return json.loads(texts[0]) if texts else result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["call", "qa-loop", "die"])
    parser.add_argument("agent")
    parser.add_argument("tool", nargs="?")
    parser.add_argument("arguments", nargs="?", default="{}")
    parser.add_argument("--expect-error", action="store_true")
    args = parser.parse_args()
    client = Client(args.agent)
    if args.mode == "call":
        payload = json.loads(args.arguments)
        for key in ("code", "source"):
            if key + "_file" in payload:
                payload[key] = payload.pop(key + "_prefix", "") + Path(payload.pop(key + "_file")).read_text()
        result = client.call(args.tool, payload, args.expect_error)
        if args.tool == "get_screenshot":
            for item in result.get("content", []):
                if item["type"] == "image":
                    (WORK / "native-qa.png").write_bytes(base64.b64decode(item["data"]))
            print("SCREENSHOT=" + str(WORK / "native-qa.png"))
        else:
            print(json.dumps(result, ensure_ascii=False))
    elif args.mode == "die":
        task, entity = map(int, (args.tool, args.arguments))
        claim = client.call("task_claim", {"id": task, "ttl_s": 2})
        client.call("claim_entities", {"ids": [entity], "ttl_s": 2})
        client.call("set_transform", {"id": entity, "position": [4.8, 0.8, -3.3]})
        marker = {"task": task, "entity": entity, "lease": claim["lease"], "death_epoch": time.time(), "agent": args.agent}
        (WORK / "death.json").write_text(json.dumps(marker))
        print("INJECTED WORKER EXIT 23: " + json.dumps(marker), flush=True)
        os._exit(23)
    else:
        revision = 0
        while not (WORK / "stop-qa").exists():
            changes = client.call("get_changes_since", {"revision": revision})
            scene = client.call("get_scene_state")
            findings = client.call("validate_scene")
            simulation = None
            if any(e.get("script") for e in scene["entities"]):
                simulation = client.call("simulate", {"ticks": 8, "asserts": []})
            row = {"at_s": time.time() - client.run["start_epoch"], "revision": changes.get("current_revision"), "entities": len(scene["entities"]), "findings": findings, "probe_passed": simulation.get("passed") if simulation else None}
            with (WORK / "qa-observations.ndjson").open("a") as output:
                output.write(json.dumps(row, ensure_ascii=False) + "\n")
            revision = changes.get("current_revision", revision)
            time.sleep(1)


if __name__ == "__main__":
    main()
