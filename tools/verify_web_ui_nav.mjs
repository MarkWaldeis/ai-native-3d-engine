import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import http from 'node:http';
import path from 'node:path';
import { installClock } from './lantern_maze/browser.mjs';

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const site = path.resolve(process.argv[2] || 'target/ui-nav-site'), shots = path.resolve('target/ui-nav-shots');
await fs.mkdir(shots, { recursive: true });
const server = http.createServer(async (req, res) => {
  try {
    const url = decodeURIComponent(new URL(req.url, 'http://localhost').pathname), gameover = url.startsWith('/game_over/');
    let file = path.resolve(site, '.' + (gameover ? url.replace('/game_over/', '/coin_game/') : url));
    if (!file.startsWith(site + path.sep)) throw Error('outside site');
    if ((await fs.stat(file)).isDirectory()) file = path.join(file, 'index.html');
    res.setHeader('Content-Type', ({ '.js': 'text/javascript', '.wasm': 'application/wasm', '.html': 'text/html', '.json': 'application/json', '.css': 'text/css', '.svg': 'image/svg+xml' })[path.extname(file)] || 'application/octet-stream');
    let data = await fs.readFile(file);
    if (gameover && path.basename(file) === 'scene.json') {
      const scene = JSON.parse(data);
      scene.entities.find(entity => entity.tags?.includes('player')).script = { source: 'fn on_start(ctx) { ctx.state_set("score", 0); ctx.state_set("lost", false); ctx.state_set("_menu_ticks", 0); } fn on_tick(ctx, dt) { let ticks = ctx.state_get("_menu_ticks") + 1; ctx.state_set("_menu_ticks", ticks); if ticks >= 20 { ctx.state_set("lost", true); } }' };
      data = JSON.stringify(scene);
    }
    res.end(data);
  } catch { res.writeHead(404); res.end(); }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const base = `http://127.0.0.1:${server.address().port}`, errors = [];
let browser;
try {
  browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
  for (const phone of [false, true]) {
    const name = phone ? 'phone-390x844' : 'desktop-1280x800';
    const page = await browser.newPage({ viewport: phone ? { width: 390, height: 844 } : { width: 1280, height: 800 }, isMobile: phone, hasTouch: phone });
    page.setDefaultTimeout(120000);
    page.on('pageerror', error => errors.push(error.message));
    page.on('response', response => { if (response.status() >= 400) errors.push(`${response.status()} ${response.url()}`); });
    await page.route('**/*', route => new URL(route.request().url()).origin === base ? route.continue() : route.abort());
    await page.addInitScript(installClock);
    await page.addInitScript(() => {
      window.testPad = { connected: true, mapping: 'standard', axes: [0, 0, 0, 0], buttons: Array.from({ length: 17 }, () => ({ pressed: false, value: 0 })) };
      Object.defineProperty(navigator, 'getGamepads', { value: () => [testPad] });
    });
    await page.goto(`${base}/coin_game/`);
    await page.waitForFunction(() => !document.getElementById('start').disabled, null, { polling: 50 });
    await page.evaluate(async () => {
      const { WebGame } = await import('./pkg/engine_web.js'), tick = WebGame.prototype.tick;
      window.tickCalls = 0; window.tickInputs = [];
      WebGame.prototype.tick = function (inputs) { tickCalls++; window.lastInputs = JSON.parse(inputs); tickInputs.push(lastInputs); const result = tick.call(this, inputs); window.snapshot = JSON.parse(result); return result; };
      advanceFrames(2);
    });
    const advance = (n = 1) => page.evaluate(n => advanceFrames(n), n);
    const focus = async id => assert.equal(await page.evaluate(() => document.activeElement.id || document.activeElement.dataset.action), id);
    const setButton = (index, pressed) => page.evaluate(({ index, pressed }) => { testPad.buttons[index] = { pressed, value: Number(pressed) }; advanceFrames(1); }, { index, pressed });
    const press = async index => { await setButton(index, true); await setButton(index, false); };
    await focus('start'); assert.ok(await page.locator('#modal .ui-nav-hint').isVisible());
    assert.ok(await page.locator('#joystick').isHidden(), 'gameplay controls do not overlap the overlay');
    assert.equal(await page.locator('#start').evaluate(node => getComputedStyle(node).outlineWidth), '3px');
    await page.screenshot({ path: path.join(shots, `${name}-intro.png`), animations: 'disabled' });
    await page.keyboard.press('Shift+Tab'); await focus('menu-load'); await press(0);
    assert.match(await page.locator('#menu-status').innerText(), /Kein Spielstand/);
    await page.keyboard.press('Tab'); await focus('start');
    await setButton(0, true); await advance(10);
    assert.ok(await page.locator('#modal').isHidden()); assert.equal(await page.evaluate(() => tickCalls), 0, 'held selection cannot leak into gameplay');
    await setButton(0, false); await advance(3); assert.equal(await page.evaluate(() => lastInputs.action), 0);
    for (const key of ['Space', 'Enter']) {
      await press(9); await page.locator('#menu-settings').click();
      await page.locator('[data-action="action"]').click(); await page.keyboard.press(key);
      await press(1); await page.locator('#start').focus();
      const ticks = await page.evaluate(() => tickCalls);
      await page.keyboard.down(key); await page.keyboard.down(key); await advance(10);
      assert.equal(await page.evaluate(() => tickCalls), ticks, `${key} selection holds the simulation until release`);
      await page.keyboard.up(key); await advance(3);
      assert.ok(await page.evaluate(ticks => tickInputs.slice(ticks).every(input => input.action === 0), ticks), `${key} repeat cannot buffer a gameplay action on release`);
      await page.keyboard.press(key); await advance();
      assert.equal(await page.evaluate(() => lastInputs.action), 1, `${key} works as gameplay input after release`);
      await advance(2);
    }
    await page.locator('#input-settings').click(); await advance(); await focus('input-close');
    await press(13); await focus('input-haptics'); await press(1); await focus('start'); await press(0);
    await press(9); await focus('start');
    const paused = await page.evaluate(() => tickCalls);
    await press(13); await focus('restart'); await press(13); await focus('menu-settings'); await press(0);
    await focus('input-close');
    await press(13); await focus('input-haptics'); await press(13); await focus('move_left'); await press(0);
    await press(3);
    assert.match(await page.locator('#input-status').innerText(), /Gamepad Y gespeichert/);
    await focus('move_left'); await advance(30); assert.equal(await page.evaluate(() => tickCalls), paused);
    await page.screenshot({ path: path.join(shots, `${name}-settings.png`), animations: 'disabled' });
    await press(1); await focus('menu-settings');
    await page.keyboard.press('Tab'); await focus('menu-save'); await press(0);
    assert.match(await page.locator('#menu-status').innerText(), /gespeichert/);
    await press(13); await focus('menu-load'); await press(0);
    await page.locator('#modal').waitFor({ state: 'hidden' }); await advance(3);
    await press(9); await focus('start');
    await page.keyboard.press('Shift+Tab'); await focus('menu-load');
    await page.keyboard.press('Tab'); await focus('start');
    await page.evaluate(() => {
      const dialog = document.createElement('dialog'); dialog.id = 'fixture-dialog';
      dialog.innerHTML = '<button id="fixture-primary" data-primary>Zurück</button><button id="fixture-other">Andere Taste</button>';
      document.body.append(dialog); dialog.showModal();
    });
    await advance(); await focus('fixture-primary'); await page.keyboard.press('Tab'); await focus('fixture-other');
    await page.locator('#mute').evaluate(node => node.focus());
    assert.ok(await page.locator('#fixture-dialog').evaluate(node => node.contains(document.activeElement)), 'native topmost dialog cannot lose focus to the footer');
    await press(1); await focus('start');
    await page.evaluate(() => { testPad.axes[1] = 0.8; advanceFrames(1); }); await focus('restart');
    await advance(25); await focus('menu-settings');
    await page.evaluate(() => { testPad.axes[1] = 0; advanceFrames(1); });
    await press(12); await press(12); await focus('start');
    await press(0); assert.ok(await page.locator('#modal').isHidden());
    await page.keyboard.down('KeyD'); let diagonal = false;
    for (let i = 0; i < 900 && !await page.evaluate(() => snapshot.state.won); i++) {
      const need = await page.evaluate(() => __engineStats.playerPosition[2] < 0.02);
      if (need !== diagonal) { await page.keyboard[need ? 'down' : 'up']('KeyS'); diagonal = need; }
      await advance();
    }
    await page.keyboard.up('KeyD'); await page.keyboard.up('KeyS');
    assert.equal(await page.evaluate(() => snapshot.state.won), true); await focus('start');
    await page.screenshot({ path: path.join(shots, `${name}-win.png`), animations: 'disabled' });
    await press(0); await page.locator('#modal').waitFor({ state: 'hidden' }); await advance(3);
    assert.equal(await page.evaluate(() => snapshot.state.score), 0); assert.equal(await page.evaluate(() => lastInputs.action), 0);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
    await press(9); await page.keyboard.press('Shift+Tab'); await focus('menu-load');
    await page.evaluate(() => localStorage.setItem(`ai-engine-save:v1:${location.pathname}`, '{}'));
    await press(0);
    await page.waitForFunction(() => document.getElementById('start').textContent === 'Seite neu laden', null, { polling: 50 });
    await focus('start'); await press(13); await focus('menu-settings'); await press(0); await focus('input-close');
    await press(1); await focus('menu-settings'); await page.keyboard.press('Shift+Tab'); await focus('start');
    await page.screenshot({ path: path.join(shots, `${name}-error.png`), animations: 'disabled' });
    await Promise.all([page.waitForEvent('load'), setButton(0, true)]);
    await page.waitForFunction(() => !document.getElementById('start').disabled, null, { polling: 50 });
    await page.goto(`${base}/game_over/`);
    await page.waitForFunction(() => !document.getElementById('start').disabled, null, { polling: 50 });
    await advance(2); await press(0); await advance(25);
    assert.equal(await page.locator('#modal').getAttribute('data-screen'), 'gameover'); await focus('start');
    await page.screenshot({ path: path.join(shots, `${name}-gameover.png`), animations: 'disabled' });
    await press(0); await page.locator('#modal').waitFor({ state: 'hidden' });
    console.log(`PASS UI navigation ${name}: intro/start, held A isolation, pause, focus trap/restore, D-pad/stick repeat, gamepad remap, B back, save/load/empty/error recovery, keyboard Tab, real WASM win/game-over/restart; visible focus and no overflow`);
    await page.close();
  }
  assert.deepEqual(errors, []); console.log('PASS UI navigation browser: no page errors or failed assets; local real WASM only');
} finally { await browser?.close(); await new Promise(resolve => server.close(resolve)); }
