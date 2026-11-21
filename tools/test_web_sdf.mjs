import assert from 'node:assert/strict';
import test from 'node:test';
import { edt, glyphSdf, ShelfPacker, layoutText, parseTextConfig } from '../crates/engine-web/player/sdf-font.js';
import { hudTemplate, parseHudConfig } from '../crates/engine-web/player/hud-layout.js';
import { HudLayout } from '../crates/engine-web/player/hud-renderer.js';

test('SDF templates share authoritative HUD counts without ai_hud, including initial authored entities and save restore', () => {
  const coins = [1, 2, 3].map(id => ({ id, tags: ['coin'] }));
  const hud = Object.create(HudLayout.prototype);
  hud.config = parseHudConfig(undefined).config; hud.nodes = new Map();
  hud.updateWidgets = hud.layout = () => assert.fail('no HUD widgets should be rendered');
  const text = () => hudTemplate('{left:coin}/{total:coin}', hud.state, hud.totals, hud.left);
  hud.reset(coins);
  hud.update({ state: {}, spawned: coins, removed: [] });
  assert.equal(text(), '3/3');
  hud.update({ state: {}, spawned: [], removed: [1] });
  assert.equal(text(), '2/3');
  hud.reset(coins);
  hud.update({ state: {}, spawned: coins.slice(1), removed: [] });
  assert.equal(text(), '2/3', 'restored initial snapshot contains only surviving entities');
  assert.equal(hud.entities.has(1), false, 'authored removed coin must not be resurrected');
  hud.update({ state: {}, spawned: [{ id: 4, tags: ['coin'] }], removed: [999] });
  assert.equal(text(), '3/3');
  hud.update({ state: {} }); assert.equal(text(), '3/3');
  hud.reset([]); hud.update({ state: {}, spawned: [], removed: [] });
  assert.equal(text(), '0/0');
});

test('EDT returns squared Euclidean distances without changing input, including rectangular/empty grids', () => {
  const grid = [Infinity, Infinity, Infinity, Infinity, 0, Infinity, Infinity, Infinity, Infinity];
  assert.deepEqual([...edt(grid, 3, 3)], [2, 1, 2, 1, 0, 1, 2, 1, 2]);
  assert.equal(grid[0], Infinity);
  assert.deepEqual([...edt([0, Infinity, Infinity, Infinity, Infinity, Infinity], 3, 2)], [0, 1, 4, 1, 2, 5]);
  assert.deepEqual([...edt([], 0, 0)], []);
  assert.ok([...edt([Infinity, Infinity], 2, 1)].every(v => v >= 1e19));
});
test('signed glyph distances increase into ink, preserve half-alpha edges and handle blank/solid glyphs', () => {
  const alpha = Uint8Array.from([0, 0, 0, 128, 255, 255, 255]);
  const sdf = glyphSdf(alpha, 7, 1);
  assert.ok(Math.abs(sdf[3] - 128) <= 1);
  for (let i = 1; i < sdf.length; i++) assert.ok(sdf[i] >= sdf[i - 1]);
  assert.ok(sdf[0] < 128 && sdf[6] > 128);
  assert.deepEqual([...glyphSdf(new Uint8Array(4), 2, 2)], [0, 0, 0, 0]);
  assert.deepEqual([...glyphSdf(new Uint8Array(4).fill(255), 2, 2)], [255, 255, 255, 255]);
  const bitmap = new Uint8Array(49); for (let y = 2; y < 5; y++) for (let x = 2; x < 5; x++) bitmap[y * 7 + x] = 255;
  const field = glyphSdf(bitmap, 7, 7);
  assert.ok(field[24] > field[16] && field[16] > field[0]);
});
test('shelves pack without overlap, reject overflow without consuming space', () => {
  const packer = new ShelfPacker(12, 10), boxes = [packer.pack(7, 3), packer.pack(5, 4), packer.pack(8, 6), packer.pack(4, 6)];
  assert.ok(boxes.every(Boolean));
  for (const [i, a] of boxes.entries()) for (const b of boxes.slice(i + 1)) assert.ok(a.x + a.width <= b.x || b.x + b.width <= a.x || a.y + a.height <= b.y || b.y + b.height <= a.y);
  assert.equal(packer.pack(1, 1), null);
  const fresh = new ShelfPacker(10, 10);
  for (const [w, h] of [[11, 1], [1, 11], [0, 1], [-1, 2]]) assert.equal(fresh.pack(w, h), null);
  assert.deepEqual(fresh.pack(10, 10), { x: 0, y: 0, width: 10, height: 10 });
});
const metric = char => ({ advance: 1, left: 0, top: 0.85, width: char === ' ' ? 0 : 1, height: 1 });
test('layout wraps at spaces, handles newlines/umlauts, alignment and unchanged inputs', () => {
  const left = layoutText('äö üß', metric, { size: 1, maxWidth: 3, align: 'left' });
  assert.equal(left.length, 4); assert.equal(left[0].x, 0); assert.equal(left[2].x, 0); assert.ok(left[0].y > left[2].y);
  assert.equal(layoutText('AB', metric, { size: 1 })[0].x, -1);
  assert.equal(layoutText('AB', metric, { size: 1, align: 'right' })[0].x, -2);
  assert.equal(layoutText('A\nB', metric, { size: 1, lineHeight: 2 })[0].y - layoutText('A\nB', metric, { size: 1, lineHeight: 2 })[1].y, 2);
  assert.equal(layoutText('😀', metric).length, 1);
  assert.deepEqual(layoutText('', metric), []);
  assert.equal(layoutText('longword', metric, { maxWidth: 1 }).length, 8);
});
test('ai_text validates strings/objects and safely warns on invalid configuration', () => {
  assert.equal(parseTextConfig('Münze').config.text, 'Münze');
  const source = { text: '{score}', offset: [1, 2, 3], outline_width: 0.4, billboard: false, depth_test: false, align: 'left', max_width: 4, fade_distance: 20 };
  const { config, warnings } = parseTextConfig(source);
  assert.deepEqual(warnings, []); assert.equal(config.size, 0.5); assert.equal(config.billboard, false);
  config.offset[0] = 9; assert.equal(source.offset[0], 1);
  for (const value of [undefined, null, '']) assert.deepEqual(parseTextConfig(value).warnings, []);
  for (const value of [12, [], false]) assert.ok(parseTextConfig(value).warnings.length);
  const invalid = parseTextConfig({ text: 1, size: -1, color: 'url(x)', outline_width: 0.5, offset: [0], billboard: 'yes', align: 'bad', max_width: NaN, fade_distance: -2, depth_test: 0 });
  assert.equal(invalid.warnings.length, 10); assert.equal(invalid.config.size, 0.5);
  const bad = {}; Object.defineProperty(bad, 'text', { get() { throw new Error('bad'); } });
  assert.doesNotThrow(() => parseTextConfig(bad)); assert.ok(parseTextConfig(bad).warnings.length);
});
