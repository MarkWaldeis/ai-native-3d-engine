import assert from 'node:assert/strict';
import {registerHooks} from 'node:module';
import test from 'node:test';

registerHooks({resolve(specifier,context,nextResolve) {
  return nextResolve(specifier === 'three' ? new URL('../crates/engine-web/player/vendor/three.module.js',import.meta.url).href : specifier,context);
}});
globalThis.ProgressEvent ??= class ProgressEvent {constructor(type,details){this.type=type;Object.assign(this,details);}};
const THREE = await import('three');
const {disposeObject, StreamingWorld, WebAssets} = await import('../crates/engine-web/player/world.js');
const {worldConfig} = await import('../crates/engine-web/player/streaming.js');
test('chunk rebuild starts are nearest first with two starts and two in flight, retaining dirty work',async()=>{
  const world=new StreamingWorld(new THREE.Scene(),{},worldConfig([{tags:['player'],props:{ai_world:{chunk_size:32,stream_radius:512}}}]));
  for(const [id,x] of [[1,200],[2,20],[3,80],[4,140]])world.add({id,kind:'box',size:[1,1,1],transform:{position:[x,0,0],scale:[1,1,1],rotation_deg:[0,0,0]}});
  const camera=new THREE.PerspectiveCamera(),template=world.template.bind(world),releases=[];
  world.template=(...args)=>new Promise(resolve=>releases.push(async()=>resolve(await template(...args))));
  const tasks=world.update(camera);assert.equal(tasks.length,2);assert.equal(world.activeBuilds,2);
  assert.deepEqual([...world.builds.keys()],['0,0','2,0']);assert.equal(world.update(camera).length,0);assert.equal(world.dirty.size,2);
  await Promise.all(releases.splice(0).map(f=>f()));await Promise.all(tasks);
  const next=world.update(camera);assert.equal(next.length,2);await Promise.all(releases.splice(0).map(f=>f()));await Promise.all(next);
  assert.equal(world.activeBuilds,0);assert.equal(world.dirty.size,0);assert.equal(world.groups.size,4);
});
const {batchKey} = await import('../crates/engine-web/player/streaming.js');
const {TerrainOcclusion} = await import('../crates/engine-web/player/occlusion.js');

test('streamed chunk visibility combines frustum with stable terrain occlusion',async()=>{
  const scene=new THREE.Scene(), config=worldConfig([{tags:['player'],props:{ai_world:{}}}]);
  const world=new StreamingWorld(scene,{},config);
  const occluder=new TerrainOcclusion(x=>x>40 && x<60 ? 20 : 0);
  world.setOcclusion(occluder);
  world.add({id:1,kind:'box',size:[4,4,4],transform:{position:[100,2,0],scale:[1,1,1],rotation_deg:[0,0,0]}});
  const camera=new THREE.PerspectiveCamera(60,1,0.1,500);
  camera.position.set(0,5,0); camera.lookAt(100,2,0); camera.updateMatrixWorld();
  await Promise.all(world.update(camera));
  const group=[...world.groups.values()][0];
  occluder.beginFrame();world.update(camera);assert.equal(group.visible,true);
  occluder.beginFrame();world.update(camera);assert.equal(group.visible,false);assert.equal(world.occludedChunks,1);
  occluder.enabled=false;occluder.beginFrame();world.update(camera);assert.equal(group.visible,true);assert.equal(world.occludedChunks,0);
});
const {primitiveMaterial} = await import('../crates/engine-web/player/materials.js');

