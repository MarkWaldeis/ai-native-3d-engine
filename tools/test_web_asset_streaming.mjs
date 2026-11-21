import assert from 'node:assert/strict';
import test from 'node:test';
import {registerHooks} from 'node:module';
registerHooks({resolve(specifier,context,nextResolve) {
  return nextResolve(specifier==='three'?new URL('../crates/engine-web/player/vendor/three.module.js',import.meta.url).href:specifier,context);
}});
const THREE=await import('three');
const {AssetStream}=await import('../crates/engine-web/player/asset-streaming.js');
const {StreamingWorld,WebAssets}=await import('../crates/engine-web/player/world.js');
const {worldConfig}=await import('../crates/engine-web/player/streaming.js');
const settle=()=>new Promise(resolve=>setImmediate(resolve));
function model(count=0) {
  const scene=new THREE.Group(), material=new THREE.MeshStandardMaterial();
  for(const slot of ['map','normalMap','roughnessMap'].slice(0,count)) material[slot]=new THREE.DataTexture(new Uint8Array(64),4,4);
  scene.add(new THREE.Mesh(new THREE.BoxGeometry(),material));
  return {scene,animations:[]};
}
const wants=(...paths)=>new Map(paths.map((path,i)=>[path,i]));

test('default concurrency is four and active assets may exceed the soft memory target',async()=>{
  let calls=0;
  const stream=new AssetStream(async()=>{calls++;return model();},{},{asset_cache_mb:0.000001});
  const wanted=wants('a','b','c','d','e');
  stream.update(wanted);await settle();assert.equal(calls,4);
  assert.equal(stream.stats().assetsQueued,1);
  stream.update(wanted);await settle();assert.equal(calls,5);
  stream.update(wanted);assert.equal(stream.stats().assetsLoaded,5);assert.equal(stream.stats().assetEvictions,0);
  assert.ok(stream.stats().assetMB>stream.config.asset_cache_mb);
});

test('queue reprioritizes, deduplicates, caps concurrency and drops unwanted queued paths',async()=>{
  const started=[],finish=new Map();
  const stream=new AssetStream(path=>{started.push(path);return new Promise(resolve=>finish.set(path,resolve));},{},{asset_concurrency:1});
  stream.update(new Map([['far',100],['near',1],['middle',10],['gone',5]]));
  await settle();assert.deepEqual(started,['near']);assert.equal(stream.stats().assetsQueued,3);
  stream.update(new Map([['far',0],['middle',10]]));
  assert.equal(stream.entries.has('gone'),false);assert.equal(stream.stats().assetsLoading,1);
  finish.get('near')(model());await settle();stream.update(wants('far','middle'));await settle();
  assert.deepEqual(started,['near','far']);
  finish.get('far')(model());await settle();stream.update(wants('middle'));await settle();
  assert.deepEqual(started,['near','far','middle']);
  finish.get('middle')(model());await settle();stream.update(new Map());
  assert.equal(stream.stats().assetsLoading,0);
});

test('texture budget gates publication, including zero-texture and empty queues',async()=>{
  const uploaded=[],ready=[];
  const stream=new AssetStream(async path=>model(path==='textured'?3:0),{initTexture:t=>uploaded.push(t)},{texture_uploads_per_frame:2},path=>ready.push(path));
  stream.update(new Map());assert.equal(stream.stats().assetsLoaded,0);
  stream.update(wants('textured','plain'));await settle();
  assert.equal(stream.get('textured'),null);assert.equal(stream.stats().textureUploadsPending,3);
  stream.update(wants('textured','plain'));
  assert.equal(uploaded.length,2);assert.equal(stream.get('textured'),null);assert.ok(stream.get('plain'));
  stream.update(wants('textured','plain'));
  assert.equal(uploaded.length,3);assert.ok(stream.get('textured'));assert.deepEqual(ready,['plain','textured']);
});

test('loader and upload errors remain placeholders without unhandled rejection or retry storms',async()=>{
  const errors=[];let calls=0;
  const stream=new AssetStream(async path=>{calls++;if(path==='bad') throw Error('load failed');return model(1);},{initTexture(){throw Error('upload failed');}},{},(path,error)=>{if(error)errors.push(error.message);});
  stream.update(wants('bad','gpu'));await settle();stream.update(wants('bad','gpu'));await settle();
  assert.deepEqual(errors,['load failed','upload failed']);assert.equal(stream.get('gpu'),null);
  stream.update(wants('bad','gpu'));assert.equal(calls,2);
  stream.update(new Map());assert.equal(stream.entries.size,0);
});

test('LRU evicts only unpinned resources, disposes once and reloads on demand',async()=>{
  const calls=[],disposed=[],evicted=[];
  const stream=new AssetStream(async path=>{
    calls.push(path);const gltf=model(1),mesh=gltf.scene.children[0];
    for(const resource of [mesh.geometry,mesh.material,mesh.material.map]) resource.addEventListener('dispose',()=>disposed.push(path));
    return gltf;
  },{initTexture(){}},{asset_cache_mb:1},()=>{},path=>evicted.push(path));
  for(const path of ['old','recent','active']) {stream.update(wants(path));await settle();stream.update(wants(path));}
  const bytes=stream.entries.get('active').resources.bytes;
  stream.config.asset_cache_mb=bytes*2.1/1024/1024;
  stream.update(wants('active'));
  assert.deepEqual(evicted,['old']);assert.deepEqual(disposed,['old','old','old']);assert.ok(stream.get('active'));
  stream.update(wants('old'));await settle();stream.update(wants('old'));
  assert.equal(calls.filter(path=>path==='old').length,2);assert.ok(stream.get('old'));
});

