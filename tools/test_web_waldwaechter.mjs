import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {compare} from './waldwaechter/parity.mjs';

test('forest parity: normal, empty and malformed/divergent snapshots',()=>{
  compare({a:[1,2],b:true},{a:[1+1e-6,2],b:true});compare([],[]);
  for(const value of [null,{},[NaN],[2]])assert.throws(()=>compare(value,[1]));
  assert.throws(()=>compare({score:4},{score:5}));
});
test('forest traces cover victory, capture and persistent checkpoint score',()=>{
  const traces=JSON.parse(fs.readFileSync(new URL('./waldwaechter/traces.json',import.meta.url)));
  assert.equal(new Set(traces.map(t=>t.name)).size,3);
  assert.ok(traces.some(t=>t.expected.won===true&&t.expected.score===5));
  assert.ok(traces.some(t=>t.expected.Erwischt===1&&t.expected.score===0));
  assert.ok(traces.some(t=>t.expected.Erwischt===1&&t.expected.score===3&&t.expected._checkpoint===24));
});
