import test from 'node:test';
import assert from 'node:assert/strict';
import {registerHooks} from 'node:module';
import {sunDirection,daylightDefaults,foliageSettings,foliagePlacements,adaptQuality} from '../crates/engine-web/player/nature-settings.js';
import {graphicsConfig} from '../crates/engine-web/player/graphics-settings.js';

registerHooks({resolve(specifier,context,nextResolve){return nextResolve(specifier==='three'?new URL('../crates/engine-web/player/vendor/three.module.js',import.meta.url).href:specifier,context);}});
const T=await import('three');
const {NatureVisuals}=await import('../crates/engine-web/player/nature.js');

test('daylight is periodic, normalized, optional, and explicit sky/sun values win',()=>{
  assert.deepEqual(daylightDefaults(undefined),{});
  assert.deepEqual(sunDirection(17),sunDirection(41));
  assert.ok(Math.abs(Math.hypot(...sunDirection(17))-1)<1e-12);
  assert.ok(sunDirection(12)[1]>0.9);assert.ok(sunDirection(0)[1]<0);
  const config=graphicsConfig({time_of_day:17,sky:'#123456',horizon:'#234567',sun_color:'#345678',sun_direction:[0,1,0],sun_intensity:1},'ultra');
  assert.equal(config.sky,'#123456');assert.equal(config.horizon,'#234567');assert.equal(config.sunColor,'#345678');assert.deepEqual(config.sunDirection,[0,1,0]);assert.equal(config.sunIntensity,1);
  assert.notDeepEqual(graphicsConfig({time_of_day:17}).sunDirection,graphicsConfig({}).sunDirection);
});
test('deprecated alias merges over graphics, quality aliases stay fixed, config is bounded',()=>{
  assert.equal(graphicsConfig(undefined,undefined,undefined,{}).quality,-1);
  const config=graphicsConfig('{"time_of_day":17,"exposure":0.8,"foliage":true}', 'high', undefined,'{"exposure":0.6,"water":{"level":-1,"color":"#123456"}}');
  assert.equal(config.exposure,0.6);assert.ok(config.foliage);assert.equal(config.water.level,-1);assert.equal(config.water.color,'#123456');
  assert.equal(config.quality,1);assert.equal(graphicsConfig({},'medium').quality,1);
  assert.equal(graphicsConfig(undefined,'legacy',undefined,{}),null);
  assert.equal(graphicsConfig({}).foliage,false);assert.equal(graphicsConfig({}).water,false);
  assert.equal(graphicsConfig({foliage:{density:100},fog_density:-1}).foliage.density,4);
  assert.equal(graphicsConfig({fog_density:-1}).fogDensity,0);
  assert.equal(graphicsConfig('bad',undefined,undefined,'[]'),null);
});
test('placement is deterministic across negative tiles and rejects water, slopes, empty and invalid heights',()=>{
  assert.deepEqual(foliagePlacements(-2,3),foliagePlacements(-2,3));assert.notDeepEqual(foliagePlacements(-2,3),foliagePlacements(2,3));
  for(const options of [{density:0},{water:1},{sample:x=>x*2},{sample:()=>NaN}])assert.deepEqual(foliagePlacements(0,0,options),[]);
  assert.ok(foliagePlacements(0,0,{water:-1,sample:()=>1}).length>0);
  assert.ok(foliageSettings({},0).density<foliageSettings({},2).density);
  assert.equal(foliageSettings({distance:128},0).distance,24);
});
test('existing quality state has asymmetric recovery, hysteresis and a start cap',()=>{
  const state={level:2,startLevel:2,slow:0,fast:0};
  assert.equal(adaptQuality(state,20,3),false);assert.equal(adaptQuality(state,20,3),true);assert.equal(state.level,1);
  assert.equal(adaptQuality(state,60,9),false);assert.equal(adaptQuality(state,50,3),false);assert.equal(state.fast,0);
  assert.equal(adaptQuality(state,60,12),true);assert.equal(state.level,2);
  assert.equal(adaptQuality(state,60,30),false);assert.equal(state.level,2);
  state.level=0;assert.equal(adaptQuality(state,10,30),false);
});
test('nature follows quality, sampler invalidation, tile lifecycle, and disposes without owning a renderer',()=>{
  globalThis.window={};
  const scene=new T.Scene(),camera=new T.PerspectiveCamera();camera.position.set(0,3,0);
  const config=graphicsConfig({foliage:true,water:{level:-1}}),visuals=new NatureVisuals(scene,config,2);
  assert.equal(window.__aiEngineVisuals,visuals);
  const ground=new T.Mesh(new T.BoxGeometry(20,0.4,20),new T.MeshBasicMaterial());ground.userData.support=true;scene.add(ground);scene.updateMatrixWorld(true);visuals.grounds=[ground];
  assert.ok(Math.abs(visuals.groundHeight(0,0)-0.2)<1e-6);assert.equal(visuals.groundHeight(100,100),0);
  ground.removeFromParent();ground.geometry.dispose();ground.material.dispose();
  visuals.setHeightSampler(()=>2);visuals.update(0.016,camera);assert.equal(visuals.tiles.size,1);
  assert.ok([...visuals.tiles.values()][0].children.every(o=>o.isInstancedMesh));
  const repeat=visuals.normal.repeat.x;visuals.setQuality(0);assert.equal(visuals.tiles.size,0);assert.ok(visuals.normal.repeat.x<repeat);
  assert.throws(()=>visuals.setHeightSampler(null),TypeError);
  visuals.setHeightSampler(()=>NaN);visuals.update(0.016,camera);assert.ok([...visuals.tiles.values()][0].children.every(o=>o.count===0));
  visuals.setHeightSampler(()=>2);camera.position.set(1000,3,-1000);visuals.update(0.016,camera);assert.equal(visuals.water.position.x,1000);
  visuals.dispose();assert.equal(scene.children.length,0);assert.equal(window.__aiEngineVisuals,undefined);
  const empty=new NatureVisuals(scene,graphicsConfig({water:{level:0}}),1);empty.update(0.016,camera);assert.equal(empty.tiles.size,0);empty.dispose();
});
