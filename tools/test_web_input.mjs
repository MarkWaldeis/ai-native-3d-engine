import assert from 'node:assert/strict';
import test from 'node:test';
import { ACTIONS, InputSystem, InputEdges, pushedBindings, parseInputConfig, radialDeadzone, sampleGamepads, describeBinding } from '../crates/engine-web/player/input.js';

const close = (a, b) => assert.ok(Math.abs(a - b) < 1e-9, `${a} != ${b}`);
const pad = (axes = [], pressed = []) => ({ axes, buttons: Array.from({ length: 17 }, (_, i) => ({ pressed: pressed.includes(i), value: pressed.includes(i) ? 1 : 0 })) });

test('gamepad modal/pause edges trigger once until released', () => {
  const edges = new InputEdges();
  assert.deepEqual([...edges.sample({})], []);
  assert.deepEqual([...edges.sample({ action: 1, pause: 1 })], ['action', 'pause']);
  assert.deepEqual([...edges.sample({ action: 1, pause: 1 })], []);
  edges.sample({ action: 0, pause: 0 });
  assert.deepEqual([...edges.sample({ action: 1 })], ['action']);
});

test('capture detects buttons and signed axes only beyond threshold, tolerating empty pads', () => {
  assert.deepEqual(pushedBindings([null, {}, { connected: false, axes: [1] }]), []);
  assert.deepEqual(pushedBindings([pad([0.6, -0.61, NaN], [2])]), [
    { type: 'button', index: 2 }, { type: 'axis', index: 1, dir: -1 },
  ]);
});

test('default keyboard map preserves arrows/WASD, Space and shooter keys', () => {
  const input = new InputSystem();
  for (const code of ['KeyW', 'ArrowUp']) assert.equal(input.sample({ keys: new Set([code]) }).move_z, -1);
  for (const [code, action] of [['Space', 'action'], ['KeyR', 'reload'], ['ShiftLeft', 'sprint'], ['ShiftRight', 'sprint'], ['Escape', 'pause']]) {
    assert.equal(input.sample({ keys: new Set([code]) }).actions[action], 1);
  }
  const diagonal = input.sample({ keys: new Set(['KeyD', 'KeyW']) });
  close(Math.hypot(diagonal.move_x, diagonal.move_z), 1);
  assert.equal(input.sample({ keys: new Set(['KeyD', 'KeyA']) }).move_x, 0);
});

test('standard buttons, d-pad and signed right-stick look', () => {
  const sampled = sampleGamepads([null, pad([0, -1, -0.575, 0], [0, 2, 7, 9, 10])]);
  assert.equal(sampled.move_z, -1); close(sampled.look_x, -0.5);
  for (const action of ['action', 'reload', 'fire', 'pause', 'sprint']) assert.equal(sampled.actions[action], 1);
  for (const [index, field, value] of [[12, 'move_z', -1], [13, 'move_z', 1], [14, 'move_x', -1], [15, 'move_x', 1]]) {
    assert.equal(sampleGamepads([pad([], [index])])[field], value);
  }
  assert.equal(sampleGamepads([{ buttons: [1] }]).actions.action, 1);
});

test('radial deadzone rescales continuously and preserves angle', () => {
  assert.deepEqual(radialDeadzone(0.1, 0.1), { x: 0, y: 0 });
  close(radialDeadzone(0.575, 0).x, 0.5);
  const diagonal = radialDeadzone(1, -1);
  close(Math.hypot(diagonal.x, diagonal.y), 1); close(diagonal.x, -diagonal.y);
  assert.deepEqual(radialDeadzone(NaN, undefined), { x: 0, y: 0 });
  assert.deepEqual(radialDeadzone(0, 0, 1), { x: 0, y: 0 });
});

test('multiple pads, keyboard and touch merge without exceeding unit movement', () => {
  const input = new InputSystem();
  const result = input.sample({ pads: [null, pad([1, 0]), pad([], [0, 13])], keys: new Set(['KeyW']), touch: { x: 1, z: -1 } });
  close(Math.hypot(result.move_x, result.move_z), 1);
  assert.equal(result.actions.action, 1);
  assert.ok(result.move_x > 0 && result.move_z < 0);
  assert.equal(input.sample({ pads: [pad([1, 0])], touch: { x: -1 } }).move_x, 0);
  assert.equal(sampleGamepads([{ ...pad([1, 0]), connected: false }]).move_x, 0);
});

