import test from 'node:test';
import assert from 'node:assert/strict';
import {registerHooks} from 'node:module';
registerHooks({resolve(s,c,next){return next(s==='three'?new URL('../crates/engine-web/player/vendor/three.module.js',import.meta.url).href:s,c);}});
const T=await import('three');
const {ShooterFx,FX_BUDGETS}=await import('../crates/engine-web/player/fx.js');
const {FirstPerson}=await import('../crates/engine-web/player/fps.js');
const gradient={addColorStop(){}};
globalThis.document={createElement(){return {getContext(){return {createRadialGradient(){return gradient;},createLinearGradient(){return gradient;},fillRect(){},translate(){},rotate(){},beginPath(){},moveTo(){},lineTo(){},fill(){}};}};}};
const event=kind=>({kind,pos:[1,2,3],dir:[0,1,0]});

test('FX storms reuse fixed objects, materials and buffers within every tier cap',()=>{
  const scene=new T.Scene(),fx=new ShooterFx(scene),children=[...scene.children];
  const buffers=fx.layers.map(l=>l.positions),materials=fx.layers.map(l=>l.points.material);
  for(let level=0;level<3;level++){
    fx.setQuality(level);const b=FX_BUDGETS[level];
    for(let i=0;i<400;i++)for(const kind of ['muzzle','impact','blood','explosion','tracer'])fx.emit(event(kind));
    fx.update(.1);
    for(const key of ['sparks','puffs','flashes','fire']){
      assert.ok(fx[key].alive<=b[key]);assert.ok(fx[key].pool.used<=b[key]);
    }
    assert.ok(fx.tracerPool.used<=b.tracers);assert.ok(fx.decals.count<=b.decals);assert.ok(fx.stats().lights<=b.lights);
    assert.deepEqual(scene.children,children);
    fx.layers.forEach((l,i)=>{assert.equal(l.positions,buffers[i]);assert.equal(l.points.material,materials[i]);});
    for(let i=0;i<80;i++)fx.update(.1);
    for(const layer of fx.layers)assert.equal(layer.alive,0);
    assert.equal(fx.tracers.visible,false);assert.equal(fx.decals.visible,false);assert.equal(fx.stats().lights,0);
  }
});

test('empty, invalid and reset FX are inert; tier changes discard hidden high-tier state',()=>{
  const scene=new T.Scene(),fx=new ShooterFx(scene);fx.update(.016);
  assert.ok(fx.layers.every(l=>!l.points.visible));assert.equal(fx.tracers.visible,false);
  for(const e of [null,{},event('unknown'),event('toString'),{...event('impact'),pos:[NaN,0,0]},{...event('impact'),dir:[Infinity,0,0]}])assert.equal(fx.emit(e),false);
  assert.equal(fx.emitted,0);fx.setQuality(2);fx.emit(event('explosion'));fx.emit(event('impact'));fx.update(.1);
  assert.ok(fx.fire.alive>0);assert.ok(fx.puffs.alive>0);assert.ok(fx.decals.visible);
  fx.setQuality(0);assert.ok(fx.layers.every(l=>l.pool.used===0));assert.ok(fx.lights.every(s=>!s.light.visible));assert.equal(fx.decals.count,0);
  fx.emit(event('muzzle'));fx.update(.1);assert.ok(fx.flashes.alive>0,'new flashes survive their first slow render frame');assert.equal(fx.stats().lights,0);
  fx.reset();fx.update(NaN);assert.ok(fx.layers.every(l=>!l.points.visible));assert.equal(fx.stats().lights,0);
  fx.setQuality('bad');assert.equal(fx.level,0);
});

test('puffs grow and fade in alpha, decals follow hit normals, and tracer heads travel',()=>{
  const fx=new ShooterFx(new T.Scene());fx.setQuality(2);fx.emit(event('impact'));fx.emit(event('tracer'));fx.update(0);
  const size=fx.puffs.sizes[0],alpha=fx.puffs.alpha[0],head=fx.tracerPositions[4];fx.update(.05);
  assert.ok(fx.puffs.sizes[0]>size);assert.ok(fx.puffs.alpha[0]<alpha);assert.ok(fx.tracerPositions[4]>head);
  const matrix=new T.Matrix4();fx.decals.getMatrixAt(0,matrix);const normal=new T.Vector3(0,0,1).transformDirection(matrix);
  assert.ok(normal.distanceTo(new T.Vector3(0,1,0))<1e-6);assert.ok(matrix.elements[13]>2);
  fx.emit(event('blood'));fx.update(0);assert.ok(fx.puffs.alive>0);
  assert.match(fx.puffs.points.material.vertexShader,/logdepthbuf_vertex/);
  assert.match(fx.puffs.points.material.vertexShader,/#include <common>/);
  assert.match(fx.decals.material.fragmentShader,/logdepthbuf_fragment/);
});

test('first-person flash survives one slow frame and only balanced/ultra enable its light',()=>{
  const f=Object.create(FirstPerson.prototype);
  Object.assign(f,{fx:new ShooterFx(new T.Scene()),renderer:{domElement:{height:900}},hud:{update(){},drawMinimap(){}},look:{yaw:0},speed:0,phase:0,recoil:0,flash:.05,flashFresh:true,weapon:new T.Group(),rest:new T.Vector3(.16,-.15,-.32),muzzleFlash:new T.Sprite(),muzzleLight:new T.PointLight(),reloadRemaining:0,reloadTotal:0});
  f.frame(.1,[],0);assert.equal(f.muzzleFlash.visible,true);assert.equal(f.muzzleLight.visible,false);
  f.frame(.1,[],0);assert.equal(f.muzzleFlash.visible,false);
  f.flash=.05;f.flashFresh=true;f.frame(.1,[],2);assert.equal(f.muzzleLight.visible,true);assert.ok(f.muzzleLight.intensity>0);
});

test('cached world tracer origin projects onto the overlay muzzle at hip and aim FOV',()=>{
  const f=Object.create(FirstPerson.prototype);
  Object.assign(f,{model:new T.Group(),vmCamera:new T.PerspectiveCamera(58,1.6,.01,10),camera:new T.PerspectiveCamera(75,1.6,.05,600),muzzlePoint:new T.Vector3(),muzzleWorld:[0,0,0]});
  f.model.position.set(.16,-.15,-.32);f.model.userData.muzzle=new T.Vector3(0,.02,-.2);
  f.camera.position.set(-2688,31.65,-2688);f.camera.rotation.set(-.3,.15,0);f.camera.updateMatrixWorld();f.vmCamera.updateMatrixWorld();
  const tip=f.model.localToWorld(f.model.userData.muzzle.clone()).project(f.vmCamera);
  for(const fov of [75,53]){
    f.camera.fov=fov;f.camera.updateProjectionMatrix();const output=f.worldMuzzle();assert.equal(output,f.muzzleWorld);
    const world=new T.Vector3().fromArray(output).project(f.camera);assert.ok(Math.abs(tip.x-world.x)<1e-8);assert.ok(Math.abs(tip.y-world.y)<1e-8);
  }
  f.model=null;assert.equal(f.worldMuzzle(),null);
});
