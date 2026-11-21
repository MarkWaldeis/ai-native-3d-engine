import assert from 'node:assert/strict';
import {registerHooks} from 'node:module';
import test from 'node:test';

registerHooks({resolve(specifier,context,nextResolve) {
  return nextResolve(specifier === 'three' ? new URL('../crates/engine-web/player/vendor/three.module.js',import.meta.url).href : specifier,context);
}});
const THREE = await import('three');
const {StreamingWorld} = await import('../crates/engine-web/player/world.js');
const {worldConfig} = await import('../crates/engine-web/player/streaming.js');

function fixture(addProp = false) {
  const scene = new THREE.Scene(), world = new StreamingWorld(scene,{},worldConfig([{tags:['player'],props:{ai_world:{}}}]));
  const ground = {id:1,kind:'box',size:[256,0.4,256],collider:{shape:'box'},transform:{position:[-128,-0.2,-128],scale:[1,1,1],rotation_deg:[0,0,0]}};
  world.add(ground);
  if (addProp) world.add({id:2,kind:'sphere',radius:1,transform:{position:[-53.802856,1,-72.73966],scale:[1,1,1],rotation_deg:[0,0,0]}});
  world.grid.update([20.4,6.5,-1.4]); world.dirty.clear();
  return {scene,world,key:world.grid.key(ground.transform.position)};
}

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
