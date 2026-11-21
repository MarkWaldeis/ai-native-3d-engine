import assert from 'node:assert/strict';
import {registerHooks} from 'node:module';
import test from 'node:test';

registerHooks({resolve(specifier,context,nextResolve) {
  return nextResolve(specifier === 'three' ? new URL('../crates/engine-web/player/vendor/three.module.js',import.meta.url).href : specifier,context);
}});
const THREE = await import('three');
const {StreamingWorld} = await import('../crates/engine-web/player/world.js');
const {worldConfig} = await import('../crates/engine-web/player/streaming.js');
const {batchKey} = await import('../crates/engine-web/player/streaming.js');

function glb(scale) {
  const document={asset:{version:'2.0'},buffers:[{byteLength:36}],bufferViews:[{buffer:0,byteLength:36}],accessors:[{bufferView:0,componentType:5126,count:3,type:'VEC3',min:[0,0,0],max:[scale,scale,0]}],meshes:[{primitives:[{attributes:{POSITION:0}}]}],nodes:[{mesh:0}],scenes:[{nodes:[0]}],scene:0};
  const json=Buffer.from(JSON.stringify(document).padEnd(Math.ceil(JSON.stringify(document).length/4)*4,' '));
  const bin=Buffer.alloc(36);[0,0,0,scale,0,0,0,scale,0].forEach((v,i)=>bin.writeFloatLE(v,i*4));
  const header=Buffer.alloc(20);header.write('glTF');header.writeUInt32LE(2,4);header.writeUInt32LE(28+json.length+bin.length,8);header.writeUInt32LE(json.length,12);header.writeUInt32LE(0x4e4f534a,16);
  const chunk=Buffer.alloc(8);chunk.writeUInt32LE(bin.length);chunk.writeUInt32LE(0x004e4942,4);
  return 'data:model/gltf-binary;base64,'+Buffer.concat([header,json,chunk,bin]).toString('base64');
}

test('authored GLB LOD paths use the existing distance levels and keep distinct batch/cache identities', async () => {
  globalThis.ProgressEvent ??= class ProgressEvent {constructor(type,details){this.type=type;Object.assign(this,details);}};
  const {world}=fixture();let impostors=0;world.impostor=()=>{impostors++;return [];};
  const base={kind:'gltf',path:glb(3)},authored={...base,props:{web_lods:{1:glb(2),2:glb(1)}}};
  assert.notEqual(batchKey(base),batchKey(authored));
  for (const [level,max] of [[0,3],[1,2],[2,1]]) {const parts=await world.template(authored,level);assert.equal(parts[0].geometry.attributes.position.getX(1),max);}
  assert.equal(impostors,0);
  await world.template(base,2);assert.equal(impostors,1);
  await world.template({...base,props:{web_lods:{}}},2);assert.equal(impostors,2);
  await assert.rejects(world.template({...base,props:{web_lods:{1:'data:model/gltf-binary;base64,YmFk'}}},1));
});

test('animated GLBs switch authored LOD meshes at both distances and restore the base mesh', async () => {
  const {world} = fixture();
  const object = new THREE.Group(), high = new THREE.Group(); object.add(high);
  const entity = {id:4,kind:'gltf',path:glb(3),props:{web_lods:{1:glb(2),2:glb(1)}},script:'animated.rhai',transform:{position:[0,0,0],scale:[1,1,1],rotation_deg:[0,0,0]}};
  world.add(entity,object);
  const camera = new THREE.PerspectiveCamera();
  for (const [distance,level] of [[150,1],[300,2]]) {
    camera.position.set(distance,10,0); camera.lookAt(0,0,0); camera.updateMatrixWorld();
    await Promise.all(world.update(camera)); world.update(camera);
    assert.equal(high.visible,false);
    const item = world.entities.get(4);
    assert.equal(item.lods.get(entity.props.web_lods[level]).visible,true);
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
  globalThis.ProgressEvent ??= class ProgressEvent {constructor(type,details){this.type=type;Object.assign(this,details);}};
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
  assert.ok(frustum.intersectsBox(old.userData.bounds),'the ground is inside the frustum at the reported location');
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
