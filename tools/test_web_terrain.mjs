import test from 'node:test';
import assert from 'node:assert/strict';
import {createTerrain, decodeHeights, sampleTile, terrainLOD, TileQueue} from '../crates/engine-web/player/terrain.js';

test('scenes without terrain need no renderer, WASM calls or browser globals', async () => {
  assert.equal(await createTerrain(null,[],null,null,null,null),null);
  assert.equal(await createTerrain(null,[{props:{}}],null,null,null,null),null);
});

test('terrain quantization decodes LE heights and rejects malformed and empty fields', () => {
  const bytes = new Uint8Array(20), view = new DataView(bytes.buffer);
  bytes.set(new TextEncoder().encode('AITC')); view.setUint32(4,2,true); view.setUint32(8,2,true);
  view.setUint16(12,0,true); view.setUint16(14,65535,true); view.setUint16(16,32768,true);
  const values=decodeHeights(bytes,'AITC',[2,2],[-10,90]);
  assert.equal(values[0],-10); assert.equal(values[1],90); assert.ok(Math.abs(values[2]-40)<0.001);
  assert.throws(()=>decodeHeights(new Uint8Array(),'AITC',[2,2],[0,1]));
  assert.throws(()=>decodeHeights(bytes,'AITT',[2,2],[0,1]));
  assert.throws(()=>decodeHeights(bytes,'AITC',[3,2],[0,1]));
});
test('terrain sampling uses the native triangle diagonal and clamps to tile', () => {
  const heights=new Float32Array(16);
  heights[10]=10;
  const c={samples:2,tile_m:256};
  assert.equal(sampleTile(heights,c,25,25,6400+128,6400+128),0);
  assert.equal(sampleTile(heights,c,25,25,6400+192,6400+192),5);
  assert.equal(sampleTile(heights,c,25,25,9000,9000),10);
});
test('LOD hysteresis prevents oscillation and supports all four strides', () => {
  assert.equal(terrainLOD(0),1); assert.equal(terrainLOD(300),2); assert.equal(terrainLOD(600),4); assert.equal(terrainLOD(1100),8);
  assert.equal(terrainLOD(190,1),1); assert.equal(terrainLOD(160,2),2); assert.equal(terrainLOD(140,2),1);
});
test('queue limits concurrency, deduplicates, evicts LRU and retries errors', async () => {
  let active=0,max=0,calls=0;
  const releases=[];
  const queue=new TileQueue(async url=>{
    calls++; active++; max=Math.max(max,active);
    await new Promise(resolve=>releases.push(resolve)); active--;
    if(url==='bad') throw new Error('HTTP 404');
    return new Uint8Array([1,2]);
  },2,2);
  const a=queue.get('a'), duplicate=queue.get('a'), b=queue.get('b'), c=queue.get('c');
  assert.equal(a,duplicate); await Promise.resolve(); assert.equal(max,2);
  releases.shift()(); releases.shift()(); await Promise.all([a,b]);
  await new Promise(resolve=>setImmediate(resolve)); releases.shift()(); await c;
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(calls,3); assert.equal(queue.bytes,6); assert.equal(queue.cache.size,2); assert.equal(queue.cache.has('a'),false);
  assert.deepEqual(await queue.get('b'),new Uint8Array([1,2]));
  for(let i=0;i<2;i++) {const failure=queue.get('bad'); await Promise.resolve(); releases.shift()(); await assert.rejects(failure,/404/); await new Promise(resolve=>setImmediate(resolve));}
  assert.equal(queue.pending.size,0); assert.equal(queue.active,0);
});
