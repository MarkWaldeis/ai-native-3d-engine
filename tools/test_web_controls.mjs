import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import { ActionLatch, viewMovement, followDistance, routeFrame, bindActivation } from '../crates/engine-web/player/controls.js';

test('every Pages document links the bundled portable favicon',()=>{
  assert.match(readFileSync(new URL('../crates/engine-web/favicon.svg',import.meta.url),'utf8'),/^<svg/);
  for(const file of ['player/index.html','landing.html'])assert.match(readFileSync(new URL('../crates/engine-web/'+file,import.meta.url),'utf8'),/<link\b[^>]*rel="icon"[^>]*href="\.\/favicon.svg"/);
});

test('each activation family starts without a compatibility click and hidden menus ignore duplicates',async()=>{
  for(const type of ['pointerup','touchend','click']){
    const handlers=new Map(),button={disabled:false,addEventListener:(k,f)=>handlers.set(k,f)};let visible=true,calls=0;
    bindActivation(button,()=>{calls++;visible=false;},()=>visible);
    await handlers.get(type)({type,button:0,preventDefault(){}});
    for(const other of ['pointerup','touchend','click'])await handlers.get(other)({type:other,button:0,preventDefault(){}});
    assert.equal(calls,1,type);
  }
});

test('start activation accepts first pointer/touch/click, fences duplicates and recovers from errors',async()=>{
  const listeners=new Map(),button={disabled:false,addEventListener:(k,f)=>listeners.set(k,f),removeEventListener:k=>listeners.delete(k),getBoundingClientRect:()=>({left:0,top:0,right:100,bottom:100})};
  let calls=0,visible=true,release;
  const unbind=bindActivation(button,async()=>{calls++;await new Promise(resolve=>{release=resolve;});visible=false;},()=>visible);
  const event=(type,extra={})=>({type,button:0,preventDefault(){},...extra});
  const first=listeners.get('pointerup')(event('pointerup'));
  await listeners.get('touchend')(event('touchend'));await listeners.get('click')(event('click'));assert.equal(calls,1);
  release();await first;await listeners.get('click')(event('click'));assert.equal(calls,1);
  visible=true;button.disabled=true;await listeners.get('touchend')(event('touchend'));assert.equal(calls,1);button.disabled=false;
  await listeners.get('pointerup')(event('pointerup',{button:2}));await listeners.get('touchend')(event('touchend',{changedTouches:[{clientX:101,clientY:10}]}));assert.equal(calls,1);
  unbind();assert.equal(listeners.size,0);
  let fail=true;bindActivation(button,()=>{calls++;if(fail)throw Error('activation failed');},()=>true);
  await assert.rejects(listeners.get('touchend')(event('touchend')),/activation failed/);fail=false;await listeners.get('click')(event('click'));assert.equal(calls,3);
});

test('short tap survives release before simulation tick, but only once', () => {
  const action = new ActionLatch();
  assert.equal(action.sample(), 0);
  action.press(1); action.release(1); action.release(1);
  assert.equal(action.sample(), 1); assert.equal(action.sample(), 0);
});
test('multi-touch release owns only its pointer; cancellation and reset clear input', () => {
  const action = new ActionLatch();
  action.press(1); action.press(2); assert.equal(action.sample(), 1);
  action.release(1); assert.equal(action.sample(), 1);
  action.release(2); assert.equal(action.sample(), 0);
  action.press(3); action.release(3, true); assert.equal(action.sample(), 0);
  action.press('keyboard'); action.reset(); assert.equal(action.sample(), 0);
});
test('view-relative motion is normalized and follows authored camera yaw', () => {
  assert.deepEqual(viewMovement(0, 0, [7, 6, 9]), {x:0,z:0});
  assert.deepEqual(viewMovement(1, 0, [0, 6, 9]), {x:1,z:0});
  assert.deepEqual(viewMovement(1, 0, [9, 6, 0]), {x:0,z:-1});
  const diagonal = viewMovement(1, 1, [7, 6, 9]);
  assert.ok(Math.abs(Math.hypot(diagonal.x, diagonal.z) - 1) < 1e-12);
});
test('portrait frames a landing island, closer than original 20m offset; invalid aspect fails', () => {
  for (const aspect of [390/844,844/390,820/1180,16/10]) {
    const distance = followDistance(aspect);
    assert.ok(distance < 14);
    assert.ok(2 * distance * Math.tan(55*Math.PI/360) * aspect >= 6.1 - 1e-12);
  }
  assert.throws(() => followDistance(0), RangeError);
});

test('route preview fits the complete route and model margin in tall projections', () => {
  const points = [[0,1,0],[3.5,1,0],[7,1,0],[11.5,1.5,0]];
  for (const aspect of [390/844,820/1180,1.6]) {
    const frame = routeFrame(points,aspect);
    const halfFov = Math.min(55*Math.PI/360,Math.atan(Math.tan(55*Math.PI/360)*aspect));
    for (const point of points) assert.ok(frame.distance*Math.sin(halfFov) >= Math.hypot(...point.map((v,i)=>v-frame.target[i]))+1.8);
  }
  assert.equal(routeFrame([],1),null);
  assert.throws(()=>routeFrame(points,0),RangeError);
  assert.throws(()=>routeFrame(points,1,180),RangeError);
  assert.throws(()=>routeFrame([[NaN,0,0]],1),RangeError);
});
