import test from 'node:test';
import assert from 'node:assert/strict';
import {TerrainOcclusion} from '../crates/engine-web/player/occlusion.js';
import {worldConfig} from '../crates/engine-web/player/streaming.js';
const ridge=(x,z)=>x>40 && x<60 ? 20 : 0;
const eye={x:0,y:5,z:0};
const box=(x,y=5)=>({min:{x,y:0,z:-2},max:{x:x+4,y,z:2}});
test('ridge hides only after two results, and visible results restore immediately',()=>{
  const c=new TerrainOcclusion(ridge);
  c.beginFrame(); assert.equal(c.occluded('behind',eye,box(100)),false);
  c.beginFrame(); assert.equal(c.occluded('behind',eye,box(100)),true);
  assert.equal(c.occluded('front',eye,box(20)),false);
  assert.equal(c.test(eye,box(100,100)),false);
  assert.equal(c.test({x:0,y:100,z:0},box(100)),false);
  c.beginFrame(); assert.equal(c.occluded('behind',{x:0,y:100,z:0},box(100)),false);
});
test('near boxes, underground eyes, empty or unknown terrain and disabled mode stay visible',()=>{
  const c=new TerrainOcclusion(ridge);
  assert.equal(c.test(eye,box(-1)),false);
  assert.equal(c.test({x:50,y:5,z:0},box(100)),false);
  assert.equal(c.test(eye,null),false);
  assert.equal(new TerrainOcclusion(()=>NaN).test(eye,box(100)),false);
  assert.equal(new TerrainOcclusion(()=>0).test(eye,box(100)),false);
  const disabled=new TerrainOcclusion(ridge,{enabled:false});
  for(let i=0;i<4;i++) {disabled.beginFrame();assert.equal(disabled.occluded('x',eye,box(100)),false);assert.equal(disabled.tests,0);}
});
test('round robin respects shared budget without starving late keys; stale entries expire',()=>{
  const c=new TerrainOcclusion(ridge,{budget:2});
  for(let frame=0;frame<9;frame++) {
    c.beginFrame();
    for(let i=0;i<5;i++) c.occluded(i,eye,box(100+i));
    assert.ok(c.tests<=2);
  }
  assert.ok([...c.cache.values()].every(item=>item.hidden));
  for(let i=0;i<122;i++) c.beginFrame();
  assert.equal(c.cache.size,0);
});
test('world config validates occlusion flags and nonnegative metre margin',()=>{
  const config=value=>worldConfig([{tags:['player'],props:{ai_world:value}}]);
  assert.equal(config({}).occlusion,true); assert.equal(config({}).occlusion_margin,2);
  assert.equal(config({occlusion:false,occlusion_margin:0}).occlusion,false);
  for(const value of [{occlusion:1},{occlusion_margin:-1},{occlusion_margin:Infinity},{occlusion_margin:'2'}]) assert.throws(()=>config(value),RangeError);
});