test('primitive maps share source pixels, distinguish UV variants, and work in streaming templates', async () => {
  const surface = 'assets/material/0.png', packed = 'assets/material/1.png', calls = [];
  const scene = new THREE.Scene(), assets = new WebAssets(4);
  const material = {base_color_map:surface, normal_map:surface, orm_map:packed, uv_repeat:[3,2], normal_scale:9};
  const box = {kind:'box', size:[1,1,1], props:{ai_material:material}};
  const tiled = {...box, props:{ai_material:{...material, uv_repeat:[4,2], metalness:0.35}}};
  const model = {kind:'gltf', props:{ai_material:{base_color_map:'assets/material/2.png'}}};
  const invalid = {...box, props:{ai_material:{base_color_map:'https://example.invalid/color.png'}}};
  await assets.preloadMaterialMaps([box, tiled, model, invalid], { async loadAsync(path) {
    calls.push(path);
    return new THREE.Texture();
  } });
  assert.deepEqual(calls, [surface, packed]);
  const regular = primitiveMaterial(box, assets), alternative = primitiveMaterial(tiled, assets);
  assert.equal(regular.map.colorSpace, THREE.SRGBColorSpace);
  assert.equal(regular.normalMap.colorSpace, THREE.NoColorSpace);
  assert.equal(regular.map.source, regular.normalMap.source);
  assert.equal(regular.map.source, alternative.map.source);
  assert.notEqual(regular.map, alternative.map);
  assert.deepEqual(regular.map.repeat.toArray(), [3,2]);
  assert.deepEqual(alternative.map.repeat.toArray(), [4,2]);
  assert.equal(regular.map.wrapS, THREE.RepeatWrapping);
  assert.equal(regular.map.anisotropy, 4);
  assert.equal(regular.map.userData.shared, true);
  assert.deepEqual(regular.normalScale.toArray(), [2,2]);
  assert.equal(regular.aoMap, regular.roughnessMap);
  assert.equal(regular.roughnessMap, regular.metalnessMap);
  assert.equal(regular.roughness, 1);
  assert.equal(regular.metalness, 1);
  assert.equal(alternative.metalness, 0.35);
  const world = new StreamingWorld(scene, {}, worldConfig([{tags:['player'],props:{ai_world:{}}}]), assets);
  assert.equal((await world.template(box, 0))[0].material.map, regular.map);
  assert.equal((await world.template(tiled, 1))[0].material.map, alternative.map);
  assert.notEqual(batchKey(box), batchKey(tiled));
  const explicitDefaults = {...box, props:{ai_material:{...material, metalness:0.08}}};
  assert.notEqual(batchKey(box), batchKey(explicitDefaults));
  assert.equal(primitiveMaterial(box, new WebAssets()).metalness, 0.08, 'use scalar defaults if the image is missing');
});

function glb(scale) {
  const document={asset:{version:'2.0'},buffers:[{byteLength:36}],bufferViews:[{buffer:0,byteLength:36}],accessors:[{bufferView:0,componentType:5126,count:3,type:'VEC3',min:[0,0,0],max:[scale,scale,0]}],meshes:[{primitives:[{attributes:{POSITION:0}}]}],nodes:[{mesh:0}],scenes:[{nodes:[0]}],scene:0};
  const json=Buffer.from(JSON.stringify(document).padEnd(Math.ceil(JSON.stringify(document).length/4)*4,' '));
  const bin=Buffer.alloc(36);[0,0,0,scale,0,0,0,scale,0].forEach((v,i)=>bin.writeFloatLE(v,i*4));
  const header=Buffer.alloc(20);header.write('glTF');header.writeUInt32LE(2,4);header.writeUInt32LE(28+json.length+bin.length,8);header.writeUInt32LE(json.length,12);header.writeUInt32LE(0x4e4f534a,16);
  const chunk=Buffer.alloc(8);chunk.writeUInt32LE(bin.length);chunk.writeUInt32LE(0x004e4942,4);
  return 'data:model/gltf-binary;base64,'+Buffer.concat([header,json,chunk,bin]).toString('base64');
}

test('authored GLB LOD paths use the existing distance levels and keep distinct batch/cache identities', async () => {
  const {world}=fixture();let impostors=0;world.impostor=()=>{impostors++;return [];};
  const base={kind:'gltf',path:glb(3)},authored={...base,props:{web_lods:{1:glb(2),2:glb(1)}}};
  assert.notEqual(batchKey(base),batchKey(authored));
  for (const [level,max] of [[0,3],[1,2],[2,1]]) {const parts=await world.template(authored,level);assert.equal(parts[0].geometry.attributes.position.getX(1),max);}
  assert.equal(impostors,0);
  await world.template(base,2);assert.equal(impostors,1);
  await world.template({...base,props:{web_lods:{}}},2);assert.equal(impostors,2);
  await assert.rejects(world.template({...base,props:{web_lods:{1:'data:model/gltf-binary;base64,YmFk'}}},1));
});

