import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from '../crates/engine-web/player/vendor/three.module.js';
import {createTerrain, decodeHeights, sampleTile, terrainLOD, terrainReach, drainTerrainWork, TileQueue} from '../crates/engine-web/player/terrain.js';

function heightfield(magic,n,value) {
  const bytes=new Uint8Array(12+n*n*2), view=new DataView(bytes.buffer);
  bytes.set(new TextEncoder().encode(magic)); view.setUint32(4,n,true); view.setUint32(8,n,true);
  for(let i=0;i<n*n;i++) view.setUint16(12+i*2,Array.isArray(value)?value[i]:value,true);
  return bytes;
}

async function terrainFixture(t,{size=[3,3],tile=2,samples=3,entities,game={},failTile=false,coarseValues=16384}={}) {
  const originals=['window','location'].map(key=>[key,Object.getOwnPropertyDescriptor(globalThis,key)]);
  t.after(()=>{for(const [key,descriptor] of originals) {if(descriptor) Object.defineProperty(globalThis,key,descriptor);else delete globalThis[key];}});
  globalThis.window={};globalThis.location={href:'http://localhost/demo/'};
  const grid=size.map(v=>Math.ceil(v/tile));
  const manifest={config:{size_m:size,tile_m:tile,samples},grid,height_range:[0,100],coarse:{dims:size.map(v=>Math.ceil(v/tile)+1),step:tile,file:'coarse.bin'},tiles:'tiles/{x}_{z}.bin'};
  const requests=[],registered=[],inserted=[],errors=[];
  t.mock.method(globalThis,'fetch',async url=>{
    requests.push(url);
    if(url.includes('/tiles/') && failTile) return new Response('',{status:404});
    return new Response(url.endsWith('.json')?JSON.stringify(manifest):url.endsWith('coarse.bin')?heightfield('AITC',manifest.coarse.dims[0],coarseValues):heightfield('AITT',samples+2,49151));
  });
  const world=new THREE.Scene();world.fog=new THREE.Fog(0,1,100);
  const wasm={set_terrain(...args){registered.push(args);},add_terrain_tile(...args){inserted.push(args);},terrain_tiles_wanted(){return '[]';},...game};
  const api=await createTerrain(THREE,entities||[{props:{ai_terrain:'terrain.json'}}],wasm,world,{},e=>errors.push(e));
  return {api,world,wasm,requests,registered,inserted,errors};
}

test('coarse and full-resolution meshes cover padded edge tiles like Rapier', async t=>{
  const {api,world,errors}=await terrainFixture(t);
  const camera=new THREE.PerspectiveCamera(90,1,0.1,2000);camera.position.set(2,150,2);camera.lookAt(2,0,2);
  assert.equal(world.children.length,0);
  for(let i=0;i<4;i++)api.update(camera,i);
  const assertBounds=()=>{
    assert.equal(world.children.length,4);
    for(const mesh of world.children) {
      mesh.geometry.computeBoundingBox();
      assert.equal(mesh.geometry.boundingBox.max.x,2);
      assert.equal(mesh.geometry.boundingBox.max.z,2);
    }
    assert.equal(Math.max(...world.children.map(m=>m.position.x+m.geometry.boundingBox.max.x)),4);
  };
  assertBounds();
  await new Promise(resolve=>setImmediate(resolve));
  for(let i=0;i<4;i++){api.update(camera,500+i);for(const mesh of world.children)if(mesh.visible)mesh.onBeforeRender();}
  const fine=world.children.filter(m=>m.visible);
  assert.equal(fine.length,4);
  for(const mesh of fine) {
    const p=mesh.geometry.attributes.position;
    assert.equal(Math.max(...Array.from({length:p.count},(_,i)=>p.getX(i))),2);
    assert.equal(Math.max(...Array.from({length:p.count},(_,i)=>p.getZ(i))),2);
    assert.ok(Math.abs(p.getY(8)-75)<.001);
  }
  assert.deepEqual(errors,[]);
});

