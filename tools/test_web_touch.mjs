import assert from 'node:assert/strict';
import test from 'node:test';
import { parseTouchConfig, layoutTouch, TouchState, hapticCue, pulseHaptics } from '../crates/engine-web/player/touch.js';
import { InputSystem, parseInputConfig } from '../crates/engine-web/player/input.js';

test('touch config validates object/JSON, defaults and malformed values without throwing', () => {
  const source = { buttons: [{ action: 'fire', label: 'Feuer', icon: '●', anchor: 'right', size: 'lg', hold: false }], joystick: { side: 'right', size: 'lg' }, hide_default_action: true };
  const parsed = parseTouchConfig(source);
  assert.deepEqual(parsed, parseTouchConfig(JSON.stringify(source))); assert.deepEqual(parsed.warnings, []);
  assert.equal(parsed.config.joystick.size, 150); assert.equal(source.joystick.size, 'lg');
  assert.deepEqual(parseTouchConfig().warnings, []);
  for (const value of ['{', 'null', [], false, 4]) assert.ok(parseTouchConfig(value).warnings.length);
  const invalid = parseTouchConfig({ buttons: [null, { action: 'unknown' }, { action: 'sprint', label: 3, icon: {}, anchor: 'bad', size: 'xl', hold: 1 }], joystick: { side: 'bad', size: -1 }, hide_default_action: 1 });
  assert.equal(invalid.config.buttons.length, 1); assert.equal(invalid.config.buttons[0].hold, true); assert.equal(invalid.config.joystick.side, 'left'); assert.ok(invalid.warnings.length >= 9);
  const bad = {}; Object.defineProperty(bad, 'buttons', { get() { throw new Error(); } });
  assert.doesNotThrow(() => parseTouchConfig(bad)); assert.ok(parseTouchConfig(bad).warnings.length);
  assert.ok(parseTouchConfig({ buttons: Array.from({ length: 13 }, () => ({ action: 'action' })) }).warnings.length);
});

test('touch layout fits all anchors, safe areas and both joystick sides without overlapping FPS/default controls', () => {
  for (const [width, height] of [[390, 844], [844, 390]]) for (const side of ['left', 'right']) for (const fps of [false, true]) {
    const config = parseTouchConfig({ buttons: ['bottom-left', 'bottom-right', 'right', 'top-right'].map(anchor => ({ action: 'sprint', anchor, size: 'lg' })), joystick: { side, size: 150 } }).config;
    const boxes = layoutTouch(config, { width, height, fps, insets: { left: 20, right: 20, top: 20, bottom: 30 } });
    assert.equal(boxes.length, 6 + (fps ? 2 : 0));
    for (const [i, a] of boxes.entries()) {
      assert.ok(a.x >= 20 && a.y >= 20 && a.x + a.width <= width - 20 && a.y + a.height <= height - 30);
      for (const b of boxes.slice(i + 1)) assert.ok(a.x + a.width <= b.x || b.x + b.width <= a.x || a.y + a.height <= b.y || b.y + b.height <= a.y);
    }
    assert.equal(boxes[0].x > width / 2, side === 'right');
  }
  const config = parseTouchConfig({ hide_default_action: true }).config;
  assert.deepEqual(layoutTouch(config, { width: 390, height: 844 }).map(p => p.id), ['joystick']);
});

test('look touch actions are hold-only so their integration does not depend on queued impulse frame duration', () => {
  const parsed = parseTouchConfig({ buttons: [{ action: 'look_x', hold: false }, { action: 'look_y', hold: false }, { action: 'fire', hold: false }] });
  assert.equal(parsed.warnings.length, 2); assert.match(parsed.warnings[0], /hold:true/);
  assert.deepEqual(parsed.config.buttons.map(b => b.hold), [true, true, false]);
  const state = new TouchState(parsed.config.buttons);
  state.press(0, 1); state.press(1, 2); state.consume();
  assert.deepEqual(state.sample(), { look_x: 1, look_y: 1 });
  state.release(1); state.release(2); assert.deepEqual(state.sample(), {});
  assert.deepEqual(parseTouchConfig({ buttons: [{ action: 'look_x' }, { action: 'look_y', hold: true }] }).warnings, []);
});

