import test from 'node:test';
import assert from 'node:assert/strict';
import {registerHooks} from 'node:module';
import {graphicsConfig} from '../crates/engine-web/player/graphics-settings.js';
import {foliageExclusion,foliagePlacements,foliageSettings,junglePlacements,jungleSettings} from '../crates/engine-web/player/nature-settings.js';
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
  fx.patch(m);const shader={uniforms:{},vertexShader:'#include <begin_vertex>\n#include <fog_vertex>',fragmentShader:'#include <fog_fragment>'};m.onBeforeCompile(shader);
  assert.equal(calls,1);assert.equal(m.customProgramCacheKey(),key+'-valley-haze-v1');
  assert.ok(shader.fragmentShader.includes('hazeLevel<0.5'));assert.ok(shader.fragmentShader.includes('hazeHorizon+hazeSun'));
  const low=shader.fragmentShader.split('}else{')[0];assert.ok(low.includes('vFogDepth*hazeDensity'));assert.ok(T.ShaderChunk.fog_pars_fragment.includes('varying float vFogDepth'));assert.ok(!low.includes('exp('));assert.ok(!low.includes('length('));
  assert.ok(shader.vertexShader.includes('instanceMatrix*hazePosition'));
  fx.setQuality(0);assert.equal(fx.clouds.visible,false);assert.equal(shader.uniforms.hazeLevel.value,0);
  fx.setQuality(1);assert.equal(fx.clouds.visible,true);fx.update(1);assert.equal(fx.clouds.material.uniforms.cloudTime.value,1);
  const dynamic=new T.MeshStandardMaterial();let variant=0;dynamic.customProgramCacheKey=()=>`water-${variant}`;fx.patch(dynamic);variant=1;assert.equal(dynamic.customProgramCacheKey(),'water-1-valley-haze-v1');
  const plain=new T.MeshBasicMaterial({fog:false}),original=plain.onBeforeCompile;fx.patch(plain);assert.equal(plain.onBeforeCompile,original);
  const sprite=new T.SpriteMaterial();fx.patch(sprite);const spriteShader={uniforms:{},vertexShader:'#include <fog_vertex>',fragmentShader:'#include <fog_fragment>'};sprite.onBeforeCompile(spriteShader);
  assert.ok(!spriteShader.vertexShader.includes('hazePosition'));assert.ok(!spriteShader.fragmentShader.includes('vHazeWorld'));
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

test('jungle canopy placement is deterministic, bounded per tier and respects terrain and authored footprints',()=>{
  assert.equal(graphicsConfig({foliage:true}).foliage.jungle,false);
  assert.equal(graphicsConfig({foliage:{jungle:'true'}}).foliage.jungle,false);
  const building={kind:'gltf',tags:['building'],props:{web_mesh_bounds:[[-18,0,-2],[18,10,2]]},
    transform:{position:[-30,0,0],rotation_deg:[0,90,0],scale:[1,1,1]}};
  const tree={tags:['tree'],transform:{position:[0,0,0]}};
  const exclude=foliageExclusion([building,tree],true);
  assert.equal(exclude(-30,18),true);
  assert.equal(exclude(-20,0),false);
  assert.equal(exclude(4,4),true);
  assert.equal(foliageExclusion([building,tree])(0,0),false);
  const opts={...jungleSettings(2),sample:(x,z)=>z<0?0:5,water:0,exclude};
  const planted=junglePlacements(-15,8,opts);
  assert.deepEqual(planted,junglePlacements(-15,8,opts));
  assert.ok(planted.length>0&&planted.length<=opts.budget);
  assert.ok(planted.every(p=>p.z>=0&&!exclude(p.x,p.z)));
  assert.ok(planted.some(p=>p.variant===0)&&planted.some(p=>p.variant===1));
  assert.deepEqual(junglePlacements(0,0,{...opts,sample:x=>x*2}),[]);
  assert.deepEqual(junglePlacements(0,0,{...opts,sample:()=>NaN}),[]);
  assert.ok(junglePlacements(0,0,{...opts,...jungleSettings(0)}).every(p=>p.variant===0));
});

test('jungle canopy uses frustum-culled unshadowed batches and resets on terrain and tier changes',()=>{
  globalThis.window={};
  const scene=new T.Scene(),camera=new T.PerspectiveCamera();
  const fx=new NatureVisuals(scene,graphicsConfig({foliage:{jungle:true,trees:0}}),2);
  fx.update(0.016,camera);
  assert.equal(fx.stats().canopyInstances,0);
  let version=0, detailed=NaN;
  fx.setHeightSampler(()=>2,()=>detailed,()=>version);fx.update(.016,camera);
  assert.equal(fx.stats().canopyInstances,0);
  detailed=6;version++;fx.update(.016,camera);
  const high=fx.stats().canopyInstances;
  assert.ok(high>jungleSettings(1).budget&&high<=jungleSettings(2).budget);
  assert.equal(fx.canopyMeshes.length,8);
  assert.ok(fx.canopyMeshes.every(mesh=>mesh.isInstancedMesh&&!mesh.castShadow&&!mesh.receiveShadow));
  const facing=fx.canopyMeshes.map(mesh=>mesh.visible);
  assert.ok(facing.some(Boolean)&&facing.some(visible=>!visible));
  camera.rotation.y=Math.PI;camera.updateMatrixWorld();fx.update(.016,camera);
  assert.notDeepEqual(fx.canopyMeshes.map(mesh=>mesh.visible),facing);
  assert.equal(fx.stats().canopyInstances,high);
  assert.ok(fx.canopyMeshes.reduce((n,m)=>n+m.count*m.geometry.attributes.position.count/3,0)<=20020);
  const mat=fx.canopyMaterials[0],shader={uniforms:{},vertexShader:'#include <begin_vertex>'};
  mat.onBeforeCompile(shader);
  assert.equal(shader.uniforms.canopyViewer,fx.viewer);
  assert.match(shader.vertexShader,/smoothstep\(20\.,39\.,d\)/);
  fx.setQuality(0);assert.equal(fx.stats().canopyInstances,0);
  fx.update(.016,camera);
  assert.equal(fx.canopyMeshes.length,4);
  assert.ok(fx.stats().canopyInstances<=jungleSettings(0).budget);
  fx.dispose();
  assert.equal(scene.children.length,0);
});
