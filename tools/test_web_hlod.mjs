import test from 'node:test';
import assert from 'node:assert/strict';
import {registerHooks} from 'node:module';

registerHooks({resolve(specifier,context,nextResolve) {
  return nextResolve(specifier==='three' ? new URL('../crates/engine-web/player/vendor/three.module.js',import.meta.url).href : specifier,context);
}});
const THREE=await import('three');
const {HLOD,groupRuns}=await import('../crates/engine-web/player/hlod.js');
const {StreamingWorld}=await import('../crates/engine-web/player/world.js');
const {worldConfig,selectLOD}=await import('../crates/engine-web/player/streaming.js');
const config=(patch={})=>worldConfig([{tags:['player'],props:{ai_world:{chunk_size:32,stream_radius:160,draw_distance:100,lod_distances:[20,60],hysteresis:10,hlod_distance:500,...patch}}}]);
const entity=(id=1,position=[144,2,0],patch={})=>({id,kind:'box',size:[4,4,4],color:[0.2,0.4,0.6],transform:{position,scale:[1,1,1],rotation_deg:[0,0,0]},...patch});
function camera(x=0) {
  const c=new THREE.PerspectiveCamera(80,1,0.1,1000);c.position.set(x,5,0);c.lookAt(144,2,0);c.updateMatrixWorld();return c;
}

test('merged proxies filter static size/visibility and bake transforms, bounds and colors',()=>{
  const scene=new THREE.Scene(),hlod=new HLOD(scene,config());
  hlod.add(entity(1,[144,2,0],{transform:{position:[144,2,0],scale:[2,1,1],rotation_deg:[0,90,0]}}),'4,0');
  hlod.add(entity(2,[176,0,0],{kind:'gltf',props:{web_mesh_bounds:[[1,0,-1],[3,6,1]]}}),'5,0');
  for(const [i,patch] of [{script:'tick.rhai'},{rigidbody:{type:'dynamic'}},{rigidbody:{type:'kinematic'}},{animator:{}},{tags:['player']},{visible:false},{size:[0.1,0.1,0.1]},{kind:'gltf',props:{}}].entries()) hlod.add(entity(i+3,[200,0,0],patch),'6,0');
  hlod.update(camera());
  assert.equal(hlod.entities.size,2);
  const cluster=[...hlod.clusters.values()][0],g=cluster.mesh.geometry;
  assert.equal(g.index.count,72);assert.equal(g.groups.length,1);
  assert.deepEqual(cluster.ranges.map(r=>r.key),['4,0','5,0']);
  const first=new THREE.Box3();for(let i=0;i<36;i++) first.expandByPoint(new THREE.Vector3().fromBufferAttribute(g.attributes.position,i));
  assert.ok(Math.abs(first.min.x-142)<1e-5);assert.ok(Math.abs(first.max.x-146)<1e-5);
  assert.ok(Math.abs(first.min.z+4)<1e-5);assert.ok(Math.abs(first.max.z-4)<1e-5);
  assert.equal(g.boundingBox.max.x,179);assert.equal(g.boundingBox.max.y,6);
  assert.ok(Math.abs(g.attributes.color.getX(0)-0.2)<1e-6);
  assert.ok(Array.isArray(cluster.mesh.material));assert.equal(cluster.mesh.material[0],hlod.material);
  assert.equal(hlod.material.vertexColors,true);assert.equal(cluster.mesh.castShadow,false);
  assert.deepEqual(hlod.stats(),{hlodClusters:1,hlodDrawCalls:1,hlodTriangles:24});
});

test('chunk handoff has one owner, includes whole chunks and respects hysteresis',()=>{
  const hlod=new HLOD(new THREE.Scene(),config());hlod.add(entity(),'4,0');
  for(const [x,far] of [[0,true],[19,true],[28,false],[20,false],[17,true]]) {
    hlod.update(camera(x));
    assert.equal(hlod.isFar('4,0'),far,`eye ${x}`);
    assert.equal(Number(hlod.isFar('4,0'))+Number(!hlod.isFar('4,0')),1);
    assert.equal([...hlod.clusters.values()][0].mesh.geometry.groups.length,Number(far));
  }
  hlod.update(camera(40));assert.equal(hlod.isFar('4,0'),false,'partially near chunks remain normal');
  hlod.update(camera(-400));assert.equal(hlod.isFar('4,0'),false,'outside far distance');
});

