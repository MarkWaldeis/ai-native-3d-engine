import test from 'node:test';
import assert from 'node:assert/strict';
import {registerHooks} from 'node:module';
import {sunDirection,daylightDefaults,foliageSettings,foliagePlacements,adaptQuality,waterDepth,bakeWaterDepth,replacesWater} from '../crates/engine-web/player/nature-settings.js';
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
  let center=null;const nanNeighbours=(x,z)=>{if(center&&((x===center[0]+0.5&&z===center[1])||(x===center[0]&&z===center[1]+0.5)))return NaN;center=[x,z];return 0;};
  assert.deepEqual(foliagePlacements(0,0,{sample:nanNeighbours}),[]);
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
  visuals.setHeightSampler(()=>2);camera.position.set(1000,3,-1000);visuals.update(0.016,camera);assert.equal(visuals.water.position.x,992);
  visuals.dispose();assert.equal(scene.children.length,0);assert.equal(window.__aiEngineVisuals,undefined);
  const empty=new NatureVisuals(scene,graphicsConfig({water:{level:0}}),1);empty.update(0.016,camera);assert.equal(empty.tiles.size,0);empty.dispose();
});

test('graphics colors accept normalized RGB and named sky defaults preserve authored overrides',()=>{
  const config=graphicsConfig({sky:'tropical',horizon:[1,0.5,0],water:{color:[0.18,0.32,0.28]}});
  assert.equal(config.sky,'#246a9b');assert.equal(config.horizon,'#ff8000');assert.equal(config.water.color,'#2e5247');
  assert.equal(graphicsConfig({sky_preset:'tropical',sky:'#123456',sun_color:'#abcdef'}).sky,'#123456');
  assert.equal(graphicsConfig({sky_preset:'tropical',sun_color:'#abcdef'}).sunColor,'#abcdef');
  for(const color of [[],[0,1],[0,1,2],[NaN,0,0],null,'invalid'])assert.equal(graphicsConfig({water:{color}}).water.color,'#126e61');
  assert.equal(graphicsConfig({sky:'unknown'}).sky,'#4d88cb');
});

test('water masks dry, invalid and submerged terrain in world coordinates',()=>{
  assert.equal(waterDepth(30,0.6),0);assert.equal(waterDepth(NaN,0.6),0);assert.equal(waterDepth(-3,0.6),3.6);assert.equal(waterDepth(-1000,0.6),64);
  const calls=[],data=bakeWaterDepth((x,z)=>{calls.push([x,z]);return x<10?-3:30;},0.6,10,-20,8,3);
  assert.deepEqual(calls[0],[6,-24]);assert.deepEqual(calls.at(-1),[14,-16]);
  const depth=(i)=>(data[i*4]*256+data[i*4+1])/65535*64;
  assert.ok(Math.abs(depth(0)-3.6)<0.001);assert.equal(depth(1),0);assert.equal(depth(2),0);
  assert.ok(bakeWaterDepth(()=>NaN,0,0,0,8,2).every((v,i)=>v===(i%4===3?255:0)));
  const plane={kind:'plane',tags:['water']};assert.equal(replacesWater(plane,{water:{level:0.6}}),true);
  assert.equal(replacesWater(plane,null),false);assert.equal(replacesWater({kind:'box',tags:['water']},{water:{}}),false);
  globalThis.window={};const scene=new T.Scene(),camera=new T.PerspectiveCamera(),visuals=new NatureVisuals(scene,graphicsConfig({water:{level:0.6}}),2);
  visuals.setHeightSampler(()=>30);visuals.update(0.1,camera);assert.equal(visuals.depthTexture.image.data[0],0);
  visuals.setHeightSampler(()=>-3);visuals.update(0.1,camera);assert.ok(visuals.depthTexture.image.data[0]>0);
  const high=visuals.depthTexture.image.width;visuals.setQuality(0);visuals.update(0.1,camera);assert.ok(visuals.depthTexture.image.width<high);assert.equal(visuals.waterLayers.value,0);
  const shader={uniforms:{},vertexShader:'#include <begin_vertex>',fragmentShader:'#include <color_fragment>\n#include <normal_fragment_maps>'};
  visuals.water.material.onBeforeCompile(shader);assert.ok(shader.fragmentShader.includes('if (depth < 0.02) discard'));assert.ok(!shader.fragmentShader.includes('waterLayers*0.45'));assert.equal(shader.uniforms.waterDepth.value,visuals.depthTexture);
  assert.throws(()=>bakeWaterDepth(()=>{throw new Error('sampler');},0,0,0,8,2),/sampler/);
  visuals.dispose();
});
