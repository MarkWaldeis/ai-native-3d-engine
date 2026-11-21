import assert from 'node:assert/strict';
import { installClock } from './lantern_maze/browser.mjs';

export async function verifyGraphics(browser, base, errors, shots) {
  const page = await browser.newPage({ viewport: { width: 800, height: 550 } });
  page.on('pageerror', error => errors.push(`graphics: ${error.message}`));
  page.on('console', message => {
    if (message.type() === 'error' && /THREE|WebGL/.test(message.text())) errors.push(`graphics: ${message.text()}`);
  });
  await page.addInitScript(installClock);
  const ready = async () => {
    await page.waitForFunction(() => !document.getElementById('start').disabled || document.getElementById('modal').dataset.screen === 'error', null, { polling: 50 });
    assert.equal(await page.locator('#start').isEnabled(), true, await page.locator('#modal-text').textContent());
  };
  try {
    for (const [query, mode, level] of [['ultra', 'hdr', '2'], ['low', 'standard', '0'], ['legacy', 'legacy', '2']]) {
      await page.goto(`${base}/coin_game/?quality=${query}`);
      await ready();
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

    const studio = `${base}/material_studio`;
    const map = `${studio}/assets/environment.hdr`;
    await page.setViewportSize({ width: 1100, height: 750 });
    await page.route('**/*', route => {
      const url = route.request().url();
      if (new URL(url).origin === base) return route.continue();
      errors.push(`graphics: external request ${url}`);
      return route.abort();
    });
    const render = async () => {
      await page.goto(`${studio}/?quality=ultra`);
      await ready();
      await page.locator('#start').click();
      return page.evaluate(() => {
        advanceFrames(2);
        if (document.getElementById('modal').dataset.screen === 'error') throw new Error(document.getElementById('modal-text').textContent);
        const game = document.getElementById('game');
        const gl = game.getContext('webgl2');
        const copy = document.createElement('canvas');
        copy.width = 40; copy.height = 28;
        const ctx = copy.getContext('2d');
        ctx.drawImage(game, 0, 0, copy.width, copy.height);
        return { pixels: Array.from(ctx.getImageData(0, 0, copy.width, copy.height).data), error: gl.getError(), title: document.getElementById('title').textContent };
      });
    };
    const [loaded, reflected] = await Promise.all([page.waitForResponse(map), render()]);
    assert.equal(loaded.status(), 200, 'the exported panorama is loaded from its bundle');
    assert.equal(reflected.title, 'Materiallabor');
    assert.equal(reflected.error, 0);
    const before = await page.evaluate(() => window.__engineStats.playerPosition);
    await page.keyboard.down('KeyD');
    await page.evaluate(() => advanceFrames(20));
    await page.keyboard.up('KeyD');
    const after = await page.evaluate(() => window.__engineStats.playerPosition);
    assert.ok(after?.[0] > before?.[0] + 0.25, await page.locator('#modal-text').textContent());
    await page.screenshot({ path: `${shots}/material-studio.png` });

    const background = `${studio}/scene.json`;
    const showMap = async route => {
      const response = await route.fetch();
      const scene = await response.json();
      scene.entities.find(entity => entity.tags.includes('player')).props.ai_graphics.environment_background = true;
      await route.fulfill({ response, json: scene });
    };
    await page.route(background, showMap);
    const panorama = await render();
    await page.unroute(background, showMap);
    await page.route(map, route => route.abort());
    const fallback = await render();
    assert.equal(panorama.error, 0);
    assert.equal(fallback.error, 0, 'a missing panorama falls back without WebGL errors');
    const delta = (a, b) => a.reduce((sum, color, i) => sum + Math.abs(color - b[i]), 0);
    const top = 40 * 2 * 4;
    assert.ok(delta(panorama.pixels.slice(0, top), reflected.pixels.slice(0, top)) > 300, 'the panorama can be visible behind the scene');
    assert.deepEqual(reflected.pixels.slice(0, top), fallback.pixels.slice(0, top), 'reflection-only HDRI preserves the procedural background');
    assert.ok(delta(reflected.pixels.slice(top), fallback.pixels.slice(top)) > 800, 'the studio map changes the lighting, not just the background');
    console.log('PASS HDRI: offline panorama, reflection-only mode, visible background, movement and missing-map fallback');
  } finally {
    await page.close();
  }
}
