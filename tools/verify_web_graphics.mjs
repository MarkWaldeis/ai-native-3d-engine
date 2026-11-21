import assert from 'node:assert/strict';
import { installClock } from './lantern_maze/browser.mjs';

export async function verifyGraphics(browser, base, errors) {
  const page = await browser.newPage({ viewport: { width: 800, height: 550 } });
  page.on('pageerror', error => errors.push(`graphics: ${error.message}`));
  page.on('console', message => {
    if (message.type() === 'error' && /THREE|WebGL/.test(message.text())) errors.push(`graphics: ${message.text()}`);
  });
  await page.addInitScript(installClock);
  try {
    for (const [query, mode, level] of [['ultra', 'hdr', '2'], ['low', 'standard', '0'], ['legacy', 'legacy', '2']]) {
      await page.goto(`${base}/coin_game/?quality=${query}`);
      await page.waitForFunction(() => !document.getElementById('start').disabled || document.getElementById('modal').dataset.screen === 'error', null, { polling: 50 });
      assert.equal(await page.locator('#start').isEnabled(), true, await page.locator('#modal-text').textContent());
      const actual = await page.evaluate(() => {
        advanceFrames(1);
        const canvas = document.getElementById('game');
        return [canvas.dataset.graphics, canvas.dataset.quality, window.__engineStats.drawCalls];
      });
      assert.deepEqual(actual.slice(0, 2), [mode, level]);
      assert.ok(actual[2] > 0, `the exported ${query} player renders a frame`);
    }
    const result = await page.evaluate(async () => {
      const THREE = await import('three');
      const { HDRPipeline } = await import('./hdr.js');
      const scene = new THREE.Scene();
      const camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 10);
      camera.position.z = 2;
      const material = new THREE.MeshStandardMaterial({ color: 0x000000, emissive: 0xff4200, emissiveIntensity: 6 });
      const geometry = new THREE.PlaneGeometry(0.25, 0.25);
      scene.add(new THREE.Mesh(geometry, material));
      const renderer = new THREE.WebGLRenderer({ canvas: document.createElement('canvas'), antialias: false });
      renderer.toneMapping = THREE.ACESFilmicToneMapping;
      const pipeline = new HDRPipeline(renderer, { bloomStrength: 0.75, bloomThreshold: 1.1 });
      const gl = renderer.getContext();
      const pixels = new Uint8Array(4);
      function sample(level, size, intensity) {
        material.emissiveIntensity = intensity;
        pipeline.resize(level, size, size, 1);
        pipeline.render(scene, camera);
        gl.readPixels(Math.round(size * 0.574), size / 2, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
        return { red: pixels[0], hdr: !!pipeline.sceneTarget, calls: renderer.info.render.calls };
      }
      try {
        const high = sample(2, 192, 6), low = sample(0, 192, 6), dim = sample(1, 128, 0.1);
        pipeline.release(); pipeline.supported = false;
        const unsupported = sample(2, 192, 6);
        return { high, low, dim, unsupported, glError: gl.getError() };
      } finally {
        pipeline.release(); geometry.dispose(); material.dispose(); renderer.dispose(); renderer.forceContextLoss();
      }
    });
    assert.equal(result.glError, 0, 'postprocessing must not generate WebGL errors');
    assert.ok(result.high.hdr && result.high.calls > result.low.calls && result.high.red > 15, JSON.stringify(result));
    assert.equal(result.low.red, 0, 'no halo outside the geometry when rendering directly');
    assert.ok(result.dim.hdr && result.dim.red <= 1, 'dim objects should not bloom');
    assert.deepEqual(result.unsupported, result.low, 'no float attachment should fall back to direct rendering');
    console.log('PASS graphics: exported presets, linear HDR bloom threshold, resize and direct-render fallback');
  } finally {
    await page.close();
  }
}