test('unbatched GLBs preserve shadow settings at both authored LOD distances', async () => {
  const {world} = fixture();
  const object = new THREE.Group(), high = new THREE.Group(); object.add(high);
  const entity = {id:4,kind:'gltf',path:glb(3),props:{web_lods:{1:glb(2),2:glb(1)},ai_sun_shadow:{cast:false}},script:'actor.rhai',transform:{position:[0,0,0],scale:[1,1,1],rotation_deg:[0,0,0]}};
  world.add(entity,object);
  const camera = new THREE.PerspectiveCamera();
  for (const [distance,level] of [[150,1],[300,2]]) {
    camera.position.set(distance,10,0); camera.lookAt(0,0,0); camera.updateMatrixWorld();
    await Promise.all(world.update(camera)); world.update(camera);
    assert.equal(high.visible,false);
    const item = world.entities.get(4);
    const lod = item.lods.get(entity.props.web_lods[level]);
    assert.equal(lod.visible,true);
    const source = (await world.assets.load(entity.props.web_lods[level])).scene;
    const mesh = lod.getObjectByProperty('isMesh',true);
    assert.equal(mesh.castShadow,false);
    assert.equal(mesh.receiveShadow,true);
    assert.equal(source.getObjectByProperty('isMesh',true).receiveShadow,false,'per-entity flags must not alter shared assets');
    assert.equal([...item.lods.values()].filter(lod=>lod.visible).length,1);
    assert.equal(item.impostor,undefined);
  }
  camera.position.set(0,10,0); camera.updateMatrixWorld(); world.update(camera);
  assert.equal(high.visible,true);
  assert.ok([...world.entities.get(4).lods.values()].every(lod=>!lod.visible));
});

function fixture(addProp = false) {
  const scene = new THREE.Scene(), world = new StreamingWorld(scene,{},worldConfig([{tags:['player'],props:{ai_world:{}}}]));
  const ground = {id:1,kind:'box',size:[256,0.4,256],collider:{shape:'box'},transform:{position:[-128,-0.2,-128],scale:[1,1,1],rotation_deg:[0,0,0]}};
  world.add(ground);
  if (addProp) world.add({id:2,kind:'sphere',radius:1,transform:{position:[-53.802856,1,-72.73966],scale:[1,1,1],rotation_deg:[0,0,0]}});
  world.grid.update([20.4,6.5,-1.4]); world.dirty.clear();
  return {scene,world,key:world.grid.key(ground.transform.position)};
}

test('four-angle impostors share one target and restore the previous renderer state', () => {
  const previous = {}, previousColor = new THREE.Color('#63758c');
  let target = previous;
  const draws = [];
  const renderer = {
    autoClear:false,
    getRenderTarget:()=>target, getActiveCubeFace:()=>2, getActiveMipmapLevel:()=>1,
    getClearColor:color=>color.copy(previousColor), getClearAlpha:()=>0.7,
    setClearColor(color,alpha) { this.color = color; this.alpha = alpha; },
    setRenderTarget(t,face,mip) { target = t; this.face = face; this.mip = mip; },
    clear() {},
    render(scene,camera) { draws.push([target, target.viewport.toArray(), camera.position.clone()]); },
  };
  const world = new StreamingWorld(new THREE.Scene(),renderer,{impostor_views:4});
  const source = new THREE.Mesh(new THREE.BoxGeometry(2,2,2),new THREE.MeshBasicMaterial());
  source.position.set(3,0,-2);
  const [part] = world.impostor(source);
  assert.deepEqual(draws.map(draw=>draw[1]),[[0,0,64,64],[64,0,64,64],[0,64,64,64],[64,64,64,64]]);
  assert.ok(draws.every(draw => draw[0] === draws[0][0]));
  assert.deepEqual(draws.map(draw=>draw[2].sub(source.position).toArray().map(v => Math.round(v) || 0)),
    [[0,0,4],[4,0,0],[0,0,-4],[-4,0,0]]);
  assert.equal(part.material.map.image.width,128);
  assert.equal(target,previous);
  assert.deepEqual([renderer.face,renderer.mip,renderer.alpha,renderer.autoClear],[2,1,0.7,false]);
  assert.ok(renderer.color.equals(previousColor));
});

