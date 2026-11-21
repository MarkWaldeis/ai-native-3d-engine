import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import http from 'node:http';
import path from 'node:path';
import { installClock } from './lantern_maze/browser.mjs';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const site = path.resolve(process.argv[2] || 'target/hud-site');
const shots = path.resolve('target/hud-shots');
await fs.mkdir(shots, { recursive: true });
const config = { replace_default: true, widgets: [
  { id: 'coins', type: 'counter', anchor: 'top-right', offset: [0, 110], width: 170, text: 'Münzen {score}/{total:coin}', phone: { anchor: 'bottom-left', offset: [0, -10], size: 'sm' } },
  { id: 'collect', type: 'toast', anchor: 'bottom-right', offset: [0, -220], width: 170, bind: 'score', text: '+ Münze! {score}', phone: { offset: [0, -10], size: 'sm' } },
  { id: 'info', type: 'panel', anchor: 'top-left', offset: [0, 140], width: 170, phone: { offset: [0, 180] }, children: [
    { id: 'progress', type: 'bar', bind: 'score', max: 3, text: 'Fortschritt {score|0.0}', color: '#c3f683' },
    { id: 'remaining', type: 'text', text: 'Übrig: {left:coin}', style: 'plain', size: 'sm' },
  ] },
] };
const server = http.createServer(async (req, res) => {
  try {
    let file = path.resolve(site, '.' + decodeURIComponent(new URL(req.url, 'http://localhost').pathname));
    if (!file.startsWith(site + path.sep)) throw new Error('outside site');
    if ((await fs.stat(file)).isDirectory()) file = path.join(file, 'index.html');
    res.setHeader('Content-Type', ({ '.wasm': 'application/wasm', '.js': 'text/javascript', '.json': 'application/json', '.html': 'text/html', '.css': 'text/css', '.glb': 'model/gltf-binary' })[path.extname(file)] || 'application/octet-stream');
    if (file === path.join(site, 'coin_game/scene.json')) {
      const scene = JSON.parse(await fs.readFile(file, 'utf8'));
      const player = scene.entities.find(entity => entity.tags?.includes('player'));
      player.props = { ...player.props, ai_hud: config };
      res.end(JSON.stringify(scene));
    } else res.end(await fs.readFile(file));
  } catch { res.writeHead(404); res.end('Not found'); }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const base = `http://127.0.0.1:${server.address().port}`, errors = [];
let browser;
try {
  browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
  for (const phone of [false, true]) {
    const name = phone ? 'phone-390x844' : 'desktop-1280x800';
    const page = await browser.newPage({ viewport: phone ? { width: 390, height: 844 } : { width: 1280, height: 800 }, hasTouch: phone, isMobile: phone, deviceScaleFactor: 1 });
    page.setDefaultTimeout(120000);
    page.on('pageerror', error => errors.push(error.message));
    page.on('response', response => { if (response.status() >= 400) errors.push(`${response.status()} ${response.url()}`); });
    await page.route('**/*', route => new URL(route.request().url()).origin === base ? route.continue() : route.abort());
    await page.addInitScript(installClock);
    await page.goto(`${base}/coin_game/`);
    await page.waitForFunction(() => document.getElementById('start')?.disabled === false, null, { polling: 50 });
    const coins = page.locator('[data-hud-id="coins"]'), toast = page.locator('[data-hud-id="collect"]');
    assert.equal(await page.locator('#state').isHidden(), true);
    assert.equal(await coins.innerText(), 'Münzen 0/3'); assert.ok(await toast.isHidden());
    assert.ok(await page.locator('[data-hud-id="progress"] .hud-widget-fill').evaluate(node => node.classList.contains('low')));
    await page.evaluate(async () => {
      const { WebGame } = await import('./pkg/engine_web.js');
      const tick = WebGame.prototype.tick;
      WebGame.prototype.tick = function(...args) {
        const result = tick.apply(this, args); window.snapshot = JSON.parse(result); return result;
      };
      window.hudNodes = [...document.querySelectorAll('[data-hud-id]')];
      window.hudMutations = 0;
      new MutationObserver(records => { window.hudMutations += records.length; }).observe(document.getElementById('hud-layout'), { subtree: true, childList: true, characterData: true, attributes: true });
    });
    if (phone) await page.locator('#start').tap(); else await page.locator('#start').click();
    await page.evaluate(() => advanceFrames(3));
    assert.equal(await page.evaluate(() => hudMutations), 0, 'unchanged state must not mutate HUD DOM');
    await page.keyboard.down('KeyD');
    let score = 0, diagonal = false;
    for (let i = 0; i < 300 && !score; i++) {
      const position = await page.evaluate(() => snapshot.transforms.find(e => e.tags.includes('player'))?.transform.position);
      if (position) {
        const need = position[2] < 0.02;
        if (need !== diagonal) { await page.keyboard[need ? 'down' : 'up']('KeyS'); diagonal = need; }
      }
      score = await page.evaluate(() => { advanceFrames(1); return snapshot.state.score; });
    }
    await page.keyboard.up('KeyD'); await page.keyboard.up('KeyS');
    assert.equal(score, 1, 'real keyboard/WASM gameplay must collect a coin');
    assert.equal(await coins.innerText(), 'Münzen 1/3'); assert.equal(await toast.innerText(), '+ Münze! 1'); assert.ok(await toast.isVisible());
    assert.equal(await page.locator('[data-hud-id="remaining"]').innerText(), 'Übrig: 2');
    assert.equal(await page.locator('[data-hud-id="progress"] .hud-widget-track').getAttribute('aria-valuenow'), '33');
    assert.ok(!await page.locator('[data-hud-id="progress"] .hud-widget-fill').evaluate(node => node.classList.contains('low')));
    assert.ok(await page.evaluate(() => hudNodes.every(node => node.isConnected)), 'persistent widget nodes');
    await page.evaluate(() => { window.hudMutations = 0; advanceFrames(3); });
    assert.equal(await page.evaluate(() => hudMutations), 0, 'fractional bar values must not rewrite unchanged styles');
    const problems = await page.evaluate(phone => {
      const failures = [], rects = [...document.querySelectorAll('#hud-layout > [data-hud-id]:not([hidden])')].map(node => ({ id: node.dataset.hudId, r: node.getBoundingClientRect() }));
      const overlap = (a, b) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
      for (const { id, r } of rects) {
        if (r.left < 0 || r.right > innerWidth || r.top < 0 || r.bottom > innerHeight) failures.push(`${id}: outside viewport`);
        for (const control of ['joystick', 'action']) if (overlap(r, document.getElementById(control).getBoundingClientRect())) failures.push(`${id}: overlaps ${control}`);
      }
      for (let i = 0; i < rects.length; i++) for (let j = i + 1; j < rects.length; j++) if (overlap(rects[i].r, rects[j].r)) failures.push('widgets overlap');
      if (document.documentElement.scrollWidth > innerWidth) failures.push('horizontal overflow');
      return failures;
    }, phone);
    assert.deepEqual(problems, []);
    if (phone) assert.ok(await coins.evaluate(node => node.classList.contains('hud-size-sm')));
    await page.screenshot({ path: path.join(shots, `hud-${name}.png`) });
    await page.locator('#pause').click(); await page.evaluate(() => advanceFrames(130));
    assert.ok(await toast.isHidden(), 'toast expires in ~2 seconds even when paused');
    await page.evaluate(() => window.__aiSaveGame());
    await page.getByRole('button', { name: 'Neustart', exact: true }).click(); await page.evaluate(() => advanceFrames(2));
    assert.equal(await coins.innerText(), 'Münzen 0/3'); assert.ok(await toast.isHidden(), 'restart resets toast baseline');
    assert.equal(await page.evaluate(() => window.__aiLoadGame()), true);
    assert.equal(await coins.innerText(), 'Münzen 1/3');
    assert.equal(await page.locator('[data-hud-id="remaining"]').innerText(), 'Übrig: 2', 'restored save excludes previously removed coins');
    assert.ok(await toast.isHidden(), 'save restore establishes a new toast baseline');
    console.log(`PASS HUD ${name}: real WASM coin collection updates counter/bar/tag counts, toast appears/expires, stable nodes, safe layout, restart and save restore`);
    await page.close();
  }
  assert.deepEqual(errors, []); console.log('PASS HUD browser: no page errors or failed asset responses');
} finally { await browser?.close(); await new Promise(resolve => server.close(resolve)); }