function fixture(bounds=true,object) {
  const assets=new WebAssets(),loaded=[],pending=new Map();
  assets.loader={loadAsync:path=>{loaded.push(path);return new Promise(resolve=>pending.set(path,resolve));}};
  const world=new StreamingWorld(new THREE.Scene(),{initTexture(){}},worldConfig([{tags:['player'],props:{ai_world:{}}}]),assets);
  const e={id:1,kind:'gltf',path:'fine',props:{web_lods:{2:'coarse'},...(bounds?{web_mesh_bounds:[[-1,0,-1],[1,3,1]]}:{})},transform:{position:[0,0,0],scale:[1,1,1],rotation_deg:[0,0,0]}};
  world.add(e,object);return {world,loaded,pending,camera:new THREE.PerspectiveCamera()};
}

test('chunk rebuild never waits for GLB, swaps bounds placeholder for coarse then fine model',async()=>{
  const {world,loaded,pending,camera}=fixture();
  await Promise.all(world.update(camera));await settle();
  let mesh=world.groups.get('0,0').children[0];assert.equal(mesh.material.wireframe,true);assert.deepEqual(loaded,['coarse']);
  const coarse=model(1);pending.get('coarse')(coarse);await settle();
  assert.equal(world.groups.get('0,0').children[0],mesh);
  await Promise.all(world.update(camera));
  mesh=world.groups.get('0,0').children[0];assert.equal(mesh.geometry,coarse.scene.children[0].geometry);
  await Promise.all(world.update(camera));await settle();assert.deepEqual(loaded,['coarse','fine']);
  const fine=model();pending.get('fine')(fine);await settle();await Promise.all(world.update(camera));
  assert.equal(world.groups.get('0,0').children[0].geometry,fine.scene.children[0].geometry);
});

test('missing bounds render nothing; unwanted in-flight assets may finish after unload',async()=>{
  const {world,pending,camera}=fixture(false);
  await Promise.all(world.update(camera));await settle();assert.equal(world.groups.get('0,0').children.length,0);
  camera.position.x=10000;await Promise.all(world.update(camera));
  pending.get('coarse')(model());await settle();await Promise.all(world.update(camera));
  assert.equal(world.groups.size,0);assert.equal(world.assetStream.entries.get('coarse').pinned,false);
});

test('unbatched streaming uses the same ready-only swap callback',async()=>{
  const object=new THREE.Group(),swaps=[];object.userData.streamedGltf=(gltf,parts)=>swaps.push({gltf,parts});
  const {world,pending,camera}=fixture(true,object);
  await Promise.all(world.update(camera));await settle();assert.equal(swaps[0].gltf,null);assert.equal(swaps[0].parts.length,1);
  const gltf=model();pending.get('coarse')(gltf);await settle();await Promise.all(world.update(camera));
  assert.equal(swaps[1].gltf,gltf);
  camera.position.x=10000;await Promise.all(world.update(camera));
  assert.equal(swaps[2].gltf,null);assert.deepEqual(swaps[2].parts,[]);
});

test('ready-only impostor templates are invalidated on source eviction and regenerated on reload',async()=>{
  const {world,pending,camera}=fixture();
  world.entities.get(1).e.props.web_lods={};camera.position.x=300;
  let builds=0,disposed=0;
  world.impostor=()=>{builds++;const geometry=new THREE.PlaneGeometry(),material=new THREE.MeshBasicMaterial();geometry.addEventListener('dispose',()=>disposed++);return [{geometry,material,matrix:new THREE.Matrix4(),billboard:true}];};
  await Promise.all(world.update(camera));await settle();assert.equal(builds,0);
  pending.get('fine')(model());await settle();await Promise.all(world.update(camera));assert.equal(builds,1);
  world.assetStream.config.asset_cache_mb=0.000001;camera.position.x=10000;
  await Promise.all(world.update(camera));assert.equal(world.streamTemplates.size,0);assert.equal(disposed,1);
  camera.position.x=300;await Promise.all(world.update(camera));await settle();
  pending.get('fine')(model());await settle();await Promise.all(world.update(camera));assert.equal(builds,2);
});

test('asset config validates positive finite budgets and integer work caps; small demos stay disabled',()=>{
  const config=value=>worldConfig([{tags:['player'],props:{ai_world:value}}]);
  assert.equal(worldConfig([]),null);
  assert.equal(config({}).asset_concurrency,4);assert.equal(config({}).texture_uploads_per_frame,2);assert.equal(config({}).asset_cache_mb,256);
  for(const key of ['asset_concurrency','texture_uploads_per_frame','asset_cache_mb']) for(const value of [0,-1,NaN,Infinity,'4']) assert.throws(()=>config({[key]:value}),RangeError);
  for(const key of ['asset_concurrency','texture_uploads_per_frame']) assert.throws(()=>config({[key]:1.5}),RangeError);
  assert.equal(config({asset_cache_mb:0.1}).asset_cache_mb,0.1);
});

test('animated GLB entities load only their animated original, never static web_lods',async()=>{
  const {world,loaded,pending,camera}=fixture();
  world.entities.get(1).e.animator={machine:{states:{}}};
  await Promise.all(world.update(camera));await settle();
  assert.deepEqual(loaded,['fine']);
  assert.deepEqual(world.assetPaths(world.entities.get(1).e),['fine']);
  assert.equal(world.groups.get('0,0').children[0].material.wireframe,true);
  const fine=model();pending.get('fine')(fine);await settle();
  await Promise.all(world.update(camera));await settle();await Promise.all(world.update(camera));
  const meshes=[];world.groups.get('0,0').traverse(o=>{if(o.isMesh)meshes.push(o);});
  assert.ok(meshes.some(o=>o.geometry===fine.scene.children[0].geometry));
  assert.ok(!meshes.some(o=>o.material.wireframe));
});