test('streamed sun shadow flags split instance batches without duplicating templates', async () => {
  const {world,key} = fixture();
  const path = glb(1);
  for (const [index, props] of [
    {},
    {ai_sun_shadow:'{"cast":true,"receive":true}'},
    {ai_sun_shadow:{cast:false}},
    {ai_sun_shadow:{receive:false}},
  ].entries()) world.add({
    id:10+index, kind:'gltf', path, props,
    transform:{position:[-128,-0.2,-128],scale:[1,1,1],rotation_deg:[0,0,0]},
  });
  world.assetStream.update(new Map([[path,0]]));
  await new Promise(resolve=>setImmediate(resolve));
  world.assetStream.update(new Map([[path,0]]));
  await world.rebuild(key);
  const meshes = world.groups.get(key).children;
  const find = id => meshes.find(mesh=>mesh.userData.ids.has(id));
  const a=find(10), b=find(12), c=find(13);
  assert.equal(a, find(11));
  assert.equal(a.geometry, b.geometry);
  assert.equal(b.material, c.material);
  assert.deepEqual([a,b,c].map(mesh=>[mesh.castShadow,mesh.receiveShadow]), [[true,true],[false,true],[true,false]]);
});

test('LOD rebuild retains visible ground until its replacement is ready', async () => {
  const {scene,world,key} = fixture(true);
  await world.rebuild(key);
  const old = world.groups.get(key);
  const camera = new THREE.PerspectiveCamera(55,1.6,0.1,480);
  camera.position.set(20.4,6.5,-1.4); camera.lookAt(13.4,0.5,-10.4); camera.updateMatrixWorld();
  const frustum = new THREE.Frustum().setFromProjectionMatrix(new THREE.Matrix4().multiplyMatrices(camera.projectionMatrix,camera.matrixWorldInverse));
  assert.ok(frustum.intersectsObject(old.children[0]),'the ground is inside the frustum at the reported location');
  let ready;
  const template = world.template.bind(world), gate = new Promise(resolve => {ready=resolve;});
  world.template = async (...args) => {await gate; return template(...args);};
  world.entities.get(2).level = 2;
  world.update(camera);
  assert.ok(world.grid.loaded.has(key),'nearby large ground tile remains loaded');
  assert.equal(old.parent,scene,'ground must stay attached during asynchronous template lookup');
  assert.equal(world.groups.get(key),old);
  ready();
  while (world.builds.size) await new Promise(resolve=>setImmediate(resolve));
  assert.equal(old.parent,null);
  assert.equal(world.groups.get(key).parent,scene);
  assert.equal(world.groups.get(key).children.length,2);
});

test('a loaded chunk outside the camera can still contribute to the shadow pass', async () => {
  const scene = new THREE.Scene(), world = new StreamingWorld(scene,{},worldConfig([
    {tags:['player'],props:{ai_world:{chunk_size:2,stream_radius:16}}},
  ]));
  const camera = new THREE.OrthographicCamera(-1,1,1,-1,0.1,20);
  camera.position.z = 5; camera.updateMatrixWorld();
  const caster = {id:1,kind:'box',size:[1,1,1],transform:{position:[3,0,0],scale:[1,1,1],rotation_deg:[0,0,0]}};
  world.add(caster);
  await Promise.all(world.update(camera)); world.update(camera);
  const group = world.groups.get(world.grid.key(caster.transform.position));
  const frustum = new THREE.Frustum().setFromProjectionMatrix(new THREE.Matrix4().multiplyMatrices(camera.projectionMatrix,camera.matrixWorldInverse));
  assert.equal(frustum.intersectsObject(group.children[0]),false);
  assert.equal(group.children[0].castShadow,true);
  assert.equal(group.visible,true);
});

test('failed, superseded and reset rebuilds cannot replace a complete chunk', async () => {
  const {scene,world,key} = fixture();
  await world.rebuild(key);
  const old = world.groups.get(key), template = world.template.bind(world);
  world.template = async () => {throw new Error('asset unavailable');};
  await assert.rejects(world.rebuild(key),/asset unavailable/);
  assert.equal(old.parent,scene);
  const pending = [];
  world.template = (...args) => new Promise(resolve => pending.push(async () => resolve(await template(...args))));
  const first = world.rebuild(key), second = world.rebuild(key);
  await pending[1](); await second;
  const newest = world.groups.get(key);
  await pending[0](); await first;
  assert.equal(world.groups.get(key),newest);
  assert.equal(newest.parent,scene);
  const reset = world.rebuild(key);
  world.reset(); await pending[2](); await reset;
  assert.equal(scene.children.length,0);
  assert.equal(world.groups.size,0);
});