test('scenes without terrain need no renderer, WASM calls or browser globals', async () => {
  assert.equal(await createTerrain(null,[],null,null,null,null),null);
  assert.equal(await createTerrain(null,[{props:{}}],null,null,null,null),null);
});

test('occlusion hides coarse and full tile meshes and restores visibility next frame',async t=>{
  const {api,world}=await terrainFixture(t);
  const camera=new THREE.PerspectiveCamera(90,1,0.1,6000);
  camera.position.set(1,100,1);camera.lookAt(1,0,1);camera.updateMatrixWorld();
  let hidden=true,tests=0;
  api.setOcclusion({occluded(key,eye,box){tests++;assert.ok(box.max.y>0);return hidden;}});
  api.update(camera,0);
  assert.ok(tests>0);assert.ok(api.stats().occludedTerrainTiles>0);
  assert.equal(world.children.filter(m=>m.visible).length,0);
  await new Promise(resolve=>setImmediate(resolve));
  api.update(camera,500);
  assert.equal(world.children.filter(m=>m.visible).length,0);
  hidden=false;api.update(camera,501);
  assert.equal(api.stats().occludedTerrainTiles,0);
  assert.ok(world.children.some(m=>m.visible));
});

test('terrain micro-relief is off by default, bounded and shares a shader across enabled strengths', async t => {
  const strengths = [undefined, -1, '0.5', 0.5, 2];
  const {api,world} = await terrainFixture(t, {size:[2,2],entities:strengths.map(ai_terrain_detail => ({
    props:{ai_terrain:'terrain.json',ai_terrain_detail},
  }))});
  for(let i=0;i<5;i++)api.update(new THREE.PerspectiveCamera(),i);
  const keys = world.children.map(({material}) => material.customProgramCacheKey());
  assert.equal(new Set(keys.slice(0,3)).size,1);
  assert.equal(keys[3],keys[4]);
  assert.notEqual(keys[0],keys[3]);
  for (const [index,mesh] of world.children.entries()) {
    const shader = {uniforms:{},vertexShader:'#include <begin_vertex>',fragmentShader:'#include <color_fragment>\n#include <normal_fragment_maps>'};
    mesh.material.onBeforeCompile(shader);
    assert.equal(shader.uniforms.terrainRelief?.value, [undefined,undefined,undefined,0.5,1][index]);
    assert.equal(shader.fragmentShader.includes('dFdx(relief)'),index >= 3);
  }
});

test('odd tile widths keep exact LOD edge samples and finite skirt positions',async t=>{
  for(const samples of [129,130]) await t.test(`${samples} samples`,async t=>{
    const {api,world,errors}=await terrainFixture(t,{size:[100.1,100.1],tile:100.1,samples});
    const camera=new THREE.PerspectiveCamera();camera.position.set(50.05,150,50.05);camera.lookAt(50.05,75,50.05);
    api.update(camera,0);await new Promise(resolve=>setImmediate(resolve));
    for(const [index,distance] of [0,300,600,850].entries()) {
      camera.position.x=50.05+distance;camera.lookAt(50.05,75,50.05);api.update(camera,(index+1)*500);
      const mesh=world.children.find(mesh=>mesh.visible),positions=mesh.geometry.attributes.position;
      const stride=2**index,n=Math.ceil((samples-1)/stride)+1;
      assert.equal(positions.count,n*n+4*(n-1));
      assert.ok([...positions.array,...mesh.geometry.attributes.normal.array].every(Number.isFinite));
      for(let i=1;i<n;i++) assert.ok(positions.getX(i)>positions.getX(i-1));
      assert.ok(Math.abs(positions.getX(n-1)-100.1)<.00001);
      assert.ok(Math.abs(positions.getY(n*n)-positions.getY(0)+5)<.00001);
    }
    assert.deepEqual(errors,[]);
  });
});

