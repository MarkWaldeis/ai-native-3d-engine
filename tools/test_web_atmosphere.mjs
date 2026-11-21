import test from 'node:test';
import assert from 'node:assert/strict';
import {registerHooks} from 'node:module';
import {graphicsConfig} from '../crates/engine-web/player/graphics-settings.js';
import {foliageExclusion,foliagePlacements,foliageSettings} from '../crates/engine-web/player/nature-settings.js';
registerHooks({resolve(s,c,next){return next(s==='three'?new URL('../crates/engine-web/player/vendor/three.module.js',import.meta.url).href:s,c);}});
const T=await import('three');
const {Atmosphere,valleyDensity}=await import('../crates/engine-web/player/atmosphere.js');
const {NatureVisuals}=await import('../crates/engine-web/player/nature.js');

test('atmosphere is opt-in, bounded and shadow tiers preserve scalar authoring',()=>{
  assert.equal(graphicsConfig({}).haze,false);assert.equal(graphicsConfig({}).clouds,false);
  assert.equal(graphicsConfig({haze:'bad',clouds:false}).haze,false);
  const config=graphicsConfig({haze:{density:1,falloff:-1},clouds:{coverage:2},foliage:{trees:0},shadow_extent:{ultra:144},shadow_map_size:{ultra:4096}});
  assert.equal(config.haze.density,.02);assert.equal(config.haze.falloff,.001);assert.equal(config.clouds.coverage,1);
  assert.equal(config.foliage.trees,0);assert.deepEqual(config.shadowExtents,[24,24,144]);assert.deepEqual(config.shadowMapSizes,[512,1024,4096]);
  assert.deepEqual(graphicsConfig({shadow_extent:80}).shadowExtents,[80,80,80]);
  assert.equal(valleyDensity(NaN,18,.045),0);assert.equal(valleyDensity(0,18,.045),1);assert.ok(valleyDensity(80,18,.045)<.1);
});

test('road exclusion handles rotated boxes, negative coordinates and empty inputs',()=>{
  const road={kind:'box',tags:['road'],transform:{position:[-20,0,30],rotation_deg:[0,90,0]},size:[6,1,40]};
  const exclude=foliageExclusion([road,{kind:'box',tags:['road']},{}]);
  assert.equal(exclude(-35,30),true);assert.equal(exclude(-20,45),false);assert.equal(exclude(0,0),false);
  assert.equal(foliageExclusion([])(0,0),false);
  assert.deepEqual(foliagePlacements(0,0,{exclude:()=>true}),[]);
  const placements=foliagePlacements(0,0,{trees:0});assert.ok(placements.length);assert.ok(placements.every(p=>!p.tree));
  assert.ok(foliageSettings({},2).distance>=100);assert.equal(foliageSettings({},0).distance,24);
});

test('haze composes material hooks, keeps program identities and uses a cheap low path',()=>{
  const scene=new T.Scene(),config=graphicsConfig({haze:true,clouds:true}),fx=new Atmosphere(scene,config,2);
  const m=new T.MeshStandardMaterial();let calls=0;
  m.onBeforeCompile=()=>{calls++;};const key=m.customProgramCacheKey();
  fx.patch(m);const shader={uniforms:{},vertexShader:'#include <fog_vertex>',fragmentShader:'#include <fog_fragment>'};m.onBeforeCompile(shader);
  assert.equal(calls,1);assert.equal(m.customProgramCacheKey(),key+'-valley-haze-v1');
  assert.ok(shader.fragmentShader.includes('hazeLevel<0.5'));assert.ok(shader.fragmentShader.includes('hazeHorizon+hazeSun'));
  const low=shader.fragmentShader.split('}else{')[0];assert.ok(low.includes('vFogDepth*hazeDensity'));assert.ok(T.ShaderChunk.fog_pars_fragment.includes('varying float vFogDepth'));assert.ok(!low.includes('exp('));assert.ok(!low.includes('length('));
  assert.ok(shader.vertexShader.includes('instanceMatrix*hazePosition'));
  fx.setQuality(0);assert.equal(fx.clouds.visible,false);assert.equal(shader.uniforms.hazeLevel.value,0);
  fx.setQuality(1);assert.equal(fx.clouds.visible,true);fx.update(1);assert.equal(fx.clouds.material.uniforms.cloudTime.value,1);
  const dynamic=new T.MeshStandardMaterial();let variant=0;dynamic.customProgramCacheKey=()=>`water-${variant}`;fx.patch(dynamic);variant=1;assert.equal(dynamic.customProgramCacheKey(),'water-1-valley-haze-v1');
  const plain=new T.MeshBasicMaterial({fog:false}),original=plain.onBeforeCompile;fx.patch(plain);assert.equal(plain.onBeforeCompile,original);
  fx.dispose();assert.equal(scene.children.length,0);
});

test('ground cover is short, tapered, colored, varied and has a cheaper far ring',()=>{
  globalThis.window={};const scene=new T.Scene(),fx=new NatureVisuals(scene,graphicsConfig({foliage:{trees:0}}),2);
  fx.setHeightSampler(()=>2);fx.tile(0,0);fx.tile(5,0,true);
  const grass=fx.geometries[0],p=grass.attributes.position;
  assert.ok(Math.max(...Array.from({length:p.count},(_,i)=>p.getY(i)))<.4);assert.equal(grass.attributes.color.count,p.count);
  assert.ok(fx.tiles.get('0,0').children.every(m=>m.geometry!==fx.geometries[1]&&m.geometry!==fx.geometries[2]));
  const far=fx.tiles.get('5,0').children;assert.equal(far.length,1);assert.equal(far[0].geometry,fx.geometries[5]);
  fx.setExclusions([{kind:'box',tags:['road'],transform:{position:[8,0,8]},size:[100,1,100]}]);fx.tile(0,0);assert.equal(fx.tiles.get('0,0').children.length,0);
  fx.dispose();
});
