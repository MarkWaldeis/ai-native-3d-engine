#!/usr/bin/env python3
"""Measure actual HTTP MCP authoring queries on the existing 44 km² generator."""
import argparse
import http.client
import json
from pathlib import Path
import socket
import statistics
import subprocess
import time

from gen_large_world import generate


def benchmark(binary, out, samples):
    root = Path(__file__).resolve().parents[2]
    project = root / "target" / "bench" / ("authoring-" + str(time.time_ns()))
    project.mkdir(parents=True)
    scene = generate(seed=1, density=1.0)
    (project / "scene.json").write_text(json.dumps(scene))
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    log = (project / "server.log").open("w")
    child = subprocess.Popen([str(binary), "--serve", f"127.0.0.1:{port}", "--project", str(project)], stdout=subprocess.DEVNULL, stderr=log)
    try:
        def request(method, params, session=None):
            connection = http.client.HTTPConnection("127.0.0.1", port, timeout=30)
            headers = {"Content-Type": "application/json", "Accept": "application/json"}
            if session:
                headers.update({"Mcp-Session-Id": session, "MCP-Protocol-Version": "2025-06-18"})
            connection.request("POST", "/mcp", json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}), headers)
            response = connection.getresponse()
            data = response.read()
            assert response.status == 200, (response.status, data)
            value = json.loads(data)
            assert "error" not in value and not value.get("result", {}).get("isError"), value
            session = response.getheader("Mcp-Session-Id")
            connection.close()
            return value, len(data), session

        start = time.monotonic()
        while True:
            assert child.poll() is None, (project / "server.log").read_text()
            try:
                _, _, session = request("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "authoring-bench", "version": "1"}})
                break
            except (ConnectionRefusedError, ConnectionResetError):
                assert time.monotonic() - start < 30
                time.sleep(.05)
        cases = {"page_all": ("query_entities", {"limit": 100}), "tag": ("query_entities", {"tag": "ground", "limit": 100}),
                 "collider": ("query_entities", {"component": "collider", "limit": 100}),
                 "bbox": ("query_entities", {"bbox": {"min": [-500, -100, -500], "max": [500, 200, 500]}, "limit": 100}),
                 "radius": ("query_entities", {"near": {"point": [0, 32, 0], "radius": 500}, "limit": 100}),
                 "region": ("query_entities", {"region": {"x": 0, "z": 0}, "limit": 100}),
                 "summary": ("get_scene_summary", {})}
        results = {}
        for name, (tool, arguments) in cases.items():
            elapsed = []
            for i in range(samples + 10):
                start = time.perf_counter()
                value, size, _ = request("tools/call", {"name": tool, "arguments": arguments}, session)
                if i >= 10:
                    elapsed.append((time.perf_counter() - start) * 1000)
            report = json.loads(value["result"]["content"][0]["text"])
            elapsed.sort()
            results[name] = {"p50_ms": round(statistics.median(elapsed), 3), "p95_ms": round(elapsed[int(.95 * (samples - 1))], 3), "max_ms": round(max(elapsed), 3), "response_bytes": size, "matches": report.get("total", report.get("entities"))}
        report = {"entities": len(scene["entities"]), "world_size_m": 6633, "samples": samples, "transport": "HTTP MCP, fresh connection per request", "binary": str(binary), "results": results}
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report, indent=2))
    finally:
        child.terminate()
        child.wait(timeout=10)
        log.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, default=Path("target/debug/ai-engine"))
    parser.add_argument("--out", type=Path, default=Path("target/bench/authoring-latency.json"))
    parser.add_argument("--samples", type=int, default=200)
    args = parser.parse_args()
    assert args.samples >= 20
    benchmark(args.binary.resolve(), args.out.resolve(), args.samples)