test('coarse edge meshes preserve the grid slope up to the real edge and stay clamped beyond it',async t=>{
  const coarseValues=[0,2,3].flatMap(z=>[0,2,3].map(x=>Math.round((x+z)/6*65535)));
  const {api,world}=await terrainFixture(t,{coarseValues});
  for(let i=0;i<4;i++)api.update(new THREE.PerspectiveCamera(),i);
  world.updateMatrixWorld(true);
  for(const [x,z] of [[2.25,.5],[2.9,2.6],[.4,2.8],[3,3],[3.5,3.5]]) {
    const ray=new THREE.Raycaster(new THREE.Vector3(x,1000,z),new THREE.Vector3(0,-1,0));
    const hits=ray.intersectObjects(world.children);
    assert.ok(hits.length,`missing coarse ground at ${x},${z}`);
    assert.ok(Math.abs(hits[0].point.y-api.heightAt(x,z))<.002,`mesh/query mismatch at ${x},${z}: ${hits[0].point.y} vs ${api.heightAt(x,z)}`);
  }
});

test('initialization preloads player and dynamic spawn tiles before load, deduplicating terrain offsets',async t=>{
  const offset=[1000,7,2000];
  const entities=[{props:{ai_terrain:'terrain.json',ai_terrain_radius:1},transform:{position:offset}},
    {tags:['player'],transform:{position:[1128,0,2128]}},
    {tags:['player'],transform:{position:[1128,0,2128]}},
    {rigidbody:{type:'dynamic'},transform:{position:[1896,0,2896]}},
    {rigidbody:{type:'static'},transform:{position:[1512,0,2512]}}];
  const {api,inserted,requests}=await terrainFixture(t,{size:[1024,1024],tile:256,entities});
  assert.deepEqual(inserted.map(([path,x,z])=>[path,x,z]).sort(),[[0,0],[0,1],[1,0],[2,3],[3,2],[3,3]].map(([x,z])=>['terrain.json',x,z]));
  assert.equal(requests.filter(url=>url.includes('/tiles/')).length,6);
  const load=()=>api.heightAt(1128,2128);
  const first=load(); await api.preload(); const restart=load();
  assert.ok(Math.abs(first-82)<.001); assert.equal(restart,first);
});

test('initial spawn preload rejects failed tiles rather than allowing a coarse-data load',async t=>{
  await assert.rejects(terrainFixture(t,{entities:[{props:{ai_terrain:'terrain.json'}},{tags:['player']}],failTile:true}),/HTTP 404/);
});

test('hidden terrain keeps physics and heights but creates no coarse or fine meshes',async t=>{
  const {api,world,registered,inserted,errors}=await terrainFixture(t,{entities:[{visible:false,props:{ai_terrain:'terrain.json'}},{tags:['player']}]});
  assert.equal(registered.length,1); assert.equal(inserted.length,4);
  assert.equal(world.children.length,0);
  assert.ok(Math.abs(api.heightAt(1,1)-75)<.001);
  const camera=new THREE.PerspectiveCamera(),far=camera.far;
  api.update(camera,0); await new Promise(resolve=>setImmediate(resolve)); api.update(camera,500);
  assert.equal(world.children.length,0);
  assert.equal(camera.far,far); assert.equal(world.fog.near,1); assert.equal(world.fog.far,100);
  assert.equal(api.stats().terrainTilesVisible,0); assert.equal(api.stats().terrainTriangles,0);
  assert.deepEqual(errors,[]);
});

