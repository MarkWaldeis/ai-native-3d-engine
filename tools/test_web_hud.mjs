import assert from 'node:assert/strict';
import test from 'node:test';
import { parseHudConfig, resolveHudWidgets, hudVisible, hudTemplate, hudBar, countHudTags, layoutHud } from '../crates/engine-web/player/hud-layout.js';

const widget = (id, rest = {}) => ({ id, type: 'text', ...rest });
test('HUD parser supports object/JSON, panels and phone overrides without mutating input', () => {
  const source = { replace_default: true, widgets: [widget('panel', { type: 'panel', children: [widget('score', { text: '{score}', phone: { anchor: 'bottom', offset: [2, 3], size: 'sm', hidden: false } })] })] };
  const { config, warnings } = parseHudConfig(source);
  assert.deepEqual(warnings, []); assert.equal(config.replace_default, true);
  assert.deepEqual(parseHudConfig(JSON.stringify(source)).config, config);
  assert.equal(config.widgets[0].children[0].width, 220);
  assert.equal(source.widgets[0].anchor, undefined);
  assert.equal(resolveHudWidgets(config.widgets, 599)[0].children[0].anchor, 'bottom');
  assert.equal(resolveHudWidgets(config.widgets, 600)[0].children[0].anchor, 'top-left');
});
test('HUD parser defaults safely on empty, malformed, duplicate and cyclic config', () => {
  assert.deepEqual(parseHudConfig(undefined), { config: { replace_default: false, widgets: [] }, warnings: [] });
  for (const value of ['{', 'null', 42, [], false]) assert.ok(parseHudConfig(value).warnings.length);
  const { config, warnings } = parseHudConfig({ replace_default: 'yes', widgets: [null, widget('a', { width: -1, offset: [0], color: 'url(x)', max: 0, anchor: 'bad', phone: { size: 'huge', hidden: 1 } }), widget('a'), widget('b', { type: 'unknown' })] });
  assert.equal(config.widgets.length, 1); assert.equal(config.widgets[0].width, 220); assert.ok(warnings.length >= 8);
  assert.ok(parseHudConfig({ widgets: {} }).warnings.length);
  const panel = widget('p', { type: 'panel' }); panel.children = [panel];
  assert.doesNotThrow(() => parseHudConfig({ widgets: [panel] }));
});
test('templates use state, decimals and initial/current tag counts with safe unknown values', () => {
  assert.equal(hudTemplate('Münzen {score}/{total:coin} · {left:coin} · {speed|0} / {speed|0.0}', { score: 1, speed: 2.36 }, { coin: 3 }, { coin: 2 }), 'Münzen 1/3 · 2 · 2 / 2.4');
  assert.equal(hudTemplate('{missing} {obj} {nil} {flag} {missing|0.0}', { obj: {}, nil: null, flag: false }), '– – – false –');
  assert.equal(hudTemplate('{total:nope} {left:nope}'), '0 0');
  assert.equal(hudTemplate('{total:toString} {left:constructor}'), '0 0');
  assert.equal(hudTemplate(''), '');
  assert.equal(hudTemplate('<img>{x}', { x: '<script>' }), '<img><script>');
  assert.deepEqual({ ...countHudTags([{ tags: ['coin', 'coin'] }, {}, { tags: ['coin', 'enemy'] }]) }, { coin: 2, enemy: 1 });
  assert.deepEqual({ ...countHudTags([]) }, {});
});
test('visibility and bars handle state maxima, empty, invalid, zero and clamped values', () => {
  assert.equal(hudVisible({}, {}), true);
  assert.equal(hudVisible({ visible_if: 'ready' }, { ready: true }), true);
  assert.equal(hudVisible({ visible_if: '!ready' }, { ready: true }), false);
  assert.equal(hudVisible({ visible_if: '!ready' }, {}), true);
  assert.equal(hudVisible({ visible_if: 'toString' }, {}), false);
  assert.deepEqual(hudBar({ bind: 'hp', max: 'cap' }, { hp: 20, cap: 100 }), { ratio: 0.2, low: true });
  assert.deepEqual(hudBar({ bind: 'hp', max: 100 }, { hp: 30 }), { ratio: 0.3, low: false });
  assert.equal(hudBar({ bind: 'hp' }, { hp: 200 }).ratio, 1);
  for (const hp of [undefined, NaN, -2, 'bad']) assert.equal(hudBar({ bind: 'hp' }, { hp }).ratio, 0);
  assert.equal(hudBar({ bind: 'hp', max: 'cap' }, { hp: 1, cap: 0 }).ratio, 0);
});
test('layout resolves phone overrides, safe insets, offsets, anchor stacking and touch clearance', () => {
  const { config } = parseHudConfig({ widgets: [widget('a', { anchor: 'bottom-left', width: 900, phone: { size: 'sm' } }), widget('b', { anchor: 'bottom-left' }), widget('c', { anchor: 'top-right', phone: { hidden: true } })] });
  assert.equal(resolveHudWidgets(config.widgets, 390).length, 2);
  assert.equal(resolveHudWidgets(config.widgets, 390)[0].size, 'sm');
  const positions = layoutHud(config.widgets, { width: 390, height: 844, touch: true, controlsTop: 640, insets: { left: 20, bottom: 34, top: 44 } }, { a: 50, b: 60 });
  assert.equal(positions.length, 2);
  assert.equal(positions[0].x, 32); assert.ok(positions[0].width <= 346);
  assert.ok(positions[0].y + 50 <= 632); assert.ok(positions[1].y + 60 + 8 <= positions[0].y);
  const desktop = layoutHud(config.widgets, { width: 1280, height: 800 });
  assert.equal(desktop.length, 3); assert.ok(desktop[2].x > 1000);
  assert.deepEqual(layoutHud([], { width: 10, height: 10 }), []);
  const { config: shifted } = parseHudConfig({ widgets: [widget('one', { offset: [-500, -500] }), widget('two')] });
  const rows = layoutHud(shifted.widgets, { width: 390, height: 844 }, { one: 90 });
  assert.ok(rows[0].x >= 12 && rows[0].y >= 12); assert.ok(rows[1].y >= rows[0].y + 98);
  const crowded = layoutHud(shifted.widgets, { width: 100, height: 120 }, { one: 900, two: 900 });
  assert.ok(crowded[0].y + crowded[0].maxHeight + 8 <= crowded[1].y);
  assert.ok(crowded[1].y + crowded[1].maxHeight <= 108);
});
