import test from 'node:test';
import assert from 'node:assert/strict';
import {ChunkGrid,selectLOD,batchKey,canInstance,entityRadius,isStatic,worldConfig} from '../crates/engine-web/player/streaming.js';
const config = {chunk_size:128,stream_radius:128,draw_distance:480,lod_distances:[100,260],hysteresis:16};
test('chunk load/unload hysteresis, movement, removal and empty grid', () => {
  assert.equal(new ChunkGrid().config.chunk_size,128);
  const grid = new ChunkGrid(config);
  assert.deepEqual(grid.update([0,0,0]),{load:[],unload:[]});
  grid.register(1,[10,0,10],2);
  assert.deepEqual(grid.update([0,0,0]).load,['0,0']);
  assert.deepEqual(grid.update([265,0,10]).unload,[]);
  assert.deepEqual(grid.update([280,0,10]).unload,['0,0']);
  grid.move(1,[300,0,10]); assert.equal(grid.items.get(1).key,'2,0');
  assert.deepEqual(grid.update([300,0,10]).load,['2,0']);
  grid.remove(1); grid.remove(9); assert.deepEqual(grid.update([300,0,10]).unload,['2,0']);
});
test('bounding radii keep large props loaded across chunk boundaries', () => {
  const grid = new ChunkGrid(config);
  grid.register(1,[10,0,10],200);
  assert.deepEqual(grid.update([400,0,10]).load,['0,0']);
  assert.deepEqual(grid.update([490,0,10]).unload,['0,0']);
});
test('GLB radius covers offset bounds from the scaled entity origin', () => {
  const bounds = [[300,-2,-4],[320,2,4]], scale = [-2,3,-4];
  const radius = entityRadius({kind:'gltf',transform:{scale},props:{web_mesh_bounds:bounds}});
  assert.equal(radius,Math.hypot(640,6,16));
  for (const x of [300,320]) for (const y of [-2,2]) for (const z of [-4,4]) assert.ok(Math.hypot(x*scale[0],y*scale[1],z*scale[2]) <= radius);
  const grid = new ChunkGrid(config);
  grid.register(1,[0,0,0],radius);
  assert.deepEqual(grid.update([700,0,0]).load,['0,0']);
  assert.deepEqual(grid.update([1000,0,0]).unload,['0,0']);
  assert.equal(entityRadius({kind:'box',size:[1,2,3]}),Math.hypot(1,2,3));
  assert.equal(entityRadius({kind:'sphere',radius:2,transform:{scale:[-3,-2,-1]}}),6);
});
test('LOD transitions retain previous level within hysteresis', () => {
  assert.deepEqual([0,150,300,600].map(d => selectLOD(d,undefined,config)),[0,1,2,3]);
  assert.equal(selectLOD(110,0,config),0); assert.equal(selectLOD(117,0,config),1);
  assert.equal(selectLOD(90,1,config),1); assert.equal(selectLOD(80,1,config),0);
  assert.equal(selectLOD(490,2,config),2); assert.equal(selectLOD(500,2,config),3);
});
test('batch keys include asset or primitive dimensions/material/segments', () => {
  const a = {kind:'sphere',radius:1,color:[1,0,0],segments:24};
  assert.equal(batchKey(a),batchKey({...a,id:55}));
  for (const patch of [{radius:2},{segments:8},{color:[0,1,0]}]) assert.notEqual(batchKey(a),batchKey({...a,...patch}));
  assert.notEqual(batchKey({kind:'gltf',path:'a'}),batchKey({kind:'gltf',path:'b'}));
  assert.notEqual(batchKey({kind:'gltf',path:'a',props:{web_lods:{1:'low-a'}}}),batchKey({kind:'gltf',path:'a',props:{web_lods:{1:'low-b'}}}));
  assert.ok(isStatic(a)); assert.ok(!isStatic({...a,script:'x'})); assert.ok(!isStatic({...a,tags:['coin']}));
});
test('mirrored instances use individual objects while even reflections can batch', () => {
  for (const kind of ['box','plane','sphere','cylinder','gltf']) {
    const entity = {kind};
    assert.ok(canInstance(entity));
    for (const scale of [[-1,1,1],[1,-1,1],[1,1,-1],[-1,-1,-1]]) assert.equal(canInstance({...entity,transform:{scale}}),false);
    for (const scale of [[2,3,4],[-2,-3,4],[-2,3,-4],[2,-3,-4]]) assert.ok(canInstance({...entity,transform:{scale}}));
    assert.equal(canInstance({...entity,script:'script.rhai'}),false);
  }
  assert.equal(canInstance({kind:'wall_with_door'}),false);
  assert.equal(canInstance({kind:'box',transform:{scale:[0,1,1]}}),false);
});
test('config opt-in, auto enable, invalid config and small scenes', () => {
  assert.equal(worldConfig([]),null);
  assert.equal(worldConfig([{transform:{position:[2000,0,0]}}]).chunk_size,128);
  assert.equal(worldConfig([{tags:['player'],props:{ai_world:'{"chunk_size":64}'}}]).chunk_size,64);
  assert.equal(worldConfig([{tags:['player'],props:{ai_world:{}}}]).sim_radius,256);
  assert.equal(worldConfig([{tags:['player'],props:{ai_world:{camera_distance:24}}}]).camera_distance,24);
  assert.equal(worldConfig([{tags:['player'],props:{ai_world:{}}}]).camera_distance,undefined);
  for (const camera_distance of [0,-1,Infinity,'close']) assert.throws(() => worldConfig([{tags:['player'],props:{ai_world:{camera_distance}}}]),RangeError);
  assert.throws(() => worldConfig([{tags:['player'],props:{ai_world:{chunk_size:0}}}]),RangeError);
  assert.throws(() => worldConfig([{tags:['player'],props:{ai_world:'bad'}}]),SyntaxError);
});
