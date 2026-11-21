import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const player = readFileSync(new URL('../crates/engine-web/player/player.js', import.meta.url), 'utf8');
const helpers = player.slice(player.indexOf('const saveKey ='), player.indexOf('function startRunning()'));

function setup(pathname = '/game/') {
  const values = new Map();
  const loaded = [];
  const window = { addEventListener(type, fn) { this[type] = fn; } };
  const context = vm.createContext({
    window, location: { pathname }, ready: true, netEndpoint: null, inputSettings: { open: false }, console,
    game: { save_game: () => '{"version":1}' },
    localStorage: { setItem: (k, v) => values.set(k, v), getItem: k => values.get(k) ?? null },
    restart: async save => loaded.push(save), startRunning: () => loaded.push('running'),
  });
  vm.runInContext(helpers, context);
  return { context, window, values, loaded };
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

test('authoritative multiplayer rejects local save/load without touching storage or restarting', async () => {
  const {context,window,values,loaded}=setup();
  context.netEndpoint='ws://localhost:7790';
  assert.throws(()=>window.__aiSaveGame(),/Multiplayer/);
  await assert.rejects(window.__aiLoadGame(),/Multiplayer/);
  assert.equal(values.size,0);
  assert.deepEqual(loaded,[]);
});

test('save hotkeys do not dismiss an open settings dialog or navigate the browser', async () => {
  const { context, window, loaded, values } = setup();
  context.inputSettings.open = true;
  let prevented = 0;
  for (const code of ['F5', 'F9']) window.keydown({ code, repeat: false, preventDefault: () => prevented++ });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(prevented, 2); assert.equal(values.size, 0); assert.deepEqual(loaded, []);
});
