import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const player = readFileSync(new URL('../crates/engine-web/player/player.js', import.meta.url), 'utf8');
const helpers = player.slice(player.indexOf('const saveKey ='), player.indexOf('function startRunning()'));

test('pause save controls share restart styling and accessible touch targets', () => {
  const html = readFileSync(new URL('../crates/engine-web/player/index.html', import.meta.url), 'utf8');
  const css = readFileSync(new URL('../crates/engine-web/player/style.css', import.meta.url), 'utf8');
  for (const id of ['restart', 'save-game', 'load-game']) {
    assert.match(html, new RegExp(`id="${id}" class="pause-menu-button"`));
  }
  assert.match(css, /\.pause-menu-button\{[^}]*min-height:44px/);
  assert.match(css, /\.pause-menu-button:disabled\{[^}]*background:#596268/);
});

function setup(pathname = '/game/') {
  const values = new Map();
  const loaded = [];
  const window = { addEventListener(type, fn) { this[type] = fn; } };
  const elements = new Map();
  const $ = id => {
    if (!elements.has(id)) elements.set(id, { addEventListener(type, fn) { this[type] = fn; } });
    return elements.get(id);
  };
  const context = vm.createContext({
    window, location: { pathname }, ready: true, console, $, setTimeout: () => 1, clearTimeout() {},
    game: { save_game: () => '{"version":1}' },
    localStorage: { setItem: (k, v) => values.set(k, v), getItem: k => values.get(k) ?? null },
    restart: async save => loaded.push(save), startRunning: () => loaded.push('running'),
  });
  vm.runInContext(helpers, context);
  return { context, window, values, loaded, $ };
}

test('save helpers persist per game and restore through runtime reload', async () => {
  const { window, values, loaded } = setup();
  assert.equal(window.__aiSaveGame(), '{"version":1}');
  assert.equal(values.get('ai-engine-save:v1:/game/'), '{"version":1}');
  assert.equal(await window.__aiLoadGame(), true);
  assert.deepEqual(loaded, ['{"version":1}', 'running']);
  assert.equal(await setup('/other/').window.__aiLoadGame(), false);
});

test('empty or unavailable storage and runtime errors are not hidden by helpers', async () => {
  const { context, window, loaded } = setup();
  assert.equal(await window.__aiLoadGame(), false);
  assert.deepEqual(loaded, []);
  context.ready = false;
  assert.throws(() => window.__aiSaveGame(), /bereit/);
  context.ready = true;
  context.localStorage.setItem = () => { throw Error('quota'); };
  assert.throws(() => window.__aiSaveGame(), /quota/);
  context.localStorage.getItem = () => '{}';
  context.restart = async () => { throw Error('malformed'); };
  await assert.rejects(window.__aiLoadGame(), /malformed/);
});

test('F5 and F9 suppress browser navigation and ignore held repeats', async () => {
  const { window, values, loaded } = setup();
  let prevented = 0;
  const key = (code, repeat = false) => window.keydown({ code, repeat, preventDefault: () => prevented++ });
  key('F5'); await new Promise(resolve => setImmediate(resolve));
  assert.equal(values.size, 1);
  key('F9', true); assert.deepEqual(loaded, []);
  key('F9'); await new Promise(resolve => setImmediate(resolve));
  assert.equal(loaded.length, 2);
  key('KeyA'); assert.equal(prevented, 3);
});

test('checkpoint autosave saves once per request, not on initial load or restore', () => {
  const { context, values, $ } = setup();
  vm.runInContext('checkpointAutosave({}, true); checkpointAutosave({});', context);
  assert.equal(values.size, 0);
  vm.runInContext('checkpointAutosave({_autosave:1});', context);
  assert.equal(values.size, 1);
  assert.equal($('save-status').textContent, 'Gespeichert');
  context.game.save_game = () => { throw Error('should not save'); };
  vm.runInContext('checkpointAutosave({_autosave:1}); checkpointAutosave({_autosave:2}, true); checkpointAutosave({_autosave:2});', context);
  assert.equal($('save-status').textContent, 'Gespeichert');
  context.localStorage.setItem = () => { throw Error('quota'); };
  vm.runInContext('checkpointAutosave({_autosave:3});', context);
  assert.equal($('save-status').textContent, 'Speichern fehlgeschlagen');
});

test('pause menu save and load are clickable and missing saves are disabled', async () => {
  const { context, $, loaded } = setup();
  vm.runInContext('updateSaveMenu()', context);
  assert.equal($('load-game').disabled, true);
  assert.match($('load-game').title, /kein Spielstand/);
  $('save-game').click();
  assert.equal($('load-game').disabled, false);
  await $('load-game').click();
  assert.deepEqual(loaded, ['{"version":1}', 'running']);
  context.restart = async () => { throw Error('bad save'); };
  await $('load-game').click();
  assert.equal($('save-status').textContent, 'Laden fehlgeschlagen');
  assert.equal($('load-game').disabled, false);
});