test('group runs merge adjacent chunks but never bridge invisible index ranges',()=>{
  const g=new THREE.BufferGeometry(),ranges=['a','b','c'].map((key,i)=>({key,start:i*36,count:36}));
  groupRuns(g,ranges,new Set(['a','b','c']));assert.deepEqual(g.groups,[{start:0,count:108,materialIndex:0}]);
  groupRuns(g,ranges,new Set(['a','c']));assert.equal(g.groups.length,2);
  groupRuns(g,ranges,new Set());assert.deepEqual(g.groups,[]);
});

test('lazy builds have a one-cluster budget, invalidation and distance disposal release geometry',()=>{
  const scene=new THREE.Scene(),hlod=new HLOD(scene,config());
  hlod.add(entity(1),'4,0');hlod.add(entity(2,[272,2,0]),'8,0');
  hlod.update(camera());assert.equal(hlod.builds,1);assert.equal(scene.children.length,1);
  hlod.update(camera());assert.equal(hlod.builds,1);assert.equal(scene.children.length,2);
  const cluster=hlod.clusters.get('1,0'),old=cluster.mesh.geometry;let disposed=0;old.addEventListener('dispose',()=>disposed++);
  hlod.add(entity(1,[148,2,0]),'4,0');hlod.update(camera());
  assert.equal(hlod.builds,1);assert.equal(disposed,1);assert.notEqual(cluster.mesh.geometry,old);
  let removed=0;cluster.mesh.geometry.addEventListener('dispose',()=>removed++);
  hlod.remove(1);assert.equal(removed,1);assert.equal(hlod.clusters.size,1);
  const remaining=[...hlod.clusters.values()][0];let farDisposed=0;remaining.mesh.geometry.addEventListener('dispose',()=>farDisposed++);
  hlod.update(camera(-1000));assert.equal(farDisposed,1);assert.equal(scene.children.length,0);
  hlod.update(camera());assert.equal(scene.children.length,1);
  hlod.reset();assert.equal(scene.children.length,0);assert.equal(hlod.entities.size,0);
});

test('cluster frustum culling and optional occluder affect proxy drawing, not ownership',()=>{
  const hlod=new HLOD(new THREE.Scene(),config());hlod.add(entity(),'4,0');
  let calls=0;hlod.setOcclusion({occluded(key,eye,box){calls++;assert.equal(key,'hlod:1,0');assert.ok(box.max.x>=146);return true;}});
  hlod.update(camera());assert.equal(calls,1);assert.equal(hlod.isFar('4,0'),true);assert.equal(hlod.stats().hlodDrawCalls,0);
  hlod.setOcclusion(null);hlod.update(camera());assert.equal(hlod.stats().hlodDrawCalls,1);
  const back=camera();back.lookAt(-100,0,0);back.updateMatrixWorld();hlod.update(back);assert.equal(hlod.stats().hlodClusters,0);
});

const drawn=group=>group.visible && group.children.some(child=>child.visible);

test('StreamingWorld hides far chunk entities and atomically restores normal geometry',async()=>{
  const scene=new THREE.Scene(),world=new StreamingWorld(scene,{},config());world.add(entity());
  const c=camera();await Promise.all(world.update(c));
  const group=world.groups.get('4,0');assert.equal(world.entities.get(1).level,3);assert.equal(drawn(group),false);
  assert.equal(world.hlod.stats().hlodDrawCalls,1);
  c.position.x=28;c.lookAt(144,2,0);c.updateMatrixWorld();
  const tasks=world.update(c);
  assert.equal(drawn(group),true,'retained near template is ready before asynchronous rebuild');
  assert.equal(world.hlod.isFar('4,0'),false);assert.ok(world.entities.get(1).level<3);assert.equal(world.hlod.stats().hlodDrawCalls,0);
  await Promise.all(tasks);
  world.move(1,{position:[280,2,0],scale:[1,1,1],rotation_deg:[0,0,0]});
  assert.equal(world.hlod.clusters.has('1,0'),false);assert.equal(world.hlod.clusters.has('2,0'),true);
  world.remove(1);assert.equal(world.hlod.clusters.size,0);world.reset();assert.equal(scene.children.length,0);
});

