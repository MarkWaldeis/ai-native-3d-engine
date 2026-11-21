"""Four-role MCP recipe replay, not independent model inference. New project only."""
import argparse
import base64
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
RECIPE = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT / 'tools'))
from swarm_demo import Swarm


def port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('project', type=Path)
    parser.add_argument('--work', type=Path, required=True)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/debug/ai-engine')
    args = parser.parse_args()
    project, work, binary = args.project.resolve(), args.work.resolve(), args.binary.resolve()
    work.mkdir(parents=True, exist_ok=False)
    started = time.monotonic()
    subprocess.run([str(binary), 'init', str(project), '--template', 'empty'], check=True)
    (project / 'scripts').mkdir(exist_ok=True)
    for name in ('player', 'guard'):
        (project / 'scripts' / f'{name}.rhai').write_text((RECIPE / f'{name}.rhai').read_text())
    server_port = port()
    swarm = Swarm(f'http://127.0.0.1:{server_port}/mcp')
    reports = {}
    with (work / 'server.log').open('w') as log:
        server = subprocess.Popen([str(binary), '--project', str(project), '--serve', f'127.0.0.1:{server_port}'], env={**os.environ, 'AI_ENGINE_BLENDER_PORT': str(port())}, stdout=log, stderr=log)
        try:
            for _ in range(100):
                try:
                    swarm.call('Lead', 'get_revision')
                    break
                except OSError:
                    if server.poll() is not None:
                        raise RuntimeError('authoring server exited')
                    time.sleep(.1)
            else:
                raise RuntimeError('server readiness timeout')
            for key, title, dependencies in [('art', 'Blender-Wald, Rigs und Collection-LODs', []), ('code', 'Relikte, Patrouillen und Wald-Anker', ['art']), ('nav', 'Regionale Navigation backen', ['code']), ('qa', 'Sieg, Erwischt und Checkpoint prüfen', ['nav'])]:
                swarm.tasks[key] = swarm.call('Lead', 'task_create', {'title': title, 'depends_on': [swarm.tasks[d] for d in dependencies]})['id']
            swarm.call('Lead', 'set_camera', {'position': [0, 10, 14], 'target': [0, 0, 0], 'fov_deg': 55})
            swarm.tasks_ready.set()

            def art():
                task = swarm.wait_task('Artist', 'art')
                swarm.call('Artist', 'blender_start')
                code = f'PROJECT_ROOT={str(project)!r}\nREPO_ROOT={str(ROOT)!r}\n' + (RECIPE / 'artist.py').read_text()
                reports['blender'] = swarm.call('Artist', 'blender_exec', {'code': code})
                reports['sync'] = swarm.call('Artist', 'blender_sync', {'prune': True})
                swarm.complete('Artist', task, 'Echte Blender-Rigs, 700 Collection-Instanzen und sechs LOD-Meshes.')

            def gameplay():
                task = swarm.wait_task('Gameplay', 'code')
                scene = swarm.call('Gameplay', 'get_scene_state')
                for entity in scene['entities']:
                    entity['name'] += ' · ' + str(entity['id'])
                    if 'guard' in entity['tags']:
                        entity['nav_agent'] = {'speed': 1.2, 'offset_y': .65}
                staging = project / 'agents.scene.json'
                staging.write_text(json.dumps(scene, ensure_ascii=False))
                swarm.call('Gameplay', 'load_scene', {'path': staging.name, 'expected_revision': swarm.call('Gameplay', 'get_revision')})
                staging.unlink()
                for tag, filename in [('player', 'player'), ('guard', 'guard')]:
                    ids = [e['id'] for e in scene['entities'] if tag in e['tags']]
                    swarm.call('Gameplay', 'claim_entities', {'ids': ids, 'ttl_s': 600})
                    try:
                        swarm.call('Gameplay', 'write_script', {'ids': ids, 'path': f'scripts/{filename}.rhai', 'source': (RECIPE / f'{filename}.rhai').read_text(), 'expected_revision': swarm.call('Gameplay', 'get_revision')})
                    finally:
                        swarm.call('Gameplay', 'release_entities', {'ids': ids})
                swarm.complete('Gameplay', task, 'Bestehende Komponenten als Szenendaten, keine Engine-Erweiterung.')

            def navigation():
                task = swarm.wait_task('Navigation', 'nav')
                staging = project / 'navigation-input.json'
                staging.write_text(json.dumps(swarm.call('Navigation', 'get_scene_state')))
                baked = subprocess.run([str(binary.with_name('examples') / 'waldwaechter_bake'), str(project)], check=True, capture_output=True, text=True)
                reports['navigation'] = json.loads(baked.stdout)
                swarm.call('Navigation', 'load_scene', {'path': staging.name, 'expected_revision': swarm.call('Navigation', 'get_revision')})
                staging.unlink()
                swarm.complete('Navigation', task, 'Bestehendes konservatives Quad-Navmesh in 128-m-Regionen.')

            def qa():
                task = swarm.wait_task('QA', 'qa')
                reports['lint'] = swarm.call('QA', 'validate_scene')
                assert not [f for f in reports['lint'] if f['severity'] == 'error'], reports['lint']
                reports['cases'] = {}
                for trace in json.loads((RECIPE / 'traces.json').read_text()):
                    spec = {k: trace[k] for k in ('ticks', 'inputs')}
                    spec['asserts'] = [{'state': k, 'equals': v} for k, v in trace['expected'].items()]
                    report = swarm.call('QA', 'simulate', spec)
                    reports['cases'][trace['name']] = report
                    assert report['passed'], json.dumps(report, ensure_ascii=False)
                swarm.complete('QA', task, 'Drei native Akzeptanz-Traces bestanden.')

            def run(role):
                try:
                    role()
                except Exception:
                    swarm.stop.set()
                    raise
            with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
                futures = [pool.submit(run, role) for role in (art, gameplay, navigation, qa)]
                for future in concurrent.futures.as_completed(futures):
                    future.result()
            swarm.call('Lead', 'save_scene', {'path': 'scene.json', 'bundle_assets': True})
            scene = json.loads((project / 'scene.json').read_text())
            def canonical(value):
                if isinstance(value, dict):
                    return {k: canonical(v) for k, v in value.items()}
                if isinstance(value, list):
                    return [canonical(v) for v in value]
                if isinstance(value, str) and value.endswith('.glb'):
                    source = Path(value) if Path(value).is_absolute() else project / value
                    if source.is_file():
                        data = source.read_bytes()
                        name = 'assets/' + hashlib.sha256(data).hexdigest() + '.glb'
                        (project / name).write_bytes(data)
                        return name
                if isinstance(value, str) and value.startswith(('{', '[')):
                    try:
                        return json.dumps(canonical(json.loads(value)), ensure_ascii=False)
                    except ValueError:
                        pass
                return value
            staging = project / 'canonical.scene.json'
            staging.write_text(json.dumps(canonical(scene), ensure_ascii=False))
            swarm.call('Lead', 'load_scene', {'path': staging.name, 'expected_revision': swarm.call('Lead', 'get_revision')})
            staging.unlink()
            swarm.call('Lead', 'save_scene', {'path': 'scene.json'})
            reports['asset_packaging'] = 'Content recipe canonicalizes GLBs by SHA256 after portable bundling; runtime/player unchanged.'
            reports['tasks'] = swarm.call('Lead', 'task_list')
            reports['journal'] = swarm.call('Lead', 'get_changes_since', {'revision': 0})
            reports['requests'] = swarm.requests
            reports['seconds'] = time.monotonic() - started
            reports['method'] = 'Scripted four-role HTTP/MCP replay; no independent model inference.'
            (work / 'report.json').write_text(json.dumps(reports, ensure_ascii=False, indent=2))
            print('PASS Waldwächter swarm:', reports['seconds'], 'seconds; evidence', work)
        finally:
            try:
                swarm.call('Artist', 'blender_exec', {'code': "bpy.app.timers.register(lambda: bpy.ops.wm.quit_blender(), first_interval=0.2)"})
            except Exception:
                pass
            server.terminate()
            server.wait(timeout=10)


if __name__ == '__main__':
    main()
