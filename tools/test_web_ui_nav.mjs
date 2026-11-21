import test from 'node:test';
import assert from 'node:assert/strict';
import { focusStep, menuKey, menuPad, MenuNavigation } from '../crates/engine-web/player/ui-nav.js';
import { OverlayNavigation } from '../crates/engine-web/player/ui-navigation.js';
import { ActionLatch } from '../crates/engine-web/player/controls.js';

const pad = () => ({ connected: true, mapping: 'standard', axes: [0, 0], buttons: Array.from({ length: 17 }, () => ({ value: 0 })) });
test('focus order wraps, including missing focus and empty overlays', () => {
  assert.equal(focusStep(2, 3, 1), 0); assert.equal(focusStep(0, 3, -1), 2);
  assert.equal(focusStep(-1, 3, 1), 0); assert.equal(focusStep(-1, 3, -1), 2); assert.equal(focusStep(0, 0, 1), -1);
  assert.equal(menuKey('Tab', true), 'previous'); assert.equal(menuKey('ArrowDown'), 'next');
  assert.equal(menuKey('Escape'), 'back'); assert.equal(menuKey('Enter'), 'activate'); assert.equal(menuKey('KeyW'), null);
});
test('standard pad navigation has deadzone, initial delay and bounded repeat without action repeats', () => {
  const p = pad(), nav = new MenuNavigation();
  p.axes[1] = 0.5; assert.deepEqual(nav.sample([p], 0), []);
  p.axes[1] = 0.8; assert.deepEqual(nav.sample([p], 1), ['next']);
  assert.deepEqual(nav.sample([p], 400), []); assert.deepEqual(nav.sample([p], 401), ['next']);
  assert.deepEqual(nav.sample([p], 520), []); assert.deepEqual(nav.sample([p], 521), ['next']);
  p.axes[1] = 0; p.buttons[0].value = 1; assert.deepEqual(nav.sample([p], 522), ['activate']);
  assert.deepEqual(nav.sample([p], 900), []); p.buttons[0].value = 0; nav.sample([p], 901);
  p.buttons[1].value = 1; assert.deepEqual(nav.sample([p], 902), ['back']);
});
test('capture receives buttons and axes without navigation or deferred activation', () => {
  const p = pad(), nav = new MenuNavigation();
  p.buttons[0].value = 1; p.axes[0] = 0.9;
  assert.deepEqual(nav.sample([p], 0, true), []);
  assert.deepEqual(nav.sample([p], 50), [], 'a captured held axis cannot become menu navigation');
  p.axes[0] = 0; assert.deepEqual(nav.sample([p], 100), []);
  p.buttons[0].value = 0; nav.sample([p], 101); p.buttons[9].value = 1;
  assert.deepEqual(nav.sample([p], 102), ['pause']);
  assert.equal(menuPad([null, { ...p, mapping: '' }]).connected, false);
  assert.equal(menuPad().held, false); assert.equal(menuPad(null).connected, false);
});

test('menu keys and their repeats cannot queue gameplay actions before release', () => {
  for (const code of ['Space', 'Enter', 'KeyW']) {
    const navigation = Object.assign(Object.create(OverlayNavigation.prototype), {
      scope: {}, keys: new Set(), sync() {}, capture: () => false,
      command() { this.scope = null; },
    });
    const action = new ActionLatch();
    const keydown = repeat => {
      let stopped = false;
      navigation.keydown({ code, repeat, preventDefault() {}, stopImmediatePropagation() { stopped = true; } });
      if (!stopped && !navigation.scope) action.press(code);
      return stopped;
    };
    assert.equal(keydown(false), code !== 'KeyW', `${code} starts in the menu`);
    assert.equal(action.sample(), 0);
    navigation.scope = null;
    assert.equal(keydown(true), true, `${code} repeats stay suppressed after the menu closes`);
    navigation.keys.delete(code); action.release(code);
    assert.equal(action.sample(), 0, `${code} release has no queued gameplay action`);
    assert.equal(keydown(false), false, `${code} can reach gameplay after release`);
    assert.equal(action.sample(), 1);
  }
});

test('remap capture retains precedence over keyboard suppression', () => {
  const navigation = Object.assign(Object.create(OverlayNavigation.prototype), {
    scope: {}, keys: new Set(['Enter']), sync() {}, capture: () => true,
  });
  navigation.keydown({ code: 'Enter', repeat: true, preventDefault() { assert.fail('capture owns the key'); } });
});
