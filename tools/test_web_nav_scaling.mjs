import {test} from 'node:test';
import assert from 'node:assert/strict';
import {compareSample,validateFixture,runCases} from './nav_scaling.mjs';

const fixture=()=>({version:1,cases:[{name:'empty',agents:0,ticks:1,scene:{entities:[]},samples:[{tick:1,nav_agents:[]}]}]});
test('navigation parity checks numeric tolerance, routes and complete shapes',()=>{
  compareSample([{id:1,position:[1,2,3],agent:{path:[[0,0,0]],next:1,status:'moving'}}],[{id:1,position:[1.000001,2,3],agent:{path:[[0,0,0]],next:1,status:'moving'}}]);
  assert.throws(()=>compareSample([1],[1.1]),/numeric/);assert.throws(()=>compareSample([1],[]),/length/);assert.throws(()=>compareSample({status:'moving'},{status:'arrived'}),/parity/);assert.throws(()=>compareSample([NaN],[0]),/numeric/);
  assert.throws(()=>compareSample({id:1.000001},{id:1}),/numeric/);assert.throws(()=>compareSample({next:1.000001},{next:1}),/numeric/);
});
test('navigation workloads accept empty agents but reject empty/malformed fixtures',()=>{
  assert.equal(validateFixture(fixture()).cases.length,1);
  for(const value of [null,{version:2,cases:[]},{version:1,cases:[]}])assert.throws(()=>validateFixture(value));
  assert.throws(()=>validateFixture({version:1,cases:[null]}),/workload/);
  for(const edit of [c=>c.agents=129,c=>c.ticks=0,c=>c.samples=[],c=>c.samples[0].tick=2,c=>c.samples[0].nav_agents=[{}]]) {const f=fixture();edit(f.cases[0]);assert.throws(()=>validateFixture(f));}
});
test('navigation runtime and budget failures free the WASM instance',()=>{
  let freed=0;
  class WebGame {load(){} tick(){return JSON.stringify({tick:1,nav_agents:[]});}free(){freed++;}}
  assert.equal(runCases({WebGame},fixture())[0].samples,1);assert.equal(freed,1);
  let clock=0;assert.throws(()=>runCases({WebGame},fixture(),{maxTickMs:1,now:()=>clock+=2}),/exceeds budget/);assert.equal(freed,2);
  class Broken extends WebGame {load(){throw Error('load failed');}}
  assert.throws(()=>runCases({WebGame:Broken},fixture()),/load failed/);assert.equal(freed,3);
  assert.throws(()=>runCases({WebGame},fixture(),{maxTickMs:0}),/positive/);
});