test('pending near templates keep proxy ownership until ready without double drawing',()=>{
  const hlod=new HLOD(new THREE.Scene(),config());hlod.add(entity(),'4,0');hlod.update(camera());
  hlod.update(camera(28),()=>false);assert.equal(hlod.isFar('4,0'),true);assert.equal(hlod.wantsFar('4,0'),false);
  hlod.update(camera(28),()=>true);assert.equal(hlod.isFar('4,0'),false);
});

test('tiny-only chunks stay on the legacy distance cutoff and far limit hides normal fallback',async()=>{
  const scene=new THREE.Scene(),world=new StreamingWorld(scene,{},config({stream_radius:1000}));
  world.add(entity(1,[144,2,0],{size:[0.1,0.1,0.1]}));
  world.add(entity(2,[700,2,0]));
  await Promise.all(world.update(camera()));
  assert.equal(world.entities.get(1).level,3);assert.equal(world.groups.get('4,0').children.length,0);
  assert.equal(world.entities.get(2).level,3);assert.equal(world.hlod.isFar('21,0'),false);
  world.update(camera());assert.equal(drawn(world.groups.get('21,0')),false);
});

test('disabled HLOD preserves legacy grid, level selection, visibility and empty scenes',async()=>{
  const cfg=config({hlod:false}),scene=new THREE.Scene(),world=new StreamingWorld(scene,{},cfg);
  assert.equal(world.hlod,null);assert.equal(world.grid.config.stream_radius,160);
  assert.deepEqual(world.update(camera()),[]);
  world.add(entity());await Promise.all(world.update(camera()));
  assert.equal(world.entities.get(1).level,selectLOD(Math.hypot(144,3)-Math.hypot(4,4,4),undefined,cfg));
  assert.equal(world.groups.get('4,0').children.length,0);assert.equal(scene.children.filter(m=>m.isMesh).length,0);
  const empty=new HLOD(new THREE.Scene(),config());empty.update(camera());assert.deepEqual(empty.stats(),{hlodClusters:0,hlodDrawCalls:0,hlodTriangles:0});
});

test('HLOD configuration defaults and invalid authoring',()=>{
  const cfg=config();assert.equal(cfg.hlod,true);assert.equal(cfg.hlod_cluster,4);assert.equal(cfg.hlod_min_radius,1.5);
  for(const patch of [{hlod:1},{hlod_distance:100},{hlod_distance:Infinity},{hlod_cluster:0},{hlod_cluster:1.5},{hlod_cluster:'4'},{hlod_min_radius:-1},{hlod_min_radius:NaN}]) assert.throws(()=>config(patch),RangeError);
  assert.equal(config({hlod_min_radius:0}).hlod_min_radius,0);
});

test('mixed chunks keep near non-HLOD entities drawn while static neighbours use the proxy',async()=>{
  const scene=new THREE.Scene(),world=new StreamingWorld(scene,{},config());
  world.add(entity(1));world.add(entity(2,[144,2,0],{script:'tick.rhai',size:[100,4,4]}));
  const c=camera();await Promise.all(world.update(c));await Promise.all(world.update(c));
  assert.equal(world.hlod.isFar('4,0'),true);assert.equal(world.entities.get(1).level,3);
  assert.notEqual(world.entities.get(2).level,3);
});

test('hlod:false accepts draw distances beyond the default hlod_distance',()=>{
  const player={tags:['player'],props:{ai_world:{hlod:false,draw_distance:2000,stream_radius:2100}}};
  assert.doesNotThrow(()=>worldConfig([player]));
});

test('HLOD-tracked and untracked entities never share an instance batch',async()=>{
  const scene=new THREE.Scene(),world=new StreamingWorld(scene,{},config());
  world.add(entity(1));world.add(entity(2,[150,2,0],{transform:{position:[150,2,0],scale:[0.01,0.01,0.01],rotation_deg:[0,0,0]}}));
  const c=camera(60);await Promise.all(world.update(c));await Promise.all(world.update(c));
  assert.ok(world.groups.get('4,0').children.length>=2);
  for(const child of world.groups.get('4,0').children) assert.equal(new Set(child.userData.items.map(i=>world.hlod.entities.has(i.e.id))).size,1);
  c.position.x=0;c.updateMatrixWorld();await Promise.all(world.update(c));await Promise.all(world.update(c));
  assert.equal(world.hlod.isFar('4,0'),true);assert.equal(drawn(world.groups.get('4,0')),false);
});