test('terrain quantization decodes LE heights and rejects malformed and empty fields', () => {
  const bytes = new Uint8Array(20), view = new DataView(bytes.buffer);
  bytes.set(new TextEncoder().encode('AITC')); view.setUint32(4,2,true); view.setUint32(8,2,true);
  view.setUint16(12,0,true); view.setUint16(14,65535,true); view.setUint16(16,32768,true);
  const values=decodeHeights(bytes,'AITC',[2,2],[-10,90]);
  assert.equal(values[0],-10); assert.equal(values[1],90); assert.ok(Math.abs(values[2]-40)<0.001);
  assert.throws(()=>decodeHeights(new Uint8Array(),'AITC',[2,2],[0,1]));
  assert.throws(()=>decodeHeights(bytes,'AITT',[2,2],[0,1]));
  assert.throws(()=>decodeHeights(bytes,'AITC',[3,2],[0,1]));
});
test('terrain sampling uses the native triangle diagonal and clamps to tile', () => {
  const heights=new Float32Array(16);
  heights[10]=10;
  const c={samples:2,tile_m:256};
  assert.equal(sampleTile(heights,c,25,25,6400+128,6400+128),0);
  assert.equal(sampleTile(heights,c,25,25,6400+192,6400+192),5);
  assert.equal(sampleTile(heights,c,25,25,9000,9000),10);
});
test('LOD hysteresis prevents oscillation and supports all four strides', () => {
  assert.equal(terrainLOD(0),1); assert.equal(terrainLOD(300),2); assert.equal(terrainLOD(600),4); assert.equal(terrainLOD(1100),8);
  assert.equal(terrainLOD(190,1),1); assert.equal(terrainLOD(160,2),2); assert.equal(terrainLOD(140,2),1);
});
test('nearest terrain work has count and time budgets, including empty and failed jobs',()=>{
  const jobs=[{distance:30},{distance:1},{distance:8}],seen=[];
  assert.equal(drainTerrainWork(jobs,j=>seen.push(j.distance),2,3,()=>0),2);
  assert.deepEqual(seen,[1,8]);assert.equal(jobs.length,1);
  let now=0;
  assert.equal(drainTerrainWork([{distance:0},{distance:1}],()=>{now+=4;},2,3,()=>now),1);
  assert.equal(drainTerrainWork([],()=>assert.fail()),0);
  assert.throws(()=>drainTerrainWork([{distance:1}],()=>{throw new Error('build failed');}),/build failed/);
});
test('terrain reach is quality dependent and never exceeds the authored far plane',()=>{
  assert.deepEqual([0,1,2].map(q=>terrainReach(600,q)),[320,480,600]);
  assert.equal(terrainReach(100,2),100);assert.equal(terrainReach(2000,2),900);
});
test('lazy coarse geometry is cheaper than the farthest detailed LOD and activation is budgeted across turns',async t=>{
  const {api,world}=await terrainFixture(t,{size:[2048,2048],tile:256,samples:33});
  const camera=new THREE.PerspectiveCamera(120,1,0.1,600);camera.position.set(1024,200,1024);camera.lookAt(1024,0,0);
  assert.equal(world.children.length,0);api.update(camera,0);
  assert.ok(world.children.length<=2);assert.ok(api.stats().terrainActivations<=2);assert.equal(camera.far,600);
  const coarse=world.children[0];assert.ok(coarse.geometry.index.count/3<64);
  for(let i=0;i<12;i++){api.update(camera,i);assert.ok(api.stats().terrainBuilds<=2);assert.ok(api.stats().terrainActivations<=2);for(const mesh of world.children)if(mesh.visible)mesh.onBeforeRender();}
  camera.lookAt(1024,0,2048);api.update(camera,20);assert.ok(api.stats().terrainActivations<=2);
  world.fog=null;assert.doesNotThrow(()=>api.update(camera,21));
});
test('queue limits concurrency, deduplicates, evicts LRU and retries errors', async () => {
  let active=0,max=0,calls=0;
  const releases=[];
  const queue=new TileQueue(async url=>{
    calls++; active++; max=Math.max(max,active);
    await new Promise(resolve=>releases.push(resolve)); active--;
    if(url==='bad') throw new Error('HTTP 404');
    return new Uint8Array([1,2]);
  },2,2);
  const a=queue.get('a'), duplicate=queue.get('a'), b=queue.get('b'), c=queue.get('c');
  assert.equal(a,duplicate); await Promise.resolve(); assert.equal(max,2);
  releases.shift()(); releases.shift()(); await Promise.all([a,b]);
  await new Promise(resolve=>setImmediate(resolve)); releases.shift()(); await c;
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(calls,3); assert.equal(queue.bytes,6); assert.equal(queue.cache.size,2); assert.equal(queue.cache.has('a'),false);
  assert.deepEqual(await queue.get('b'),new Uint8Array([1,2]));
  for(let i=0;i<2;i++) {const failure=queue.get('bad'); await Promise.resolve(); releases.shift()(); await assert.rejects(failure,/404/); await new Promise(resolve=>setImmediate(resolve));}
  assert.equal(queue.pending.size,0); assert.equal(queue.active,0);
});

