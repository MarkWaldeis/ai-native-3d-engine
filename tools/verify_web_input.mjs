import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import http from 'node:http';
import path from 'node:path';
import { installClock } from './lantern_maze/browser.mjs';

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const site = path.resolve(process.argv[2] || 'target/input-site');
const shots = path.resolve('target/input-shots');
const cases = new Set((process.env.INPUT_CASES || 'coin,fps,phone').split(','));
await fs.mkdir(shots, { recursive: true });
const server = http.createServer(async (req, res) => {
  try {
    let file = path.resolve(site, '.' + new URL(req.url, 'http://localhost').pathname);
    if (!file.startsWith(site + path.sep)) throw new Error('outside site');
    if ((await fs.stat(file)).isDirectory()) file = path.join(file, 'index.html');
    res.setHeader('Content-Type', ({ '.js': 'text/javascript', '.wasm': 'application/wasm', '.html': 'text/html', '.css': 'text/css', '.json': 'application/json', '.svg': 'image/svg+xml' })[path.extname(file)] || 'application/octet-stream');
    res.end(await fs.readFile(file));
  } catch { res.writeHead(404); res.end(); }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const base = `http://127.0.0.1:${server.address().port}`;
let browser;
const errors = [];
try {
  browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
  async function open(game, options = {}) {
    const page = await browser.newPage({ viewport: { width: 1100, height: 800 }, ...options });
    page.setDefaultTimeout(120000);
    page.on('pageerror', error => errors.push(error.message));
    page.on('response', response => { if (response.status() >= 400) errors.push(`${response.status()} ${response.url()}`); });
    await page.addInitScript(installClock);
    await page.addInitScript(() => {
      window.testPad = { connected: true, mapping: 'standard', axes: [0, 0, 0, 0], buttons: Array.from({ length: 17 }, () => ({ pressed: false, value: 0 })) };
      Object.defineProperty(navigator, 'getGamepads', { value: () => [null, window.testPad] });
    });
    await page.goto(`${base}/${game}/`);
    try { await page.waitForFunction(() => document.getElementById('start')?.disabled === false, null, { polling: 50 }); }
    catch (error) {
      await page.screenshot({ path: path.join(shots, `${game}-failure.png`) });
      console.error('Boot failure:', await page.locator('#modal-text').innerText(), errors);
      throw error;
    }
    return page;
  }
  const advance = (page, count = 1) => page.evaluate(n => advanceFrames(n), count);
  async function button(page, index, pressed) {
    await page.evaluate(({ index, pressed }) => { testPad.buttons[index] = { pressed, value: Number(pressed) }; advanceFrames(1); }, { index, pressed });
  }
  if (cases.has('coin')) {
  const page = await open('coin_game');
  await button(page, 0, true);
  await page.locator('#modal').waitFor({ state: 'hidden' });
  await button(page, 0, false); await advance(page, 60);
  const before = await page.evaluate(() => __engineStats.playerPosition);
  await page.evaluate(() => { testPad.axes[0] = 0.8; advanceFrames(35); });
  const after = await page.evaluate(() => __engineStats.playerPosition);
  assert.ok(Math.hypot(after[0] - before[0], after[2] - before[2]) > 0.5, 'left stick moves exported coin player');
  await page.evaluate(() => { testPad.axes[0] = 0; });
  assert.match(await page.locator('#help').innerText(), /Gamepad verbunden/);
  await button(page, 9, true); assert.ok(await page.locator('#modal').isVisible());
  await advance(page, 10); assert.ok(await page.locator('#modal').isVisible(), 'held Start cannot toggle repeatedly');
  await button(page, 9, false); await button(page, 0, true);
  assert.ok(await page.locator('#modal').isHidden()); await button(page, 0, false);
  console.log('PASS input coin_game: fake standard gamepad A starts/resumes, left stick moves player, Start pauses once per press');

  await page.locator('#input-settings').click();
  assert.ok(await page.locator('#modal').isVisible(), 'settings pauses running game');
  await page.locator('[data-action="action"]').click(); await page.keyboard.press('KeyE');
  assert.match(await page.locator('#input-bindings li').filter({ has: page.locator('[data-action="action"]') }).innerText(), /Taste E/);
  await page.screenshot({ path: path.join(shots, 'steuerung-desktop.png') });
  await page.reload(); await page.waitForFunction(() => document.getElementById('start')?.disabled === false, null, { polling: 50 });
  assert.match(await page.locator('#help').innerText(), /Taste E: Aktion/, 'loaded bindings refresh keyboard help');
  await page.locator('#input-settings').click();
  assert.match(await page.locator('#input-bindings li').filter({ has: page.locator('[data-action="action"]') }).innerText(), /Taste E/);
  await page.locator('[data-action="fire"]').click(); await page.keyboard.press('Escape');
  assert.ok(await page.locator('#input-panel').isVisible(), 'Escape cancels capture, not dialog');
  await page.locator('[data-action="reload"]').click(); await page.evaluate(() => { testPad.buttons[3] = { pressed: true, value: 1 }; advanceFrames(1); });
  assert.match(await page.locator('#input-bindings li').filter({ has: page.locator('[data-action="reload"]') }).innerText(), /Gamepad Y/);
  await page.evaluate(() => { testPad.buttons[3] = { pressed: false, value: 0 }; });
  await page.locator('[data-action="sprint"]').click(); await page.evaluate(() => { testPad.axes[0] = -0.8; advanceFrames(1); });
  assert.match(await page.locator('#input-bindings li').filter({ has: page.locator('[data-action="sprint"]') }).innerText(), /Linker Stick ←/);
  await page.locator('#input-close').click();
  assert.ok(await page.locator('#modal').evaluate(element => element.contains(document.activeElement)), 'closing settings restores focus inside the pause overlay');
  console.log('PASS input remapping: KeyE survives reload; Escape cancels; gamepad button/axis capture; focus restored');
  await page.evaluate(() => { testPad.axes[0] = 0; });
  await page.locator('#start').click();
  assert.equal(await page.evaluate(() => document.activeElement?.tagName === 'BUTTON'), false, 'starting releases UI button focus');
  await button(page, 9, true); await button(page, 9, false);
  await page.locator('#input-settings').click(); await page.locator('#input-close').click();
  await button(page, 9, true); await button(page, 9, false);
  assert.equal(await page.evaluate(() => document.activeElement?.tagName === 'BUTTON'), false, 'gamepad resume releases footer focus');
  await page.keyboard.press('Space'); assert.ok(await page.locator('#input-panel').isHidden());
  await page.locator('#input-settings').focus();
  for (const key of ['Space', 'Enter']) {
    await page.keyboard.press(key); assert.ok(await page.locator('#input-panel').isHidden(), `${key} must not activate footer while running`);
  }
  console.log('PASS input focus: start/gamepad resume blur UI buttons; unbound Space and Enter cannot reopen settings during gameplay');
  await page.close();
  }

  if (cases.has('fps')) {
  const fps = await open('fps_arena');
  await button(fps, 0, true); await button(fps, 0, false);
  const initialLook = await fps.evaluate(() => ({ ...__aiEngineFps.look }));
  await fps.evaluate(() => { testPad.axes[2] = 0.8; testPad.axes[3] = -0.7; advanceFrames(30); });
  const look = await fps.evaluate(() => ({ ...__aiEngineFps.look }));
  assert.ok(Math.abs(look.yaw - initialLook.yaw) > 0.1 && Math.abs(look.pitch - initialLook.pitch) > 0.1);
  await fps.screenshot({ path: path.join(shots, 'fps-gamepad-look.png') });
  console.log('PASS input fps_arena: right stick accumulates absolute yaw and pitch in real WASM player');
  await fps.close();
  }

  if (cases.has('phone')) {
  const phone = await open('coin_game', { viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
  await phone.locator('#input-settings').tap();
  assert.ok(await phone.locator('#input-panel').evaluate(element => element.scrollWidth <= element.clientWidth && element.getBoundingClientRect().right <= innerWidth));
  await phone.screenshot({ path: path.join(shots, 'steuerung-phone-390x844.png') });
  await phone.locator('#input-close').tap(); await button(phone, 0, true); await button(phone, 0, false); await advance(phone, 60);
  await phone.evaluate(async () => {
    const { WebGame } = await import('./pkg/engine_web.js');
    const tick = WebGame.prototype.tick;
    WebGame.prototype.tick = function(inputs) { window.lastInputs = JSON.parse(inputs); return tick.call(this, inputs); };
  });
  const stick = await phone.locator('#joystick').boundingBox(), action = await phone.locator('#action').boundingBox();
  const cdp = await phone.context().newCDPSession(phone);
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [
    { x: stick.x + stick.width * 0.8, y: stick.y + stick.height / 2, id: 1 },
    { x: action.x + action.width / 2, y: action.y + action.height / 2, id: 2 },
  ] });
  await advance(phone, 2);
  const touch = await phone.evaluate(() => lastInputs);
  assert.ok(Math.hypot(touch.move_x, touch.move_z) > 0.1 && touch.action === 1 && touch.jump === 1, 'simultaneous joystick and action reach WASM');
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] }); await advance(phone, 2);
  assert.equal(await phone.evaluate(() => lastInputs.action), 0);
  console.log('PASS input phone: 390x844 dialog has no horizontal overflow; simultaneous real touch joystick + action, release clears action');
  await phone.close();
  }
  assert.deepEqual(errors, []);
  console.log('PASS input browser: no page errors or failed asset responses');
} finally { await browser?.close(); await new Promise(resolve => server.close(resolve)); }