test('independent touch pointers retain held actions, queue short taps until a tick and cancel/reset safely', () => {
  const state = new TouchState([{ action: 'fire', hold: true }, { action: 'sprint', hold: true }, { action: 'reload', hold: false }]);
  assert.deepEqual(state.sample(), {}); assert.equal(state.press(9, 1), false);
  state.press(0, 1); state.press(1, 2); state.press(2, 3);
  assert.deepEqual(state.sample(), { fire: 1, sprint: 1, reload: 1 });
  state.consume(); assert.deepEqual(state.sample(), { fire: 1, sprint: 1 });
  state.release(1); assert.deepEqual(state.sample(), { sprint: 1 });
  state.release(2); state.release(3);
  state.press(0, 4); state.release(4); assert.deepEqual(state.sample(), { fire: 1 }, 'tap survives until a simulation sample');
  state.consume(); assert.deepEqual(state.sample(), {});
  state.press(0, 5); state.press(0, 6); state.consume(); state.release(5, true);
  assert.deepEqual(state.sample(), { fire: 1 }); state.release(6, true); assert.deepEqual(state.sample(), {});
  state.press(1, 7); state.reset(); assert.deepEqual(state.sample(), {});
});

test('touch placement reserves radar, health, ammo, compass, HUD widgets and footer, including joystick label space', () => {
  const config = parseTouchConfig({ buttons: ['bottom-right', 'bottom-left', 'right', 'top-right'].map(anchor => ({ action: 'fire', anchor })), joystick: { side: 'left', size: 130 } }).config;
  const obstacles = [
    { x: 20, y: 96, width: 110, height: 110 },
    { x: 320, y: 316, width: 204, height: 28 },
    { x: 700, y: 96, width: 120, height: 48 },
    { x: 212, y: 92, width: 420, height: 58 },
    { x: 152, y: 160, width: 130, height: 44 },
    { x: 24, y: 354, width: 796, height: 28 },
  ];
  const copy = structuredClone(obstacles), boxes = layoutTouch(config, { width: 844, height: 390, fps: true, obstacles });
  assert.equal(boxes.length, 8);
  for (const box of boxes) for (const b of obstacles) {
    const height = box.height + (box.id === 'joystick' ? 22 : 0);
    assert.ok(box.x + box.width <= b.x || b.x + b.width <= box.x || box.y + height <= b.y || b.y + b.height <= box.y);
  }
  assert.deepEqual(obstacles, copy, 'reserved rectangles are not mutated');
  const blocked = layoutTouch(config, { width: 844, height: 390, obstacles: [{ x: 0, y: 0, width: 844, height: 390 }] });
  assert.equal(blocked.length, 1); assert.equal(blocked[0].id, 'joystick'); assert.equal(blocked[0].width, 80);
  assert.deepEqual(layoutTouch(config, { width: 844, height: 390, obstacles: [{ x: NaN }] }), layoutTouch(config, { width: 844, height: 390 }));
});

test('portrait 320x568 to landscape 568x320 retries an 80px joystick and never retains an off-screen position', () => {
  const config = parseTouchConfig({ joystick: { size: 130 } }).config;
  const portrait = layoutTouch(config, { width: 320, height: 568, fps: true })[0];
  const obstacles = [
    { x: 20, y: 96, width: 110, height: 110 },
    { x: 85, y: 92, width: 397, height: 58 },
    { x: 420, y: 96, width: 124, height: 48 },
    { x: 182, y: 246, width: 204, height: 28 },
    { x: 24, y: 284, width: 520, height: 28 },
  ];
  const landscape = layoutTouch(config, { width: 568, height: 320, fps: true, obstacles })[0];
  assert.equal(landscape.id, 'joystick'); assert.equal(landscape.width, 80);
  assert.ok(portrait.y > 320); assert.ok(landscape.y < portrait.y);
  assert.ok(landscape.x >= 0 && landscape.y >= 0 && landscape.x + landscape.width <= 568 && landscape.y + landscape.height <= 320);
  for (const b of obstacles) assert.ok(landscape.x + 80 <= b.x || b.x + b.width <= landscape.x || landscape.y + 102 <= b.y || b.y + b.height <= landscape.y);
  const fallback = layoutTouch(config, { width: 568, height: 320, fps: true, obstacles: [{ x: 0, y: 0, width: 568, height: 320 }] })[0];
  assert.equal(fallback.width, 80); assert.ok(fallback.x >= 0 && fallback.x + 80 <= 568 && fallback.y >= 0 && fallback.y + 80 <= 320);
});