test('unloading a distant chunk releases its instance buffers and reuses templates on return', async () => {
  const {scene,world,key} = fixture();
  const actor = new THREE.Group();
  world.add({...world.entities.get(1).e,id:5},actor);
  await world.rebuild(key);
  const camera = new THREE.PerspectiveCamera(), old = world.groups.get(key);
  const instance = old.children.find(o=>o.isInstancedMesh);
  let disposals = 0;
  instance.addEventListener('dispose',()=>disposals++);
  const geometry = instance.geometry, material = instance.material;
  const nested = new THREE.InstancedMesh(geometry,material,1);
  nested.userData.sharedAsset = true;
  actor.add(nested);
  let nestedDisposals = 0;
  nested.addEventListener('dispose',()=>nestedDisposals++);
  let templateDisposals = 0;
  for (const resource of [geometry,material]) resource.addEventListener('dispose',()=>templateDisposals++);
  camera.position.x = 2000; camera.updateMatrixWorld();
  assert.deepEqual(world.update(camera),[]);
  assert.equal(disposals,1);
  assert.equal(world.groups.has(key),false);
  assert.equal(old.parent,null);
  assert.equal(actor.parent,null);
  assert.equal(nestedDisposals,0);
  assert.equal(templateDisposals,0);
  camera.position.x = 0; camera.updateMatrixWorld();
  await Promise.all(world.update(camera));
  const restored = world.groups.get(key), mesh = restored.children.find(o=>o.isInstancedMesh);
  assert.notEqual(mesh,instance);
  assert.equal(mesh.geometry,geometry);
  assert.equal(mesh.material,material);
  assert.equal(restored.parent,scene);
  assert.equal(actor.parent,restored);
  world.reset();
  assert.equal(nestedDisposals,0);
  disposeObject(actor);
  assert.equal(nestedDisposals,1);
  assert.equal(templateDisposals,0);
});

test('an instance build in flight cannot restore a chunk after it has unloaded', async () => {
  const {world,key} = fixture();
  let release;
  const template = world.template.bind(world), gate = new Promise(resolve=>{release=resolve;});
  world.template = async (...args) => {await gate; return template(...args);};
  const camera = new THREE.PerspectiveCamera();
  camera.updateMatrixWorld();
  const build = world.update(camera);
  assert.equal(build.length,1);
  camera.position.x = 2000; camera.updateMatrixWorld();
  world.update(camera);
  release(); await Promise.all(build);
  assert.equal(world.groups.has(key),false);
  camera.position.x = 0; camera.updateMatrixWorld();
  await Promise.all(world.update(camera));
  assert.equal(world.groups.get(key).parent,world.scene);
});

test('streamed objectives honor authored collectible tags without including unrelated collectibles', () => {
  const {world} = fixture();
  for (const [id,tags] of [[10,['lantern']],[11,['coin']],[12,['goal']]]) {
    world.add({id,kind:'box',size:[1,1,1],tags,transform:{position:[0,0,0],scale:[1,1,1],rotation_deg:[0,0,0]}});
  }
  for (const key of world.grid.chunks.keys()) world.grid.loaded.add(key);
  assert.ok(world.objectiveEntries('lantern').some(e=>e.id===10));
  assert.ok(!world.objectiveEntries('lantern').some(e=>e.id===11));
  assert.ok(world.objectiveEntries('lantern').some(e=>e.id===12));
  assert.ok(!world.objectiveEntries().some(e=>e.id===10));
  world.reset();
  assert.deepEqual(world.objectiveEntries('lantern'),[]);
});

test('initial chunk preparation waits for templates, supports empty scenes and propagates failures', async () => {
  const {world,key} = fixture();
  const camera = new THREE.PerspectiveCamera();
  camera.position.set(20.4,6.5,-1.4); camera.lookAt(13.4,0.5,-10.4); camera.updateMatrixWorld();
  let ready;
  const template = world.template.bind(world), gate = new Promise(resolve=>{ready=resolve;});
  world.template = async (...args)=>{await gate; return template(...args);};
  world.dirty.add(key);
  const tasks = world.update(camera);
  assert.equal(tasks.length,1);
  assert.equal(world.groups.size,0);
  ready(); await Promise.all(tasks);
  assert.equal(world.groups.get(key).parent,world.scene);
  world.reset();
  assert.deepEqual(world.update(camera),[]);
  const entity = {id:3,kind:'box',size:[1,1,1],transform:{position:[0,0,0],scale:[1,1,1],rotation_deg:[0,0,0]}};
  world.add(entity);
  world.template = async ()=>{throw new Error('missing template');};
  await assert.rejects(Promise.all(world.update(camera)),/missing template/);
});
