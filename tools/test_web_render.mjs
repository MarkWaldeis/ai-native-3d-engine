import test from 'node:test';
import assert from 'node:assert/strict';
import {renderConfig,PRESETS,sunDirection,foliagePlacements,QualityGovernor} from '../crates/engine-web/player/render-config.js';
const player = value => [{tags:['player'],props:{ai_render:value}}];
test('render opt-in, JSON, URL override and mobile cap',()=>{
  assert.equal(renderConfig([]),null);
  assert.equal(renderConfig([], '?quality=invalid'),null);
  assert.equal(renderConfig(player('{"preset":"ultra"}')).preset,'ultra');
  assert.equal(renderConfig(player({preset:'ultra'}),'',true).preset,'medium');
  assert.equal(renderConfig(player({preset:'low'}),'?quality=ultra',true).preset,'ultra');
  assert.equal(renderConfig([],'?quality=low').auto_quality,false);
  assert.equal(renderConfig([],'?quality=auto').auto_quality,true);
  assert.throws(()=>renderConfig(player('bad')),SyntaxError);
  assert.throws(()=>renderConfig(player({preset:'bad'})),RangeError);
});
test('presets progressively increase detail',()=>{
  assert.equal(PRESETS.low.cascades,0);assert.equal(PRESETS.medium.antialias,'fxaa');
  assert.equal(PRESETS.high.cascades,3);assert.equal(PRESETS.ultra.cascades,4);
  assert.ok(PRESETS.ultra.ao);assert.ok(PRESETS.ultra.foliage>PRESETS.medium.foliage);
});
test('ai_graphics alias enables defaults and ai_render wins the merged configuration',()=>{
  assert.equal(renderConfig([{tags:['player'],props:{ai_graphics:{}}}]).preset,'high');
  const config=renderConfig([{tags:['player'],props:{ai_graphics:'{"preset":"ultra","exposure":0.7}',ai_render:'{"preset":"low"}'}}]);
  assert.equal(config.preset,'low');assert.equal(config.exposure,0.7);
  assert.throws(()=>renderConfig([{tags:['player'],props:{ai_graphics:'bad'}}]),SyntaxError);
});
test('sun direction is normalized, periodic and above horizon by day',()=>{
  assert.ok(sunDirection(12)[1]>0.9);assert.ok(sunDirection(0)[1]<0);
  assert.deepEqual(sunDirection(39),sunDirection(15));
  assert.ok(Math.abs(Math.hypot(...sunDirection(15))-1)<1e-12);
});
test('foliage deterministic tiles, empty density, water and slopes',()=>{
  assert.deepEqual(foliagePlacements(2,-3),foliagePlacements(2,-3));
  assert.notDeepEqual(foliagePlacements(2,-3),foliagePlacements(3,-3));
  assert.deepEqual(foliagePlacements(0,0,{density:0}),[]);
  assert.deepEqual(foliagePlacements(0,0,{water:1}),[]);
  assert.deepEqual(foliagePlacements(0,0,{sample:x=>x*2}),[]);
  assert.deepEqual(foliagePlacements(0,0,{sample:()=>NaN}),[]);
});
test('governor steps down, recovers, respects cap and fixed quality',()=>{
  const g=new QualityGovernor('high');
  for(let i=0;i<70;i++)g.update(1/25);
  assert.equal(g.level,9);assert.ok(g.settings().pixelRatio<PRESETS.high.pixelRatio);
  for(let i=0;i<1000;i++)g.update(1/60);
  assert.equal(g.level,10);
  const fixed=new QualityGovernor('ultra',false);for(let i=0;i<300;i++)fixed.update(0.1);
  assert.equal(fixed.level,15);assert.equal(g.update(0),false);
});