test('visuals receives offset-aware coarse/full-res heights immediately or on a later poll', async t => {
  const originals = ['window','location'].map(key=>[key,Object.getOwnPropertyDescriptor(globalThis,key)]);
  t.after(()=>{for(const [key,descriptor] of originals) {if(descriptor) Object.defineProperty(globalThis,key,descriptor); else delete globalThis[key];}});
  globalThis.location={href:'http://localhost/demo/'};
  const manifest={config:{size_m:[2,2],tile_m:2,samples:3},grid:[1,1],height_range:[0,100],coarse:{dims:[2,2],step:2,file:'coarse.bin'},tiles:'tiles/{x}_{z}.bin'};
  function field(magic,n,value) {
    const bytes=new Uint8Array(12+n*n*2), view=new DataView(bytes.buffer);
    bytes.set(new TextEncoder().encode(magic)); view.setUint32(4,n,true); view.setUint32(8,n,true);
    for(let i=0;i<n*n;i++) view.setUint16(12+i*2,value,true);
    return bytes;
  }
  t.mock.method(globalThis,'fetch',async url=>new Response(url.endsWith('.json') ? JSON.stringify(manifest) : url.endsWith('coarse.bin') ? field('AITC',2,16384) : field('AITT',5,49151)));
  for(const late of [false,true]) {
    globalThis.window={};
    const samplers=[];
    const visuals={setHeightSampler(sampler){assert.equal(this,visuals);samplers.push(sampler);}};
    if(!late) window.__aiEngineVisuals=visuals;
    const world=new THREE.Scene(); world.fog=new THREE.Fog(0,1,100);
    const errors=[];
    const api=await createTerrain(THREE,[{props:{ai_terrain:'terrain.json'},transform:{position:[100,7,200]}}],{set_terrain(){},terrain_tiles_wanted(){return '[]';}},world,{},error=>errors.push(error));
    assert.equal(samplers.length,late?0:1);
    if(late) {window.__aiEngineVisuals={setHeightSampler:null};api.update(new THREE.PerspectiveCamera(),0);window.__aiEngineVisuals=visuals;api.update(new THREE.PerspectiveCamera(),500);}
    assert.equal(samplers[0],api.heightAt);
    assert.ok(Math.abs(samplers[0](101,201)-32)<.001);
    await new Promise(resolve=>setImmediate(resolve));
    if(!late) {api.update(new THREE.PerspectiveCamera(),0);await new Promise(resolve=>setImmediate(resolve));}
    assert.ok(Math.abs(samplers[0](101,201)-82)<.001);
    assert.ok(Math.abs(samplers[0](-100,-200)-32)<.001);
    api.update(new THREE.PerspectiveCamera(),1000);
    assert.equal(samplers.length,1);
    const replacement=[]; window.__aiEngineVisuals={setHeightSampler:s=>replacement.push(s)};
    api.update(new THREE.PerspectiveCamera(),1500);
    assert.equal(replacement[0],api.heightAt);
    assert.deepEqual(errors,[]);
  }
});

test('occluder sampler ignores hidden terrain and returns NaN outside the map', async t=>{
  const {api}=await terrainFixture(t,{entities:[{props:{ai_terrain:'terrain.json'},transform:{position:[10,0,0]}}]});
  assert.ok(Math.abs(api.occluderHeightAt(11,1)-api.heightAt(11,1))<1e-6);
  assert.ok(Number.isNaN(api.occluderHeightAt(9,1)));
  assert.ok(Number.isNaN(api.occluderHeightAt(14,1)));
  assert.ok(Number.isFinite(api.heightAt(9,1)));
});

test('invisible terrain never occludes', async t=>{
  const {api}=await terrainFixture(t,{entities:[{visible:false,props:{ai_terrain:'terrain.json'}}]});
  assert.ok(Number.isNaN(api.occluderHeightAt(1,1)));
});
