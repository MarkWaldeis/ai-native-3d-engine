import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import http from 'node:http';
import path from 'node:path';
import { installClock } from './lantern_maze/browser.mjs';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const site = path.resolve(process.argv[2] || 'target/sdf-site'), shots = path.resolve('target/sdf-shots');
await fs.mkdir(shots, { recursive: true });
const instrument = `
const updateLabel=SdfLabel.prototype.update, disposeLabel=SdfLabel.prototype.dispose;
window.sdfLabels=new Set();
SdfLabel.prototype.update=function(camera){
  window.sdfLabels.add(this); window.sdfCamera=camera;
  if(window.sdfCloseup){const p=window.sdfCloseup.getWorldPosition(new THREE.Vector3());camera.position.copy(p).add(new THREE.Vector3(0,0.6,4));camera.lookAt(p);}
  return updateLabel.call(this,camera);
};
SdfLabel.prototype.dispose=function(){window.sdfLabels.delete(this);return disposeLabel.call(this);};
`;
const server = http.createServer(async (req, res) => {
  try {
    let file = path.resolve(site, '.' + decodeURIComponent(new URL(req.url, 'http://localhost').pathname));
    if (!file.startsWith(site + path.sep)) throw new Error('outside site');
    if ((await fs.stat(file)).isDirectory()) file = path.join(file, 'index.html');
    res.setHeader('Content-Type', ({ '.wasm': 'application/wasm', '.js': 'text/javascript', '.json': 'application/json', '.html': 'text/html', '.css': 'text/css' })[path.extname(file)] || 'application/octet-stream');
    if (file === path.join(site, 'coin_game/scene.json')) {
      const scene = JSON.parse(await fs.readFile(file, 'utf8'));
      for (const entity of scene.entities) {
        const tags = entity.tags || [];
        const text = tags.includes('player') ? '{score} / {total:coin} ({left:coin})' : tags.includes('coin') ? 'Münze' : tags.includes('goal') ? 'Ziel\nÄ Ö Ü ß' : null;
        if (text) entity.props = { ...entity.props, ai_text: { text, size: 0.5, color: '#fff', outline_width: 0.15, outline_color: '#102030', ...(tags.includes('goal') ? { offset: [0, 2.5, 0] } : {}) } };
      }
      res.end(JSON.stringify(scene));
    } else if (path.basename(file) === 'sdf-text.js') res.end(await fs.readFile(file, 'utf8') + instrument);
    else res.end(await fs.readFile(file));
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
    page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
    page.on('response', response => { if (response.status() >= 400) errors.push(`${response.status()} ${response.url()}`); });
    await page.route('**/*', route => new URL(route.request().url()).origin === base ? route.continue() : route.abort());
    await page.addInitScript(installClock);
    await page.goto(`${base}/coin_game/`);
    await page.waitForFunction(() => document.getElementById('start')?.disabled === false, null, { polling: 50 });
    await page.evaluate(async () => {
      const { WebGame } = await import('./pkg/engine_web.js');
      const tick = WebGame.prototype.tick;
      WebGame.prototype.tick = function(...args) { const result = tick.apply(this, args); window.snapshot = JSON.parse(result); return result; };
    });
    if (phone) await page.locator('#start').tap(); else await page.locator('#start').click();
    await page.evaluate(() => { advanceFrames(3); window.playerLabel = [...sdfLabels].find(l => l.config.text.includes('{score}')); window.originalGeometry = playerLabel.mesh.geometry; });
    assert.equal(await page.locator('#hud-layout > *').count(), 0, 'SDF counts work without ai_hud');
    assert.deepEqual(await page.evaluate(() => ({ count: __engineStats.sdfLabels, text: playerLabel.text })), { count: 5, text: '0 / 3 (3)' });
    assert.ok(await page.evaluate(() => __engineStats.sdfGlyphs >= 15));
    await page.evaluate(() => advanceFrames(3));
    assert.ok(await page.evaluate(() => originalGeometry === playerLabel.mesh.geometry), 'unchanged text preserves geometry');
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
    assert.equal(score, 1, 'real WASM coin pickup');
    assert.deepEqual(await page.evaluate(() => ({ count: __engineStats.sdfLabels, text: playerLabel.text, live: sdfLabels.size })), { count: 4, text: '1 / 3 (2)', live: 4 });
    assert.ok(await page.evaluate(() => originalGeometry !== playerLabel.mesh.geometry), 'template changes rebuild geometry');
    await page.locator('#pause').click(); await page.evaluate(() => advanceFrames(2));
    const pixels = await page.evaluate(() => {
      const canvas = document.getElementById('game'), copy = document.createElement('canvas');
      copy.width = canvas.width; copy.height = canvas.height; const ctx = copy.getContext('2d');
      const label = playerLabel;
      const pos = label.mesh.geometry.attributes.position;
      let x0 = copy.width, y0 = copy.height, x1 = 0, y1 = 0;
      for (let i = 0; i < pos.count; i++) {
        const p = label.worldPosition.clone().fromBufferAttribute(pos, i).applyMatrix4(label.mesh.matrixWorld).project(sdfCamera);
        const x = (p.x + 1) * copy.width / 2, y = (1 - p.y) * copy.height / 2;
        x0 = Math.min(x0, x); x1 = Math.max(x1, x); y0 = Math.min(y0, y); y1 = Math.max(y1, y);
      }
      x0 = Math.max(0, Math.floor(x0)); y0 = Math.max(0, Math.floor(y0)); x1 = Math.min(copy.width, Math.ceil(x1)); y1 = Math.min(copy.height, Math.ceil(y1));
      if (x1 <= x0 || y1 <= y0) return 0;
      label.mesh.visible = false; advanceFrames(1); ctx.drawImage(canvas, 0, 0);
      const before = ctx.getImageData(x0, y0, x1 - x0, y1 - y0).data;
      label.mesh.visible = true; advanceFrames(1); ctx.drawImage(canvas, 0, 0);
      const after = ctx.getImageData(x0, y0, x1 - x0, y1 - y0).data;
      let changed = 0;
      for (let i = 0; i < before.length; i += 4) if (Math.abs(before[i] - after[i]) + Math.abs(before[i + 1] - after[i + 1]) + Math.abs(before[i + 2] - after[i + 2]) > 60) changed++;
      return changed;
    });
    assert.ok(pixels >= 30, `visible SDF pixels near projected label: ${pixels}`);
    await page.getByRole('button', { name: 'Weiterspielen', exact: true }).click();
    await page.evaluate(() => advanceFrames(1));
    await page.screenshot({ path: path.join(shots, `sdf-${name}.png`) });
    await page.locator('#pause').click();
    if (!phone) {
      await page.evaluate(() => { window.sdfCloseup = [...sdfLabels].find(l => l.config.text.startsWith('Ziel')); document.getElementById('modal').hidden = true; advanceFrames(2); });
      await page.screenshot({ path: path.join(shots, 'sdf-closeup.png') });
      await page.evaluate(() => { window.sdfCloseup = null; document.getElementById('modal').hidden = false; });
    }
    await page.evaluate(() => window.__aiSaveGame());
    await page.getByRole('button', { name: 'Neustart', exact: true }).click(); await page.evaluate(() => advanceFrames(2));
    assert.equal(await page.evaluate(() => __engineStats.sdfLabels), 5);
    assert.equal(await page.evaluate(() => [...sdfLabels].find(l => l.config.text.includes('{score}')).text), '0 / 3 (3)');
    assert.equal(await page.evaluate(() => window.__aiLoadGame()), true);
    await page.evaluate(() => advanceFrames(2));
    assert.equal(await page.evaluate(() => __engineStats.sdfLabels), 4);
    assert.equal(await page.evaluate(() => [...sdfLabels].find(l => l.config.text.includes('{score}')).text), '1 / 3 (2)', 'restored save excludes collected coins without ai_hud');
    console.log(`PASS SDF ${name}: real WASM labels, ${pixels} visible projected text pixels, shared left counts without ai_hud, pickup, removal, stable geometry, restart and save restore`);
    await page.close();
  }
  assert.deepEqual(errors, []); console.log('PASS SDF browser: no console/page errors or failed assets; local assets only');
} finally { await browser?.close(); await new Promise(resolve => server.close(resolve)); }
