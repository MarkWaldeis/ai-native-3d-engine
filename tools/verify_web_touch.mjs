import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import http from 'node:http';
import path from 'node:path';
import { installClock } from './lantern_maze/browser.mjs';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const site = path.resolve(process.argv[2] || 'target/touch-site'), shots = path.resolve('target/touch-shots');
await fs.mkdir(shots, { recursive: true });
const config = fps => ({ buttons: [
  { action: 'fire', label: 'Feuer', icon: '●', anchor: 'bottom-right', size: 'lg', hold: true },
  { action: 'sprint', label: 'Sprint', anchor: 'bottom-left', hold: true },
  { action: 'reload', label: 'Laden', anchor: 'right', hold: false },
  { action: 'pause', label: 'Pause', anchor: 'top-right', hold: false },
], joystick: { side: fps ? 'left' : 'right', size: 130 }, hide_default_action: fps });
const server = http.createServer(async (req, res) => {
  try {
    let file = path.resolve(site, '.' + decodeURIComponent(new URL(req.url, 'http://localhost').pathname));
    if (!file.startsWith(site + path.sep)) throw new Error('outside site');
    if ((await fs.stat(file)).isDirectory()) file = path.join(file, 'index.html');
    res.setHeader('Content-Type', ({ '.wasm': 'application/wasm', '.js': 'text/javascript', '.json': 'application/json', '.html': 'text/html', '.css': 'text/css', '.svg': 'image/svg+xml' })[path.extname(file)] || 'application/octet-stream');
    if (file === path.join(site, 'coin_game/scene.json') || file === path.join(site, 'fps_arena/scene.json')) {
      const scene = JSON.parse(await fs.readFile(file, 'utf8')), player = scene.entities.find(e => e.tags?.includes('player'));
      player.props = { ...player.props, ai_touch: config(file.includes('fps_arena')), ai_input: { haptics: { enabled: true, strength: 0.7 } }, ai_hud: { widgets: [{ id: 'touch-info', type: 'text', text: 'Touch bereit', anchor: 'bottom-right', width: 130 }] } };
      res.end(JSON.stringify(scene));
    } else res.end(await fs.readFile(file));
  } catch { res.writeHead(404); res.end('Not found'); }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const base = `http://127.0.0.1:${server.address().port}`, errors = [];
let browser;
try {
  browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
  for (const game of (process.env.TOUCH_CASES || 'coin_game,fps_arena').split(',')) for (const landscape of [false, true]) {
    const name = `${game}-${landscape ? 'landscape-844x390' : 'portrait-390x844'}`;
    const page = await browser.newPage({ viewport: landscape ? { width: 844, height: 390 } : { width: 390, height: 844 }, hasTouch: true, isMobile: true, deviceScaleFactor: 1 });
    page.setDefaultTimeout(120000);
    page.on('pageerror', error => errors.push(error.message));
    page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
    page.on('response', response => { if (response.status() >= 400) errors.push(`${response.status()} ${response.url()}`); });
    await page.route('**/*', route => new URL(route.request().url()).origin === base ? route.continue() : route.abort());
    await page.addInitScript(installClock);
    await page.addInitScript(withPad => {
      window.haptics = [];
      Object.defineProperty(navigator, 'vibrate', { value: ms => { haptics.push({ type: 'touch', ms }); return true; } });
      window.pad = { connected: withPad, mapping: 'standard', axes: [0, 0, 0, 0], buttons: Array.from({ length: 17 }, () => ({ pressed: false, value: 0 })), vibrationActuator: { playEffect: (type, options) => { haptics.push({ type, ...options }); return Promise.resolve(); } } };
      Object.defineProperty(navigator, 'getGamepads', { value: () => pad.connected ? [null, pad] : [] });
    }, landscape);
    await page.goto(`${base}/${game}/`);
    await page.waitForFunction(() => document.getElementById('start')?.disabled === false, null, { polling: 50 });
    await page.evaluate(async () => {
      const { WebGame } = await import('./pkg/engine_web.js'), tick = WebGame.prototype.tick;
      WebGame.prototype.tick = function(inputs) { window.lastInputs = JSON.parse(inputs); const result = tick.call(this, inputs); window.snapshot = JSON.parse(result); return result; };
    });
    await page.locator('#start').tap(); await page.evaluate(() => { if (pad.connected) pad.buttons[10] = { pressed: true, value: 1 }; advanceFrames(2); pad.buttons[10] = { pressed: false, value: 0 }; });
    assert.equal(await page.locator('[data-touch-action]').count(), 4);
    assert.equal(await page.locator('#action').isHidden(), game === 'fps_arena');
    const problems = await page.evaluate(() => {
      const nodes = [...document.querySelectorAll('#joystick,#action,.fps-fire,.fps-reload-button,[data-touch-action]')].filter(n => n.getClientRects().length);
      const rects = nodes.map(n => ({ name: n.getAttribute('aria-label'), r: n.getBoundingClientRect() })), failures = [];
      const overlap = (a, b) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
      const hud = document.querySelector('[data-hud-id="touch-info"]').getBoundingClientRect();
      for (const [i, { name, r }] of rects.entries()) {
        if (r.left < 0 || r.right > innerWidth || r.top < 0 || r.bottom > innerHeight) failures.push(`${name}: overflow`);
        if (overlap(r, hud)) failures.push(`${name}: HUD overlap`);
        for (const b of rects.slice(i + 1)) if (overlap(r, b.r)) failures.push(`${name} / ${b.name}: overlap`);
      }
      if (document.documentElement.scrollWidth > innerWidth) failures.push('horizontal overflow');
      return failures;
    });
    assert.deepEqual(problems, []);
    const cdp = await page.context().newCDPSession(page);
    const center = async selector => { const r = await page.locator(selector).boundingBox(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; };
    const stick = await center('#joystick'), sprint = await center('[data-touch-action="sprint"]'), fire = await center('[data-touch-action="fire"]');
    const points = [{ id: 1, ...stick }];
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: points });
    points[0].x += 35;
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: points });
    points.push({ id: 2, ...sprint }); await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: points });
    points.push({ id: 3, ...fire }); await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: points });
    await page.evaluate(() => advanceFrames(2));
    const inputs = await page.evaluate(() => lastInputs);
    assert.ok(Math.hypot(inputs.move_x, inputs.move_z) > 0.1, 'simultaneous joystick reaches real WASM tick');
    if (game === 'fps_arena') { assert.equal(inputs.sprint, 1); assert.equal(inputs.fire, 1); }
    await page.screenshot({ path: path.join(shots, `touch-${name}.png`) });
    const released = points.pop(); await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [released] });
    await page.evaluate(() => advanceFrames(1));
    if (game === 'fps_arena') { assert.equal(await page.evaluate(() => lastInputs.fire), 0); assert.equal(await page.evaluate(() => lastInputs.sprint), 1); }
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
    await page.evaluate(() => advanceFrames(1));
    assert.equal(await page.evaluate(() => lastInputs.move_x), 0);
    if (game === 'fps_arena') {
      assert.equal(await page.evaluate(() => lastInputs.sprint), 0);
      const reload = await center('[data-touch-action="reload"]');
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ id: 4, ...reload }] });
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
      await page.evaluate(() => advanceFrames(1)); assert.equal(await page.evaluate(() => lastInputs.reload), 1, 'short tap retained until tick');
      await page.evaluate(() => advanceFrames(1)); assert.equal(await page.evaluate(() => lastInputs.reload), 0, 'hold:false is a one-tick impulse');
    } else {
      await page.keyboard.down('KeyD'); let score = 0, diagonal = false;
      for (let i = 0; i < 300 && !score; i++) {
        const position = await page.evaluate(() => snapshot.transforms.find(e => e.tags.includes('player'))?.transform.position);
        if (position) { const need = position[2] < 0.02; if (need !== diagonal) { await page.keyboard[need ? 'down' : 'up']('KeyS'); diagonal = need; } }
        score = await page.evaluate(() => { advanceFrames(1); return snapshot.state.score; });
      }
      await page.keyboard.up('KeyD'); await page.keyboard.up('KeyS'); assert.equal(score, 1);
      const calls = await page.evaluate(() => haptics);
      assert.ok(calls.some(call => call.type === (landscape ? 'dual-rumble' : 'touch')), 'coin pickup pulses the active device');
      if (landscape) assert.ok(!calls.some(call => call.type === 'touch'), 'active pad takes priority over phone');
      await page.locator('[data-touch-action="pause"]').tap(); await page.evaluate(() => advanceFrames(1));
      assert.ok(await page.locator('#modal').isVisible(), 'custom pause follows logical action edges');
      await page.locator('#input-settings').click(); await page.getByLabel('Vibration', { exact: true }).uncheck();
      await page.locator('[data-action="action"]').click(); await page.keyboard.press('KeyE');
      await page.reload(); await page.waitForFunction(() => document.getElementById('start')?.disabled === false, null, { polling: 50 });
      await page.locator('#input-settings').click(); assert.equal(await page.getByLabel('Vibration', { exact: true }).isChecked(), false);
      assert.match(await page.locator('#input-bindings').innerText(), /Taste E/);
      await page.locator('#input-reset').click(); assert.ok(await page.getByLabel('Vibration', { exact: true }).isChecked());
    }
    console.log(`PASS touch ${name}: authored controls, safe layout/HUD clearance, simultaneous real touch pointers, WASM inputs${game === 'coin_game' ? ', coin pickup haptics and persisted Vibration/remap toggle' : ', held sprint/fire, release and queued one-tick reload'}`);
    await page.close();
  }
  assert.deepEqual(errors, []); console.log('PASS touch browser: no console/page errors or failed assets; local WASM/assets only');
} finally { await browser?.close(); await new Promise(resolve => server.close(resolve)); }