test('custom touch actions share InputSystem movement/analog look and action sampling', () => {
  const input = new InputSystem({ invert_y: true, look_sensitivity: 2 });
  const sampled = input.sample({ keys: new Set(['KeyW']), touch: { x: 1, actions: { move_right: 1, fire: 1, reload: 1, sprint: 1, action: 1, pause: 1, look_y: 1, unknown: 1 } } });
  assert.ok(Math.hypot(sampled.move_x, sampled.move_z) <= 1); assert.ok(sampled.move_z < 0 && sampled.move_x > 0);
  for (const action of ['fire', 'reload', 'sprint', 'action', 'pause']) assert.equal(sampled.actions[action], 1);
  assert.equal(sampled.look_y, -2); assert.equal(sampled.actions.unknown, undefined);
  assert.equal(input.sample().actions.fire, 0);
});

test('haptics settings validate and survive remap serialization/reset', () => {
  const input = new InputSystem({ haptics: { enabled: false, strength: 0.4 } });
  input.rebind('action', { type: 'key', code: 'KeyE' });
  assert.deepEqual(new InputSystem(input.toJSON()).config.haptics, { enabled: false, strength: 0.4 });
  for (const haptics of [false, { enabled: 1 }, { strength: -1 }, { strength: 2 }]) assert.ok(parseInputConfig({ haptics }).warnings.length);
  input.reset(); assert.deepEqual(input.config.haptics, { enabled: true, strength: 1 });
});

test('haptic cues prioritize real collect/win and FPS damage/hit events, ignoring idle/unrelated events', () => {
  assert.equal(hapticCue(), null); assert.equal(hapticCue({}, [{ type: 'kill' }]), null);
  assert.equal(hapticCue({ collect: true }), 'collect'); assert.equal(hapticCue({ win: true, collect: true }), 'win');
  assert.equal(hapticCue({}, [{ type: 'damage', to_player: true }]), 'damage');
  assert.equal(hapticCue({}, [{ type: 'damage', by_player: true }]), 'hit');
  assert.equal(hapticCue({}, [{ type: 'hit' }]), 'hit');
});

test('haptics use active gamepad rumble or touch vibration, respect strength/disable, never throw', async () => {
  const calls = [], device = { vibrate: ms => calls.push(ms) }, settings = { enabled: true, strength: 0.5 };
  const pad = { connected: true, vibrationActuator: { playEffect: (type, value) => { calls.push({ type, ...value }); return Promise.resolve(); } } };
  assert.equal(pulseHaptics('collect', { settings, pads: [null, pad], gamepadActive: true, touch: true, device }), true);
  assert.equal(calls[0].type, 'dual-rumble'); assert.equal(calls[0].strongMagnitude, 0.2); assert.equal(calls.length, 1);
  pulseHaptics('collect', { settings, touch: true, device }); assert.equal(calls[1], 30);
  for (const options of [{ settings: { enabled: false } }, { settings: { strength: 0 } }, { touch: false }, { device: {} }, { gamepadActive: true, pads: [] }]) assert.equal(pulseHaptics('collect', { touch: true, device, ...options }), false);
  assert.equal(pulseHaptics(null), false); assert.equal(pulseHaptics('unknown', { touch: true, device }), false);
  assert.doesNotThrow(() => pulseHaptics('hit', { touch: true, device: { vibrate() { throw new Error(); } } }));
  pad.vibrationActuator.playEffect = () => Promise.reject(new Error('unsupported'));
  assert.doesNotThrow(() => pulseHaptics('hit', { pads: [pad], gamepadActive: true }));
  await new Promise(resolve => setImmediate(resolve));
});
