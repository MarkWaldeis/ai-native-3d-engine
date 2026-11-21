import test from 'node:test';
import assert from 'node:assert/strict';
import { ActionLatch, viewMovement, followDistance, routeFrame } from '../crates/engine-web/player/controls.js';

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
