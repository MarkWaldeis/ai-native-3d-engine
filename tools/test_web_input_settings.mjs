import assert from 'node:assert/strict';
import test from 'node:test';
import { InputSystem } from '../crates/engine-web/player/input.js';
import { InputSettings } from '../crates/engine-web/player/input-settings.js';

class Element extends EventTarget {
  children = []; dataset = {}; open = false;
  append(...children) { this.children.push(...children); }
  replaceChildren() { this.children = []; }
  setAttribute() {}
  focus() { this.focused = true; }
  showModal() { this.open = true; }
  close() { this.open = false; this.dispatchEvent(new Event('close')); }
  querySelector(selector) {
    const action = selector.match(/"(.*?)"/)[1];
    return this.children.flatMap(row => row.children).find(child => child.dataset.action === action);
  }
}
function fixture(saved, authored) {
  const elements = new Map(), storage = new Map();
  globalThis.document = { getElementById: id => { if (!elements.has(id)) elements.set(id, new Element()); return elements.get(id); }, createElement: () => new Element() };
  globalThis.localStorage = { getItem: key => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value), removeItem: key => storage.delete(key) };
  Object.defineProperty(globalThis, 'navigator', { configurable: true, value: { getGamepads: () => [] } });
  if (saved !== undefined) storage.set('ai-engine:input:Test', saved);
  let paused = 0, changes = 0;
  const input = new InputSystem(), settings = new InputSettings(input, authored, 'Test', () => paused++, () => changes++);
  return { input, settings, storage, elements, paused: () => paused, changes: () => changes };
}

test('settings opens paused, replaces an action, persists per title and resets authored defaults', () => {
  const { input, settings, storage, elements, paused, changes } = fixture(undefined, { deadzone: 0.3 });
  assert.equal(changes(), 1, 'load refreshes help');
  settings.show(); assert.equal(settings.open, true); assert.equal(paused(), 1);
  elements.get('input-bindings').querySelector('[data-action="action"]').dispatchEvent(new Event('click'));
  settings.bind({ type: 'key', code: 'KeyE' });
  assert.equal(changes(), 3, 'open and rebind refresh help');
  assert.deepEqual(input.config.bindings.action, [{ type: 'key', code: 'KeyE' }]);
  assert.equal(JSON.parse(storage.get(settings.storageKey)).bindings.action[0].code, 'KeyE');
  settings.dialog.close(); assert.equal(settings.open, false); assert.ok(settings.trigger.focused);
  elements.get('input-reset').dispatchEvent(new Event('click'));
  assert.equal(changes(), 4, 'reset refreshes help');
  assert.equal(storage.has(settings.storageKey), false); assert.equal(input.config.deadzone, 0.3);
  assert.equal(input.config.bindings.action[0].code, 'Space');
});

test('saved config loads; corrupt or unavailable storage produces warnings, not failures', () => {
  const config = new InputSystem(); config.rebind('fire', { type: 'key', code: 'KeyE' });
  assert.deepEqual(fixture(JSON.stringify(config)).input.toJSON(), config.toJSON());
  assert.ok(fixture('{').settings.warnings.length);
  assert.ok(fixture('null').settings.warnings.length);
  const { settings, input } = fixture();
  globalThis.localStorage.setItem = () => { throw new Error('denied'); };
  settings.capturing = 'action'; settings.bind({ type: 'key', code: 'KeyE' });
  assert.equal(input.config.bindings.action[0].code, 'KeyE');
  assert.ok(settings.warnings.some(message => message.includes('Speichern')));
});

test('gamepad capture waits for held buttons to release and Escape cancels keyboard capture', () => {
  const { settings, input, elements } = fixture();
  settings.show();
  const pads = [{ buttons: [{ pressed: true }], axes: [0, 0] }];
  navigator.getGamepads = () => pads;
  elements.get('input-bindings').querySelector('[data-action="fire"]').dispatchEvent(new Event('click'));
  settings.poll(pads); assert.equal(settings.capturing, 'fire');
  settings.poll([]); settings.poll(pads);
  assert.deepEqual(input.config.bindings.fire, [{ type: 'button', index: 0 }]);
  assert.ok(!input.config.bindings.action.some(binding => binding.type === 'button' && binding.index === 0));
  settings.capturing = 'action';
  const escape = new Event('keydown', { cancelable: true }); escape.code = 'Escape';
  settings.dialog.dispatchEvent(escape);
  assert.equal(settings.capturing, null); assert.ok(settings.open); assert.ok(escape.defaultPrevented);
});