test('empty and malformed device data stays finite and unpressed', () => {
  for (const pads of [undefined, null, [], [null], [{}], [pad([NaN, Infinity])]]) {
    const result = sampleGamepads(pads);
    assert.deepEqual(result.actions, Object.fromEntries(ACTIONS.map(action => [action, 0])));
    for (const field of ['move_x', 'move_z', 'look_x', 'look_y']) assert.equal(result[field], 0);
  }
  assert.equal(new InputSystem().sample({ touch: { x: NaN } }).move_x, 0);
});

test('rebind transfers conflicts, ignores invalid data, resets and persists copies', () => {
  const input = new InputSystem();
  const binding = { type: 'key', code: 'Space' };
  assert.equal(input.rebind('fire', binding), true);
  assert.equal(input.sample({ keys: new Set(['Space']) }).actions.action, 0);
  assert.equal(input.sample({ keys: new Set(['Space']) }).actions.fire, 1);
  input.rebind('fire', binding);
  assert.equal(input.config.bindings.fire.filter(b => b.code === 'Space').length, 1);
  for (const value of [null, {}, { type: 'axis', index: 0, dir: 0 }, { type: 'button', index: -1 }]) assert.equal(input.rebind('fire', value), false);
  assert.equal(input.rebind('unknown', binding), false);
  const restored = new InputSystem(JSON.stringify(input));
  assert.deepEqual(restored.toJSON(), input.toJSON());
  const copy = restored.toJSON(); copy.bindings.fire.length = 0;
  assert.notDeepEqual(copy, restored.toJSON());
  input.reset(); assert.equal(input.sample({ keys: new Set(['Space']) }).actions.action, 1);
});

test('authored object/JSON overrides selected defaults and supports explicit unbinding', () => {
  const value = { bindings: { action: [], fire: [{ type: 'axis', index: 4, dir: -1 }] }, deadzone: 0.2, look_sensitivity: 2, invert_y: true };
  assert.deepEqual(parseInputConfig(value), parseInputConfig(JSON.stringify(value)));
  const input = new InputSystem(value);
  assert.equal(input.warnings.length, 0);
  assert.equal(input.sample({ keys: new Set(['Space']) }).actions.action, 0);
  const result = input.sampleGamepads([pad([0, 0, 0, 0.6, -1])]);
  close(result.look_y, -1); assert.equal(result.actions.fire, 1);
  assert.equal(input.config.bindings.move_left[0].code, 'KeyA');
});

test('invalid authored values retain defaults and report warnings', () => {
  for (const value of ['{', 'null', null, [], 42]) {
    const parsed = parseInputConfig(value);
    assert.deepEqual(parsed.config, parseInputConfig().config); assert.ok(parsed.warnings.length);
  }
  const parsed = parseInputConfig({ bindings: { unknown: [], action: [null], fire: 'bad' }, deadzone: 1, look_sensitivity: -1, invert_y: 'true' });
  assert.equal(parsed.warnings.length, 6);
  assert.deepEqual(parsed.config, parseInputConfig().config);
  assert.ok(parseInputConfig({ bindings: null }).warnings.length);
});

test('German binding labels cover keys, standard buttons, sticks and fallbacks', () => {
  assert.equal(describeBinding({ type: 'key', code: 'KeyW' }), 'Taste W');
  assert.equal(describeBinding({ type: 'button', index: 0 }), 'Gamepad A');
  assert.equal(describeBinding({ type: 'axis', index: 1, dir: -1 }), 'Linker Stick ↑');
  assert.equal(describeBinding({ type: 'axis', index: 2, dir: 1 }), 'Rechter Stick →');
  assert.equal(describeBinding({ type: 'button', index: 30 }), 'Gamepad Taste 30');
  assert.equal(describeBinding(null), 'Unbekannte Bindung');
});
