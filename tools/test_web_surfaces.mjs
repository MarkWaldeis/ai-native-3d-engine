import test from 'node:test';
import assert from 'node:assert/strict';
import * as T from '../crates/engine-web/player/vendor/three.module.js';
import {terrainWeights,surfacePixels,surfaceTextures,terrainSurface,roadSurface} from '../crates/engine-web/player/surfaces.js';

test('river bed is mud, steep ground is rock and high flat ground is grass',()=>{
  assert.deepEqual(terrainWeights(-3,0),[0,0,0,1]);
  assert.deepEqual(terrainWeights(40,0.8),[0,0,1,0]);
  assert.deepEqual(terrainWeights(80,0),[1,0,0,0]);
  terrainWeights(101,0,100).forEach((v,i)=>assert.ok(Math.abs(v-terrainWeights(1,0,0)[i])<1e-12));
  for(const h of [-3,0.6,2,6,40])for(const s of [0,.1,.3,.8])for(const n of [0,.5,1]){
    const weights=terrainWeights(h,s,0.6,n);
    assert.ok(weights.every(v=>v>=0&&v<=1));assert.ok(Math.abs(weights.reduce((a,b)=>a+b,0)-1)<1e-12);
  }
  assert.notDeepEqual(terrainWeights(3,0,.6,0),terrainWeights(3,0,.6,1));
  assert.deepEqual(terrainWeights(NaN,0),[1,0,0,0]);
});

test('procedural atlas is deterministic with four varied layers and matching finite normals',()=>{
  const a=surfacePixels(),b=surfacePixels();assert.deepEqual(a,b);assert.equal(a.albedo.length,128*128*4);
  for(let y=0;y<a.width;y++)for(let x=0;x<a.width;x++){
    const i=(y*a.width+x)*4,normal=Array.from(a.normal.slice(i,i+3),v=>v/255*2-1);
    assert.ok(Math.abs(Math.hypot(...normal)-1)<.015);assert.equal(a.normal[i+3],255);assert.equal(a.albedo[i+3],255);
  }
  assert.notDeepEqual(a.albedo.slice(0,64*4),a.albedo.slice(64*4,128*4));
  assert.equal(surfaceTextures(T),surfaceTextures(T));
});

test('surface shaders support full and cheap quality without replacing geometry or other materials',()=>{
  for(const decorate of [terrainSurface,roadSurface]){
    const material=new T.MeshStandardMaterial();decorate(T,material);
    const shader={uniforms:{},vertexShader:'#include <begin_vertex>',fragmentShader:'#include <color_fragment>\n#include <normal_fragment_maps>'};
    material.onBeforeCompile(shader);
    for(const [quality,expected] of [['0',0],['1',1],['2',1]]){
      material.onBeforeRender({domElement:{dataset:{quality}}});assert.equal(shader.uniforms.surfaceDetail.value,expected);
    }
    assert.ok(shader.fragmentShader.includes('surfaceNoise'));assert.ok(shader.uniforms.surfaceAlbedo.value.isDataTexture);
    assert.ok(!shader.fragmentShader.includes('discard'));material.dispose();
  }
  const road=new T.MeshStandardMaterial({metalness:.5});roadSurface(T,road);assert.equal(road.metalness,0);assert.equal(road.roughness,1);
});
