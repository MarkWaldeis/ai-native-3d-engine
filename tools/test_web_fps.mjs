import assert from 'node:assert/strict';
import test from 'node:test';
import { applyLook, combatCues, fpsConfig, fpsInputs, heading, lookDir, minimapPoint, PITCH_LIMIT, reloadDip, SlotPool, tracerSpan, wrapAngle } from '../crates/engine-web/player/fps_core.js';

const close = (a, b) => assert.ok(Math.abs(a - b) < 1e-9, `${a} != ${b}`);

test('fpsConfig only enables first person for a player with ai_fps', () => {
  assert.equal(fpsConfig([{ tags: ['player'], props: {} }]), null);
  assert.equal(fpsConfig([{ tags: ['enemy'], props: { ai_fps: {} } }]), null);
  const config = fpsConfig([{ tags: ['player'], transform: { rotation_deg: [0, 90, 0] }, props: { ai_fps: '{"move_speed":3,"mouse_sens":"x"}', ai_viewmodel: 'assets/rifle.glb' } }]);
  assert.equal(config.move_speed, 3); assert.equal(config.mouse_sens, 0.0022); assert.equal(config.eye_height, 0.75);
  assert.equal(config.viewmodel, 'assets/rifle.glb'); close(config.yaw, Math.PI / 2);
});

test('look is absolute radians, yaw wraps and pitch clamps', () => {
  let look = { yaw: 0, pitch: 0 };
  look = applyLook(look, 100, 0, 0.01); close(look.yaw, -1);
  look = applyLook(look, 0, -1e6, 0.01); assert.equal(look.pitch, PITCH_LIMIT);
  close(wrapAngle(Math.PI * 3), -Math.PI);
  assert.ok(Math.abs(applyLook({ yaw: 3, pitch: 0 }, -100, 0, 0.01).yaw) <= Math.PI);
});

test('fpsInputs sends exactly the contract fields', () => {
  const inputs = fpsInputs({ x: 1, z: -1 }, { yaw: 0.5, pitch: -0.2 }, { fire: true, reload: false, sprint: 1, jump: 0 });
  assert.deepEqual(Object.keys(inputs).sort(), ['fire', 'jump', 'look_pitch', 'look_yaw', 'move_x', 'move_z', 'reload', 'sprint']);
  close(Math.hypot(inputs.move_x, inputs.move_z), 1);
  assert.equal(inputs.fire, 1); assert.equal(inputs.reload, 0); assert.equal(inputs.sprint, 1); assert.equal(inputs.jump, 0);
  assert.equal(inputs.look_yaw, 0.5); assert.equal(inputs.look_pitch, -0.2);
});

test('direction, compass and minimap share the runtime convention (yaw 0 = -Z)', () => {
  const forward = lookDir(0, 0); close(forward[2], -1); close(lookDir(Math.PI / 2, 0)[0], -1);
  close(heading(0), 0); close(heading(-Math.PI / 2), 90);
  const ahead = minimapPoint([0, 0, 0], 0, [0, 0, -10], 20, 100); close(ahead.x, 50); close(ahead.y, 25);
  const right = minimapPoint([0, 0, 0], 0, [10, 0, 0], 20, 100); close(right.x, 75); close(right.y, 50);
  const turned = minimapPoint([0, 0, 0], -Math.PI / 2, [10, 0, 0], 20, 100); close(turned.x, 50); close(turned.y, 25);
  assert.equal(minimapPoint([0, 0, 0], 0, [100, 0, 0], 20, 100), null);
});

test('SlotPool recycles the oldest slot instead of allocating', () => {
  const pool = new SlotPool(3);
  assert.deepEqual([pool.take(), pool.take(), pool.take(), pool.take()], [0, 1, 2, 0]);
  assert.equal(pool.used, 3);
});

test('combatCues maps runtime events to HUD cues', () => {
  const cues = combatCues({
    fx: [{ kind: 'muzzle', pos: [0, 1, 0], dir: [0, 0, -1] }, { kind: 'muzzle', pos: [20, 1, 0], dir: [0, 0, 1] }],
    events: [
      { type: 'damage', by_player: true, to_player: false, amount: 34, killed: true },
      { type: 'kill', by_player: true, target_name: 'Gegner 1' },
      { type: 'damage', by_player: false, to_player: true, amount: 12 },
    ],
  }, [0, 1, 0]);
  assert.equal(cues.shots, 1); assert.equal(cues.hits, 1); assert.equal(cues.killedTarget, true);
  assert.equal(cues.damageTaken, 12); assert.deepEqual(cues.kills, ['Du ✕ Gegner 1']);
  assert.deepEqual(combatCues({ state: {} }, null), { shots: 0, hits: 0, kills: [], damageTaken: 0, killedTarget: false });
});

test('tracer span and reload dip', () => {
  assert.deepEqual(tracerSpan([0, 0, -1]), [0, 0, -30]);
  assert.deepEqual(tracerSpan([0, 0, -12]), [0, 0, -12]);
  assert.equal(reloadDip(0, 1.6), 0); close(reloadDip(0.8, 1.6), 1);
});
