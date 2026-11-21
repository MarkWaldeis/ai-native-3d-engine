"""Generate a deterministic 44 km² streaming demo; run from the project root."""
import argparse
import json
import random
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('output', type=Path)
args = parser.parse_args()
rng = random.Random(44)
entities = []

def add(**entity):
    entity['id'] = len(entities) + 1
    entities.append(entity)

for x in range(-13, 13):
    for z in range(-13, 13):
        add(name='ground', kind='box', size=[256, 0.4, 256],
            transform={'position': [x * 256 + 128, -0.2, z * 256 + 128]},
            color=[0.19, 0.32, 0.2, 1], collider={'shape': 'box'})
for i in range(30000):
    x, z = rng.uniform(-3300, 3300), rng.uniform(-3300, 3300)
    if abs(x) < 4 and abs(z) < 4:
        x += 10
    shape = i % 4
    common = dict(name=f'prop-{i}', transform={'position': [x, 1, z]}, color=[0.35, 0.5, 0.25, 1])
    if shape == 0:
        add(**common, kind='gltf', path='assets/suzanne.glb')
    elif shape == 1:
        add(**common, kind='box', size=[2, 2, 2])
    elif shape == 2:
        add(**common, kind='sphere', radius=1, segments=24)
    else:
        add(**common, kind='cylinder', radius=0.7, height=2, segments=24)
add(name='player', kind='sphere', radius=0.5, segments=24, tags=['player'],
    color=[0.05, 0.5, 1, 1], transform={'position': [0, 0.5, 0]},
    script={'source': 'fn on_tick(ctx,dt) { let p=ctx.position(); p[0]+=ctx.input("move_x")*60.0*dt; p[2]+=ctx.input("move_z")*60.0*dt; ctx.set_position(p); }'},
    props={'ai_world': {'chunk_size': 128, 'stream_radius': 512, 'draw_distance': 480,
                        'lod_distances': [100, 260]},
           'ai_ui': {'title': 'Große Welt', 'mission': '44 km² · WASD / Joystick: 60 m/s'}})
scene = dict(name='44 km² Streaming', camera={'position': [7, 6, 9], 'target': [0, 0.5, 0], 'fov_deg': 55}, entities=entities)
args.output.write_text(json.dumps(scene, separators=(',', ':')))
print(f'{len(entities)} entities → {args.output} ({args.output.stat().st_size} bytes)')
