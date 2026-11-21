import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || './bench/node_modules/playwright/index.mjs');
const macChrome = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const browser = await chromium.launch({
  executablePath: process.env.CHROME_PATH || (existsSync(macChrome) ? macChrome : undefined),
  args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--autoplay-policy=no-user-gesture-required'],
});
const errors = [];
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
  await page.goto(process.env.GAME_URL || 'http://127.0.0.1:8769/audio_garden/');
  await page.getByRole('button', { name: 'Spielen', exact: true }).click();
  await page.waitForFunction(() => window.__aiAudio?.stats().audioVoices > 0 && window.__aiAudio.stats().audioBufferBytes > 0 && window.__aiAudio.stats().musicLayers.length === 1);
  const initial = await page.evaluate(() => window.__aiAudio.stats());
  assert.equal(initial.audioReverbZone, null);
  assert.deepEqual(initial.musicLayers, ['base']);
  const panners = await page.evaluate(() => [...window.__aiAudio.engine.voices.values()].filter(v => v.panner).map(v => v.panner.panningModel));
  assert.ok(panners.length >= 4, 'synth and bundled WAV spatial voices must be live');
  assert.ok(panners.every(model => model === 'HRTF'));
  console.log(`PASS start: ${initial.audioVoices} voices, ${initial.audioBufferBytes} decoded bytes, ${panners.length} HRTF panners, no zone, base music`);
  await page.keyboard.down('KeyW');
  try {
    await page.waitForFunction(() => window.__aiAudio.stats().audioReverbZone === 'cave' && window.__aiAudio.stats().musicLayers.length === 2, null, { timeout: 30000 });
  } finally { await page.keyboard.up('KeyW'); }
  const cave = await page.evaluate(() => ({ stats: window.__aiAudio.stats(), player: window.__engineStats.playerPosition, state: window.__aiAudio.engine.state }));
  assert.ok(cave.player[2] < -4, 'keyboard must actually move the simulated player');
  assert.equal(cave.state['Höhle'], 1, 'Rhai must set the cave state');
  assert.deepEqual(cave.stats.musicLayers, ['base', 'cave']);
  assert.equal(cave.stats.audioReverbZone, 'cave');
  assert.ok(await page.evaluate(() => window.__aiAudio.engine.reverbs.get('cave').input.buffer.duration > 0));
  assert.deepEqual(errors, [], 'no console errors or unhandled exceptions');
  const shot = path.join(root, 'docs/web_audio_garden.png');
  await page.screenshot({ path: shot });
  console.log(`PASS keyboard cave: position=${cave.player.map(v => v.toFixed(2))}, zone=${cave.stats.audioReverbZone}, music=${cave.stats.musicLayers.join(',')}, no console errors`);
  console.log(`Screenshot: ${shot}`);
} finally { await browser.close(); }
