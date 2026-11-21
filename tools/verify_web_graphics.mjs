import assert from 'node:assert/strict';
import { installClock } from './lantern_maze/browser.mjs';

export async function verifyMobileGraphics(browser, base, profile, device, errors, shots) {
  const page = await browser.newPage({...device, deviceScaleFactor:1});
  const studio = `${base}/material_studio`;
  page.on('pageerror', error => errors.push(`${profile}/graphics: ${error.message}`));
  page.on('console', message => {
    if (message.type() === 'error' && /THREE|WebGL/.test(message.text())) errors.push(`${profile}/graphics: ${message.text()}`);
  });
  page.on('response', response => {
    if (response.status() >= 400) errors.push(`${profile}/graphics: ${response.status()} ${response.url()}`);
  });
  await page.route('**/*', route => {
    const url = route.request().url();
    if (new URL(url).origin === base) return route.continue();
    errors.push(`${profile}/graphics: external request ${url}`);
    return route.abort();
  });
  await page.addInitScript(installClock);
  try {
    for (const preset of ['low', 'ultra']) {
      await page.goto(`${studio}/?quality=${preset}`);
      await page.waitForFunction(() => !document.querySelector('#start').disabled, null, {polling:50});
      await page.getByRole('button', {name:'Spielen'}).tap();
      const actual = await page.evaluate(() => {
        advanceFrames(2);
        const canvas = document.getElementById('game'), gl = canvas.getContext('webgl2');
        return {mode:canvas.dataset.graphics, quality:canvas.dataset.quality,
          calls:window.__engineStats.drawCalls, error:gl.getError(),
          floatTargets:!!gl.getExtension('EXT_color_buffer_float')};
      });
      assert.equal(actual.quality, preset === 'low' ? '0' : '2');
      assert.ok(actual.calls > 0 && actual.error === 0, JSON.stringify(actual));
      if (preset === 'low' || !actual.floatTargets) assert.equal(actual.mode, 'standard');
      else assert.ok(['hdr', 'standard'].includes(actual.mode), JSON.stringify(actual));
      if (preset === 'ultra') {
        await page.screenshot({path:`${shots}/material-studio-${profile.replaceAll(' ','-')}.png`});
        console.log(`PASS webkit/${profile}/graphics: playable material studio, Ultra ${actual.mode}, no WebGL errors`);
      }
    }
  } finally { await page.close(); }
}

export async function verifyGraphics(browser, base, errors, shots) {
  const page = await browser.newPage({ viewport: { width: 800, height: 550 } });
  page.on('pageerror', error => errors.push(`graphics: ${error.message}`));
  page.on('console', message => {
    if (message.type() === 'error' && /THREE|WebGL/.test(message.text())) errors.push(`graphics: ${message.text()}`);
  });
  await page.addInitScript(installClock);
  await page.addInitScript(() => {
    const proto = WebGL2RenderingContext.prototype;
    const setParameter = proto.texParameterf;
    const getExtension = proto.getExtension;
    window.gpuTimerRequests = 0;
    proto.getExtension = function(name) {
      if (name === 'EXT_disjoint_timer_query_webgl2') {
        window.gpuTimerRequests++;
        if (new URLSearchParams(location.search).has('no_timer')) return null;
      }
      return getExtension.call(this, name);
    };
    window.textureAnisotropy = [];
    proto.texParameterf = function(target, parameter, value) {
      if (parameter === 0x84fe) window.textureAnisotropy.push(value);
      return setParameter.call(this, target, parameter, value);
    };
  });
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
        return [canvas.dataset.graphics, canvas.dataset.quality, window.__engineStats.drawCalls, window.gpuTimerRequests, canvas.dataset.depth];
      });
      assert.deepEqual(actual.slice(0, 2), [mode, level]);
      assert.ok(actual[2] > 0, `the exported ${query} player renders a frame`);
      assert.equal(actual[3], 0, 'default player does not request GPU timing');
      assert.equal(actual[4], 'standard');
    }

    const scaledScene = '**/coin_game/scene.json';
    const scale = async route => {
      const response = await route.fetch(), scene = await response.json();
      const player = scene.entities.find(entity => entity.tags?.includes('player'));
      player.props ??= {};
      player.props.ai_graphics = {render_scale:0.75};
      await route.fulfill({response,json:scene});
    };
    const bufferSize = () => page.evaluate(() => {
      advanceFrames(1);
      const canvas = document.getElementById('game'), gl = canvas.getContext('webgl2');
      return [canvas.dataset.graphics, canvas.dataset.quality, canvas.clientWidth, canvas.clientHeight,
        gl.drawingBufferWidth, gl.drawingBufferHeight, gl.getError()];
    });
    await page.route(scaledScene, scale);
    try {
      for (const [quality, expected] of [['ultra',['hdr','2',800,550,600,412,0]],
        ['low',['standard','0',800,550,480,330,0]], ['legacy',['legacy','2',800,550,800,550,0]]]) {
        await page.goto(`${base}/coin_game/?quality=${quality}`);
        await ready();
        assert.deepEqual(await bufferSize(),expected);
      }
      await page.goto(`${base}/coin_game/?quality=ultra`);
      await ready();
      await page.setViewportSize({width:880,height:600});
      await page.waitForFunction(() => document.getElementById('game').width === 660, null, { polling: 50 });
      assert.deepEqual(await bufferSize(),['hdr','2',880,600,660,450,0]);
    } finally {
      await page.unroute(scaledScene, scale);
      await page.setViewportSize({width:800,height:550});
    }
    console.log('PASS render scale: fewer pixels in HDR and direct modes without resizing the HUD or changing presets; legacy unchanged');

    await page.goto(`${base}/coin_game/?quality=ultra&stats=1&no_timer`);
    await ready();
    const timing = await page.evaluate(() => {
      advanceFrames(1);
      const {renderCpuMs, renderGpuMs} = window.__engineStats;
      return {renderCpuMs, renderGpuMs, requests:window.gpuTimerRequests, text:document.getElementById('render-stats').textContent};
    });
    assert.ok(Number.isFinite(timing.renderCpuMs) && timing.renderCpuMs >= 0, JSON.stringify(timing));
    assert.equal(timing.renderGpuMs, null);
    assert.equal(timing.requests, 1);
    assert.match(timing.text, /CPU [\d.]+ ms · GPU —$/);
    console.log('PASS render timing: opt-in CPU submission and GPU fallback');
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
      const noBloom = new HDRPipeline(renderer, { bloomStrength: 0, bloomThreshold: 1.1 });
      const gl = renderer.getContext();
      const pixels = new Uint8Array(4);
      function sample(level, size, intensity) {
        material.emissiveIntensity = intensity;
        pipeline.resize(level, size, size, 1);
        pipeline.render(scene, camera);
        gl.readPixels(Math.round(size * 0.574), size / 2, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
        return { red: pixels[0], hdr: !!pipeline.sceneTarget, calls: renderer.info.render.calls };
      }
      function halo(ratio, level, intensity) {
        pipeline.scatterRatio = ratio;
        const center = sample(level, 192, intensity);
        gl.readPixels(123, 96, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
        return { far: pixels[0], size: pipeline.wide?.width ?? 0, calls: center.calls };
      }
      function edgeCoverage(filter, level) {
        pipeline.edgeAA = filter;
        pipeline.resize(level, 192, 192, 1);
        pipeline.render(scene, camera);
        const image = new Uint8Array(192 * 192 * 4);
        gl.readPixels(0, 0, 192, 192, gl.RGBA, gl.UNSIGNED_BYTE, image);
        let intermediate = 0;
        const center = image[(96 * 192 + 96) * 4];
        for (let i = 0; i < image.length; i += 4) {
          if (image[i] > 8 && image[i] < center - 8) intermediate++;
        }
        return { intermediate, center };
      }
      function filmic(level, mapping) {
        renderer.toneMapping = mapping;
        noBloom.resize(level, 192, 192, 1);
        noBloom.render(scene, camera);
        gl.readPixels(96, 96, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
        return { rgb: Array.from(pixels.slice(0, 3)), hdr: !!noBloom.sceneTarget,
          bloom: !!noBloom.bright, calls: renderer.info.render.calls };
      }
      try {
        const high = sample(2, 192, 6), low = sample(0, 192, 6), dim = sample(1, 128, 0.1);
        const tight = halo(0, 2, 6), wide = halo(0.65, 2, 6), wideDim = halo(0.65, 2, 0.1);
        const balancedWide = halo(0.65, 1, 6), balancedTight = halo(0, 1, 6);
        const panel = scene.children[0];
        panel.rotation.z = 0.41;
        panel.scale.setScalar(5);
        material.emissive.setHex(0xffffff);
        material.emissiveIntensity = 0.7;
        const sharp = edgeCoverage(false, 2), smoothed = edgeCoverage(true, 2), balanced = edgeCoverage(true, 1);
        panel.rotation.z = 0;
        panel.scale.setScalar(1);
        material.emissive.setHex(0xff4200);
        pipeline.release(); pipeline.supported = false;
        const unsupported = sample(2, 192, 6);
        const aces = filmic(2, THREE.ACESFilmicToneMapping);
        const agx = filmic(2, THREE.AgXToneMapping);
        const directAgx = filmic(0, THREE.AgXToneMapping);
        return { high, low, dim, tight, wide, wideDim, balancedWide, balancedTight,
          sharp, smoothed, balanced, unsupported, aces, agx, directAgx, glError: gl.getError() };
      } finally {
        noBloom.release(); pipeline.release(); geometry.dispose(); material.dispose(); renderer.dispose(); renderer.forceContextLoss();
      }
    });
    assert.equal(result.glError, 0, 'postprocessing must not generate WebGL errors');
    assert.ok(result.high.hdr && result.high.calls > result.low.calls && result.high.red > 15, JSON.stringify(result));
    assert.equal(result.low.red, 0, 'no halo outside the geometry when rendering directly');
    assert.ok(result.dim.hdr && result.dim.red <= 1, 'dim objects should not bloom');
    assert.equal(result.tight.size, 0);
    assert.equal(result.wide.size, 24, 'scatter uses one-eighth resolution per axis on Ultra');
    assert.equal(result.wide.calls, result.tight.calls + 1, 'scatter uses one additional pass');
    assert.ok(result.wide.far > result.tight.far + 15 && result.wideDim.far <= 1, JSON.stringify(result));
    assert.deepEqual(result.balancedWide, result.balancedTight, 'balanced ignores scatter and frees its target');
    assert.deepEqual(result.unsupported, result.low, 'no float attachment should fall back to direct rendering');
    assert.ok(result.smoothed.intermediate > result.sharp.intermediate + 100, JSON.stringify(result));
    assert.equal(result.smoothed.center, result.sharp.center, 'edge filtering leaves the uniform interior unchanged');
    assert.deepEqual(result.balanced, result.sharp, 'the balanced tier does not run edge filtering');
    assert.ok(result.agx.hdr && !result.agx.bloom && result.agx.calls === 2, JSON.stringify(result));
    assert.ok(result.agx.rgb.some((channel, i) => Math.abs(channel - result.aces.rgb[i]) > 20), 'AgX changes the highlight response');
    assert.ok(result.agx.rgb.every((channel, i) => Math.abs(channel - result.directAgx.rgb[i]) <= 2), 'direct and HDR compositing apply AgX once');
    console.log('PASS graphics: exported presets, HDR bloom and wide Ultra halo, edge smoothing, resize and direct-render fallback');

    const exposure = await page.evaluate(async () => {
      const THREE = await import('three');
      const {HDRPipeline} = await import('./hdr.js');
      const {graphicsConfig} = await import('./graphics-settings.js');
      const renderer = new THREE.WebGLRenderer({canvas:document.createElement('canvas')});
      renderer.toneMapping = THREE.ACESFilmicToneMapping;
      const gl = renderer.getContext();
      const scene = new THREE.Scene();
      const camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 10);
      camera.position.z = 2;
      const material = new THREE.MeshBasicMaterial();
      const geometry = new THREE.PlaneGeometry(2, 2);
      scene.add(new THREE.Mesh(geometry, material));
      const config = graphicsConfig({bloom_strength:0, hdr_antialias:false, auto_exposure:true}, 'balanced');
      const pipeline = new HDRPipeline(renderer, config);
      const fallback = new HDRPipeline(renderer, {...config, bloomStrength:0.2});
      const pixels = new Uint8Array(4);
      const read = () => {
        gl.readPixels(16, 16, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
        return pixels[0];
      };
      const render = (level, luma, frames = 1) => {
        pipeline.resize(level, 32, 32, 1);
        material.color.setRGB(luma, luma, luma);
        for (let i = 0; i < frames; ++i) pipeline.render(scene, camera, 0.1);
        return read();
      };
      try {
        const bright = render(1, 1.5);
        const metered = !!pipeline.exposureMeter && !pipeline.bright && renderer.info.render.calls === 3;
        const enteringShade = render(1, 0.06);
        const adaptedShade = render(1, 0.06, 40);
        const leavingShade = render(1, 1.5);
        const adaptedBright = render(1, 1.5, 24);
        const fixedShade = render(0, 0.06);
        const released = !pipeline.sceneTarget && !pipeline.exposureMeter;
        const restoredShade = render(1, 0.06);
        const original = gl.checkFramebufferStatus;
        let checks = 0;
        gl.checkFramebufferStatus = function(target) {
          return ++checks === 2 ? gl.FRAMEBUFFER_UNSUPPORTED : original.call(this, target);
        };
        try { fallback.resize(1, 32, 32, 1); }
        finally { gl.checkFramebufferStatus = original; }
        return {
          bright, enteringShade, adaptedShade, leavingShade, adaptedBright, fixedShade, restoredShade, metered,
          released, bloomFallback:!!fallback.bright && !fallback.exposureMeter, glError:gl.getError(),
        };
      } finally {
        pipeline.release(); fallback.release(); geometry.dispose(); material.dispose();
        renderer.dispose(); renderer.forceContextLoss();
      }
    });
    assert.ok(exposure.metered && exposure.released && exposure.bloomFallback && exposure.glError === 0, JSON.stringify(exposure));
    assert.ok(exposure.enteringShade + 30 < exposure.adaptedShade && exposure.fixedShade + 35 < exposure.adaptedShade, JSON.stringify(exposure));
    assert.ok(exposure.leavingShade > exposure.bright + 20, JSON.stringify(exposure));
    assert.ok(Math.abs(exposure.adaptedBright - exposure.bright) <= 3 && Math.abs(exposure.restoredShade - exposure.adaptedShade) <= 6, JSON.stringify(exposure));
    console.log('PASS auto exposure: gradual bright/dark adaptation, fixed Low exposure and independent bloom fallback');

    const coinScene = `${base}/coin_game/scene.json`;
    const authoredExposure = async route => {
      const response = await route.fetch();
      const scene = await response.json();
      scene.entities.find(entity => entity.tags.includes('player')).props.ai_graphics = {
        auto_exposure:true, bloom_strength:0, hdr_antialias:false,
      };
      await route.fulfill({response, json:scene});
    };
    await page.route(coinScene, authoredExposure);
    try {
      await page.goto(`${base}/coin_game/?quality=balanced`);
      await ready();
      const mode = await page.evaluate(() => {
        advanceFrames(1);
        return document.querySelector('#game').dataset.graphics;
      });
      assert.equal(mode, 'hdr', 'authored auto exposure enables HDR without bloom or AA');
    } finally { await page.unroute(coinScene, authoredExposure); }

    const clouds = await page.evaluate(async () => {
      const THREE = await import('three');
      const {applySky} = await import('./sky.js');
      const {graphicsConfig} = await import('./graphics-settings.js');
      const renderer = new THREE.WebGLRenderer({canvas:document.createElement('canvas')});
      const gl = renderer.getContext(), pixels = new Uint8Array(96 * 64 * 4);
      const camera = new THREE.PerspectiveCamera(65, 1.5, 0.1, 50);
      const geometry = new THREE.SphereGeometry(0.5, 24, 16);
      const material = new THREE.MeshStandardMaterial({metalness:1, roughness:0.13, color:0xffffff});
      const mirror = new THREE.Mesh(geometry, material);
      const scene = new THREE.Scene();
      scene.add(mirror);
      renderer.setSize(96, 64, false);
      camera.position.set(0, 0, 2);
      const render = () => {
        renderer.render(scene, camera);
        gl.readPixels(0, 0, 96, 64, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
        return Array.from(pixels);
      };
      const capture = sky_clouds => {
        const config = graphicsConfig({sky_clouds, sun_direction:[1,1,0]}, 'ultra');
        applySky(renderer, scene, config, new THREE.Vector3(...config.sunDirection));
        const sky = scene.background;
        try {
          mirror.visible = false;
          camera.lookAt(1, 1, 2);
          const background = render();
          scene.background = null;
          camera.lookAt(0, 0, 0);
          mirror.visible = true;
          return {background, reflected:render()};
        } finally {
          sky.dispose(); scene.environment.dispose();
          scene.background = scene.environment = null;
        }
      };
      try {
        const clear = capture(false), cloudy = capture({coverage:0.7, seed:123});
        const difference = key => clear[key].reduce((sum, value, i) => sum + Math.abs(value - cloudy[key][i]), 0);
        return {skyDelta:difference('background'), reflectionDelta:difference('reflected'), error:gl.getError()};
      } finally {
        material.dispose(); geometry.dispose(); renderer.dispose(); renderer.forceContextLoss();
      }
    });
    assert.ok(clouds.skyDelta > 10000 && clouds.reflectionDelta > 2000 && clouds.error === 0, JSON.stringify(clouds));
    console.log('PASS clouds: generated sky and its reflection change without external assets');

    const renderContact = logarithmicDepthBuffer => page.evaluate(async logarithmicDepthBuffer => {
      const THREE = await import('three');
      const { HDRPipeline } = await import('./hdr.js');
      const scene = new THREE.Scene();
      scene.background = new THREE.Color(0x436b93);
      const camera = new THREE.PerspectiveCamera(46, 1, 0.05, 150);
      camera.position.set(3, 2.8, 4);
      camera.lookAt(0, 0.1, 0);
      const gray = new THREE.MeshBasicMaterial({color: 0xaaaaaa});
      const floor = new THREE.Mesh(new THREE.PlaneGeometry(7, 7), gray);
      floor.rotation.x = -Math.PI / 2;
      const block = new THREE.Mesh(new THREE.BoxGeometry(1.5, 1.2, 1.5), gray);
      block.position.y = 0.6;
      scene.add(floor, block);
      const renderer = new THREE.WebGLRenderer({canvas: document.createElement('canvas'), logarithmicDepthBuffer});
      const gl = renderer.getContext();
      const config = {bloomStrength: 0, hdrAntialias: false, contactStrength: 1, contactRadius: 1.25};
      const pipeline = new HDRPipeline(renderer, config);
      try {
        const size = 193;
        const sharp = new Uint8Array(size * size * 4), shaded = new Uint8Array(sharp.length);
        pipeline.resize(1, size, size, 1);
        pipeline.render(scene, camera);
        gl.readPixels(0, 0, size, size, gl.RGBA, gl.UNSIGNED_BYTE, sharp);
        pipeline.resize(2, size, size, 1);
        pipeline.render(scene, camera);
        gl.readPixels(0, 0, size, size, gl.RGBA, gl.UNSIGNED_BYTE, shaded);
        let darkened = 0, flat = 0, sky = 0;
        for (let i = 0; i < sharp.length; i += 4) {
          if (sharp[i] - shaded[i] > 8) darkened++;
          if (sharp[i] > 155 && Math.abs(shaded[i] - sharp[i]) <= 1) flat++;
          if (sharp[i + 2] - sharp[i] > 35 && Math.abs(shaded[i] - sharp[i]) <= 1) sky++;
        }
        const draws = renderer.info.render.calls, half = pipeline.contact.target.width;
        pipeline.resize(0, size, size, 1);
        const released = !pipeline.sceneTarget && !pipeline.contact;
        const check = gl.checkFramebufferStatus.bind(gl);
        let rejectDepth = true;
        gl.checkFramebufferStatus = target => {
          if (rejectDepth) { rejectDepth = false; return gl.FRAMEBUFFER_INCOMPLETE_ATTACHMENT; }
          return check(target);
        };
        const fallback = new HDRPipeline(renderer, {...config, bloomStrength: 0.2});
        try {
          fallback.resize(2, size, size, 1);
          fallback.render(scene, camera);
          return {darkened, flat, sky, half, draws, released,
            fallback: !!fallback.bright && !fallback.contact && !fallback.sceneTarget.depthTexture,
            glError: gl.getError()};
        } finally { fallback.release(); gl.checkFramebufferStatus = check; }
      } finally {
        pipeline.release();
        block.geometry.dispose(); floor.geometry.dispose(); gray.dispose();
        renderer.dispose(); renderer.forceContextLoss();
      }
    }, logarithmicDepthBuffer);
    for (const logarithmic of [false, true]) {
      const contact = await renderContact(logarithmic);
      assert.ok(contact.darkened > 50 && contact.flat > 5000 && contact.sky > 500, JSON.stringify({logarithmic, ...contact}));
      assert.deepEqual([contact.half, contact.draws, contact.released, contact.fallback, contact.glError], [97, 4, true, true, 0]);
    }
    console.log('PASS contact occlusion: standard and logarithmic depth, unchanged open surfaces and sky, and GPU/tier fallback');

    const sceneUrl = `${base}/coin_game/scene.json`;
    const useLogDepth = async route => {
      const response = await route.fetch();
      const scene = await response.json();
      const player = scene.entities.find(entity => entity.tags.includes('player'));
      player.props ??= {};
      player.props.ai_graphics = {logarithmic_depth:true};
      await route.fulfill({response,json:scene});
    };
    await page.route(sceneUrl, useLogDepth);
    try {
      for (const [preset, mode] of [['ultra', 'logarithmic'], ['legacy', 'standard']]) {
        await page.goto(`${base}/coin_game/?quality=${preset}`);
        await ready();
        assert.equal(await page.locator('#game').getAttribute('data-depth'), mode);
      }
    } finally { await page.unroute(sceneUrl, useLogDepth); }
    const depth = await page.evaluate(async () => {
      const THREE = await import('three');
      const {HDRPipeline} = await import('./hdr.js');
      const scene = new THREE.Scene();
      const camera = new THREE.PerspectiveCamera(45, 1, 0.05, 6000);
      const geometry = new THREE.PlaneGeometry(200, 200);
      const near = new THREE.Mesh(geometry, new THREE.MeshBasicMaterial({color:0x00ff00}));
      const far = new THREE.Mesh(geometry, new THREE.MeshBasicMaterial({color:0xff0000}));
      far.renderOrder = 1;
      scene.add(near, far);
      const draw = logarithmicDepthBuffer => {
        const renderer = new THREE.WebGLRenderer({canvas:document.createElement('canvas'), logarithmicDepthBuffer});
        const gl = renderer.getContext(), pixel = new Uint8Array(4);
        const pipeline = new HDRPipeline(renderer, {bloomStrength:0,hdrAntialias:true});
        try {
          return [0, 2].map(tier => {
            pipeline.resize(tier, 64, 64, 1);
            let correct = 0;
            for (let i = 0; i < 12; i++) {
              near.position.z = -2000 - i * 0.37;
              far.position.z = near.position.z - 0.5;
              pipeline.render(scene, camera);
              gl.readPixels(32, 32, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel);
              correct += Number(pixel[1] > pixel[0]);
            }
            return {correct,hdr:!!pipeline.sceneTarget,error:gl.getError()};
          });
        } finally { pipeline.release(); renderer.dispose(); renderer.forceContextLoss(); }
      };
      try { return {standard:draw(false), logarithmic:draw(true)}; }
      finally { geometry.dispose(); near.material.dispose(); far.material.dispose(); }
    });
    assert.deepEqual(depth.logarithmic, [{correct:12,hdr:false,error:0},{correct:12,hdr:true,error:0}]);
    assert.ok(depth.standard.some(sample => sample.correct < 12), JSON.stringify(depth));
    console.log('PASS logarithmic depth: near surface wins at 2 km in direct and HDR rendering');

    const fog = await page.evaluate(async () => {
      const THREE = await import('three');
      const { HDRPipeline } = await import('./hdr.js');
      const { graphicsConfig } = await import('./graphics-settings.js');
      const geometry = new THREE.PlaneGeometry(10, 10);
      const material = new THREE.MeshBasicMaterial({color:'#202020'});
      const mesh = new THREE.Mesh(geometry, material);
      const scene = new THREE.Scene();
      const fallback = new THREE.Fog('#cc4410', 4, 10);
      scene.fog = fallback;
      scene.background = new THREE.Color('#403020');
      scene.add(mesh);
      const config = graphicsConfig({
        height_fog:{density:0.04, falloff:10, color:'#94caff'}, bloom_strength:0,
        hdr_antialias:false,
      }, 'balanced');
      const result = [];
      for (const [logarithmicDepthBuffer, orthographic] of [[false,false],[true,false],[true,true]]) {
        const camera = orthographic
          ? new THREE.OrthographicCamera(-10, 10, 10, -10, 0.1, 200)
          : new THREE.PerspectiveCamera(70, 1, 0.1, 200);
        const renderer = new THREE.WebGLRenderer({canvas:document.createElement('canvas'), logarithmicDepthBuffer});
        renderer.debug.onShaderError = () => { throw new Error('height fog shader did not compile'); };
        const gl = renderer.getContext();
        const pipeline = new HDRPipeline(renderer, config);
        const contact = new HDRPipeline(renderer, {...config, contactStrength:0.8});
        const noDepth = new HDRPipeline(renderer, {...config, bloomStrength:0.2, contactStrength:0.8});
        const pixels = new Uint8Array(4);
        const read = (x,y) => {
          gl.readPixels(x,y,1,1,gl.RGBA,gl.UNSIGNED_BYTE,pixels);
          return Array.from(pixels.slice(0,3));
        };
        const sample = (height, distance, fromHeight = height, source = pipeline) => {
          camera.position.set(0,fromHeight,0);
          camera.lookAt(0,height,-distance);
          mesh.position.set(0,height,-distance);
          mesh.quaternion.copy(camera.quaternion);
          source.render(scene,camera);
          return read(48,48);
        };
        try {
          pipeline.resize(1,96,96,1);
          const near = sample(-1,8);
          const ground = sample(-1,45);
          const sky = read(1,1);
          const elevated = sample(45,45);
          const through = sample(-1,45,45);
          const fogOnly = !!pipeline.sceneTarget && !pipeline.bright && !pipeline.contact;
          contact.resize(2,96,96,1);
          const combined = sample(-1,45,-1,contact);
          pipeline.resize(0,96,96,1);
          const low = sample(-1,45);
          const check = gl.checkFramebufferStatus;
          let rejectDepth = true;
          gl.checkFramebufferStatus = function(target) {
            if (rejectDepth) { rejectDepth = false; return gl.FRAMEBUFFER_INCOMPLETE_ATTACHMENT; }
            return check.call(this,target);
          };
          try { noDepth.resize(2,96,96,1); }
          finally { gl.checkFramebufferStatus = check; }
          const bloomFallback = !!noDepth.bright && !noDepth.sceneTarget.depthTexture && !noDepth.fogEnabled;
          result.push({near,ground,sky,elevated,through,fogOnly,combined,low,bloomFallback,
            restored:scene.fog === fallback,error:gl.getError()});
        } finally { noDepth.release(); contact.release(); pipeline.release(); renderer.dispose(); renderer.forceContextLoss(); }
      }
      geometry.dispose(); material.dispose();
      return result;
    });
    for (const result of fog) {
      const {near,ground,sky,elevated,through,fogOnly,combined,low,bloomFallback,restored,error} = result;
      assert.ok(ground[2] > near[2] + 40 && near[2] > elevated[2] + 35, JSON.stringify(result));
      assert.ok(through[2] > elevated[2] + 30 && through[2] < ground[2] - 15, JSON.stringify(result));
      assert.deepEqual(sky,[64,48,32]);
      assert.ok(combined.every((value,index) => Math.abs(value-ground[index]) <= 2), JSON.stringify(result));
      assert.deepEqual(low,[204,68,16]);
      assert.ok(fogOnly && bloomFallback && restored && error === 0, JSON.stringify(result));
    }
    for (const key of ['near','ground','elevated','through']) {
      assert.ok(fog[0][key].every((value,index) => Math.abs(value-fog[1][key][index]) <= 3), JSON.stringify(fog));
    }
    console.log('PASS height fog: low layers, clear sky, log-depth and contact compatibility, and Low fallback');

    const grades = await page.evaluate(async () => {
      const THREE = await import('three');
      const { HDRPipeline } = await import('./hdr.js');
      const { graphicsConfig } = await import('./graphics-settings.js');
      const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera(55, 1, 0.1, 20);
      const geometry = new THREE.PlaneGeometry(4, 4), material = new THREE.MeshBasicMaterial();
      const patch = new THREE.Mesh(geometry, material);
      patch.position.z = -2;
      scene.add(patch);
      const renderer = new THREE.WebGLRenderer({canvas:document.createElement('canvas')});
      renderer.debug.onShaderError = () => { throw new Error('color grade shader did not compile'); };
      renderer.info.autoReset = false;
      const gl = renderer.getContext(), pixel = new Uint8Array(4);
      const make = color_grade => new HDRPipeline(renderer, graphicsConfig({
        color_grade, bloom_strength:0, hdr_antialias:false,
      }, 'balanced'));
      const neutral = make({saturation:1, contrast:1});
      const grayscale = make({saturation:0});
      const contrast = make({contrast:2});
      const sample = (pipeline, color, tier = 1, gain = 1) => {
        pipeline.resize(tier,64,64,1);
        material.color.set(color).multiplyScalar(gain);
        pipeline.render(scene, camera);
        gl.readPixels(32,32,1,1,gl.RGBA,gl.UNSIGNED_BYTE,pixel);
        return [...pixel.slice(0,3)];
      };
      try {
        return [THREE.ACESFilmicToneMapping, THREE.AgXToneMapping].map(curve => {
          renderer.toneMapping = curve;
          const plain = sample(neutral, '#ef5017');
          const mono = sample(grayscale, '#ef5017');
          const resolvesOnly = !neutral.sceneTarget && !!grayscale.sceneTarget
            && !grayscale.bright && !grayscale.contact && renderer.info.render.calls === 2;
          return {plain, mono, resolvesOnly, low:sample(grayscale, '#ef5017', 0),
            shadow:sample(neutral, '#505050')[0], gradedShadow:sample(contrast, '#505050')[0],
            light:sample(neutral, '#ffffff', 1, 1.6)[0], gradedLight:sample(contrast, '#ffffff', 1, 1.6)[0],
            error:gl.getError()};
        });
      } finally {
        neutral.release(); grayscale.release(); contrast.release();
        geometry.dispose(); material.dispose(); renderer.dispose(); renderer.forceContextLoss();
      }
    });
    for (const result of grades) {
      const {plain, mono, resolvesOnly, low, shadow, gradedShadow, light, gradedLight, error} = result;
      assert.ok(plain[0] - plain[2] > 30 && Math.max(...mono)-Math.min(...mono) <= 1, JSON.stringify(result));
      assert.ok(shadow - gradedShadow > 8 && gradedLight - light > 4, JSON.stringify(result));
      assert.deepEqual(low, plain);
      assert.ok(resolvesOnly && error === 0, JSON.stringify(result));
    }
    console.log('PASS color grade: monochrome and contrast after ACES and AgX; no bloom, Low unchanged');

    const spot = await page.evaluate(async () => {
      const THREE = await import('three');
      const { LocalLights } = await import('./local-lights.js');
      const scene = new THREE.Scene(), lights = new LocalLights(scene);
      const camera = new THREE.OrthographicCamera(-4, 4, 4, -4, 0.1, 20);
      camera.position.z = 10;
      const plane = new THREE.Mesh(new THREE.PlaneGeometry(8, 8), new THREE.MeshStandardMaterial({color: 0xffffff}));
      scene.add(plane);
      const transform = yaw => ({position:[-2,0,5], rotation_deg:[0,yaw,0], scale:[1,1,1]});
      lights.register({id:1, transform:transform(0), props:{ai_light:{
        type:'spot', intensity:80, range:12, inner_angle:8, outer_angle:17,
      }}});
      const renderer = new THREE.WebGLRenderer({canvas: document.createElement('canvas')});
      const gl = renderer.getContext(), pixel = new Uint8Array(4);
      try {
        renderer.setSize(128, 128, false);
        const sample = (yaw, points) => {
          lights.move(1, transform(yaw));
          lights.update(camera.position, 0);
          renderer.render(scene, camera);
          return points.map(x => {
            gl.readPixels(x, 64, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel);
            return pixel[0];
          });
        };
        return {straight: sample(0, [32, 48, 96]), turned: sample(-39, [32, 96]), error: gl.getError()};
      } finally {
        lights.reset(); plane.geometry.dispose(); plane.material.dispose();
        renderer.dispose(); renderer.forceContextLoss();
      }
    });
    assert.ok(spot.straight[0] > spot.straight[1] + 20 && spot.straight[1] > spot.straight[2] + 20, JSON.stringify(spot));
    assert.ok(spot.turned[1] > spot.turned[0] + 40 && spot.error === 0, JSON.stringify(spot));
    console.log('PASS spotlights: rendered soft cone edge follows an emitter rotation');

    const spotShadow = await page.evaluate(async () => {
      const THREE = await import('three');
      const {LocalLights} = await import('./local-lights.js');
      const scene = new THREE.Scene(), camera = new THREE.OrthographicCamera(-4, 4, 4, -4, 0.1, 20);
      camera.position.set(0, 8, 0.001);
      camera.lookAt(0, 0, 0);
      camera.updateMatrixWorld();
      const material = new THREE.MeshStandardMaterial({color:0xbbbbbb, roughness:1});
      const floor = new THREE.Mesh(new THREE.PlaneGeometry(8, 8), material);
      floor.rotation.x = -Math.PI / 2;
      floor.receiveShadow = true;
      const box = new THREE.Mesh(new THREE.BoxGeometry(1.3, 2, 1.3), material);
      box.position.y = 1;
      box.castShadow = true;
      scene.add(floor, box, new THREE.AmbientLight(0xffffff, 0.2));
      const renderer = new THREE.WebGLRenderer({canvas:document.createElement('canvas')});
      renderer.setSize(128, 128, false);
      renderer.shadowMap.enabled = true;
      renderer.shadowMap.type = THREE.PCFSoftShadowMap;
      renderer.toneMapping = THREE.ACESFilmicToneMapping;
      const lights = new LocalLights(scene), gl = renderer.getContext(), pixel = new Uint8Array(4);
      lights.register({id:1, transform:{position:[0,4,3], scale:[1,1,1], rotation_deg:[0,0,0]},
        props:{ai_light:{ type:'spot', direction:[0,-4,-3], intensity:90, range:10,
          outer_angle:40, inner_angle:32, cast_shadow:true }}});
      const sample = level => {
        lights.update(new THREE.Vector3(), level);
        renderer.render(scene, camera);
        return [0, 2].map(x => {
          const p = new THREE.Vector3(x,0,-1.4).project(camera);
          gl.readPixels(Math.floor((p.x + 1) * 64), Math.floor((p.y + 1) * 64), 1,1,gl.RGBA,gl.UNSIGNED_BYTE,pixel);
          return pixel[0];
        });
      };
      try {
        const lit = sample(1), shadow = sample(2), downgraded = sample(1);
        return {lit, shadow, downgraded, error:gl.getError()};
      } finally {
        lights.reset(); material.dispose(); floor.geometry.dispose(); box.geometry.dispose();
        renderer.dispose(); renderer.forceContextLoss();
      }
    });
    assert.ok(spotShadow.lit[0] > spotShadow.shadow[0] + 35, JSON.stringify(spotShadow));
    assert.ok(Math.abs(spotShadow.lit[1] - spotShadow.shadow[1]) < 3, JSON.stringify(spotShadow));
    assert.deepEqual(spotShadow.downgraded, spotShadow.lit);
    assert.equal(spotShadow.error, 0);
    console.log('PASS spotlight shadows: Ultra occludes the cone behind a caster; downgrade keeps the light');

    const shadows = await page.evaluate(async () => {
      const THREE = await import('three');
      const {GameEffects} = await import('./effects.js');
      const {graphicsConfig} = await import('./graphics-settings.js');
      const renderer = new THREE.WebGLRenderer({canvas: document.createElement('canvas')});
      renderer.shadowMap.enabled = true;
      renderer.shadowMap.type = THREE.PCFSoftShadowMap;
      const camera = new THREE.OrthographicCamera(-6, 6, 6, -6, 0.1, 50);
      camera.position.set(18, 20, 0.01);
      camera.lookAt(18, 0, 0);
      camera.updateMatrixWorld();
      const box = new THREE.BoxGeometry(2, 5, 2), floor = new THREE.PlaneGeometry(70, 70);
      const material = new THREE.MeshStandardMaterial({color: 0xc8c8c8});
      const caster = new THREE.Mesh(box, material), receiver = new THREE.Mesh(floor, material);
      caster.position.set(18, 2.5, 0);
      caster.castShadow = receiver.receiveShadow = true;
      receiver.rotation.x = -Math.PI / 2;
      const gl = renderer.getContext(), pixel = new Uint8Array(4);
      const sample = extent => {
        const config = graphicsConfig({shadow_extent:extent, sun_direction:[0,1,1], sun_intensity:5}, 'low');
        const scene = new THREE.Scene(), effects = new GameEffects(renderer, scene, null, config);
        scene.fog = null;
        scene.add(caster, receiver);
        effects.update(0, new Map(), camera, false, 0, new THREE.Vector3());
        effects.render(camera);
        const {x,y} = new THREE.Vector3(18, 0, -3).project(camera);
        gl.readPixels(Math.floor((x + 1) * gl.drawingBufferWidth / 2), Math.floor((y + 1) * gl.drawingBufferHeight / 2), 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel);
        effects.sun.shadow.dispose();
        return pixel[0];
      };
      try { return {normal:sample(24), wide:sample(64), error:gl.getError()}; }
      finally { box.dispose(); floor.dispose(); material.dispose(); renderer.dispose(); renderer.forceContextLoss(); }
    });
    assert.ok(shadows.normal > shadows.wide + 30 && shadows.error === 0, JSON.stringify(shadows));
    console.log('PASS sun shadows: wider authored volume captures a caster outside the default volume');

    const streamedShadow = await page.evaluate(async () => {
      const THREE = await import('three');
      const {worldConfig} = await import('./streaming.js');
      const {StreamingWorld} = await import('./world.js');
      const scene = new THREE.Scene(), size = 96;
      const renderer = new THREE.WebGLRenderer({canvas:document.createElement('canvas')});
      renderer.setSize(size,size,false);
      renderer.shadowMap.enabled = true;
      renderer.shadowMap.type = THREE.PCFSoftShadowMap;
      const gl = renderer.getContext(), pixel = new Uint8Array(4);
      const world = new StreamingWorld(scene,renderer,worldConfig([
        {tags:['player'],props:{ai_world:{chunk_size:2,stream_radius:16}}},
      ]));
      const camera = new THREE.OrthographicCamera(-1.2,1.2,1.2,-1.2,0.1,20);
      camera.position.set(0,8,0); camera.up.set(0,0,-1); camera.lookAt(0,0,0); camera.updateMatrixWorld();
      const sun = new THREE.DirectionalLight(0xffffff,3);
      sun.position.set(6,6,0); sun.castShadow = true;
      scene.add(sun,sun.target,new THREE.AmbientLight(0xffffff,0.2));
      const ground = new THREE.Mesh(new THREE.PlaneGeometry(4,4),new THREE.MeshStandardMaterial());
      ground.rotation.x = -Math.PI/2; ground.receiveShadow = true; scene.add(ground);
      const position = [2.6,2.6,0];
      world.add({id:1,kind:'box',size:[1,1.3,1],transform:{position,scale:[1,1,1],rotation_deg:[0,0,0]}});
      const sample = y => {
        gl.readPixels(size/2,y,1,1,gl.RGBA,gl.UNSIGNED_BYTE,pixel);
        return pixel[0];
      };
      try {
        await Promise.all(world.update(camera)); world.update(camera);
        const mesh = world.groups.get(world.grid.key(position)).children[0];
        const frustum = new THREE.Frustum().setFromProjectionMatrix(new THREE.Matrix4().multiplyMatrices(camera.projectionMatrix,camera.matrixWorldInverse));
        renderer.render(scene,camera);
        const shaded = sample(size/2), clear = sample(4);
        mesh.castShadow = false; renderer.render(scene,camera);
        return {shaded, lit:sample(size/2), clear, litClear:sample(4), inView:frustum.intersectsObject(mesh), error:gl.getError()};
      } finally {
        sun.shadow.dispose(); ground.geometry.dispose(); ground.material.dispose();
        renderer.dispose(); renderer.forceContextLoss();
      }
    });
    assert.ok(!streamedShadow.inView && streamedShadow.lit > streamedShadow.shaded + 30
      && streamedShadow.clear === streamedShadow.litClear && streamedShadow.error === 0, JSON.stringify(streamedShadow));
    console.log('PASS streaming shadows: an offscreen chunk casts onto the visible ground');

    const impostors = await page.evaluate(async () => {
      const THREE = await import('three');
      const { StreamingWorld } = await import('./world.js');
      const { worldConfig } = await import('./streaming.js');
      const renderer = new THREE.WebGLRenderer({canvas:document.createElement('canvas')});
      renderer.setSize(96, 96, false);
      const gl = renderer.getContext(), pixel = new Uint8Array(4);
      const config = worldConfig([{tags:['player'],props:{ai_world:{impostor_views:4,lod_distances:[2,4]}}}]);
      const scene = new THREE.Scene(), model = new THREE.Group();
      const color = [0xff0000,0x00ff00,0xffffff,0xffffff,0x0000ff,0xffff00];
      const source = new THREE.Mesh(new THREE.BoxGeometry(2, 2, 2),color.map(value=>new THREE.MeshBasicMaterial({color:value})));
      source.position.x = 2;
      model.add(source);
      const world = new StreamingWorld(scene,renderer,config,{load:async()=>({scene:model})});
      const camera = new THREE.PerspectiveCamera(30,1,0.1,40);
      const samples = [];
      try {
        for (const [index, batched] of [true,false].entries()) {
          const transform = {position:[0,0,0],scale:[1.3,1,1],rotation_deg:[0,90,0]};
          const entity = {id:index+1,kind:'gltf',path:'fixture',transform};
          const object = batched ? null : new THREE.Group();
          if (object) { object.rotation.y = Math.PI / 2; object.scale.fromArray(transform.scale); object.add(model.clone()); }
          world.add(entity,object);
          const center = new THREE.Vector3(0,0,-2.6);
          let instance, before;
          for (const offset of [[1,0,0],[0,0,-1],[-1,0,0],[0,0,1]]) {
            camera.position.fromArray(offset).multiplyScalar(10).add(center);
            camera.lookAt(center); camera.updateMatrixWorld();
            await Promise.all(world.update(camera));
            await world.entities.get(entity.id).impostorTask;
            world.update(camera);
            if (batched && !instance) {
              instance = scene.getObjectByProperty('isInstancedMesh',true);
              before = instance.instanceMatrix.version;
            }
            renderer.render(scene,camera);
            gl.readPixels(48,48,1,1,gl.RGBA,gl.UNSIGNED_BYTE,pixel);
            samples.push(pixel.slice(0,3).reduce((mask,channel,i)=>mask | (Number(channel>128)<<i),0));
          }
          if (batched && before !== instance.instanceMatrix.version) throw new Error('Camera motion updated the instance buffer');
          world.reset();
        }
        return {samples,error:gl.getError()};
      } finally { renderer.dispose(); renderer.forceContextLoss(); }
    });
    assert.deepEqual(impostors,{samples:[4,1,3,2,4,1,3,2],error:0});
    console.log('PASS impostors: four camera angles use the correct baked faces, centered on the rotated model without instance buffer uploads');

    const finishes = await page.evaluate(async () => {
      const THREE = await import('three');
      const { primitiveMaterial } = await import('./materials.js');
      const scene = new THREE.Scene(), camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 10);
      camera.position.z = 3;
      const key = new THREE.DirectionalLight(0xffffff, 4);
      key.position.set(2, 1, 4);
      const geometry = new THREE.SphereGeometry(0.8, 64, 32), mesh = new THREE.Mesh(geometry);
      mesh.material.dispose();
      scene.add(mesh, key, new THREE.HemisphereLight(0xffffff, 0x555555, 0.2));
      const renderer = new THREE.WebGLRenderer({canvas: document.createElement('canvas')});
      const gl = renderer.getContext(), size = 128;
      renderer.setSize(size, size, false);
      renderer.toneMapping = THREE.ACESFilmicToneMapping;
      const sample = settings => {
        const native = {color:[0.29, 0.20, 0.17], props:{ai_material:settings}};
        mesh.material = primitiveMaterial(native);
        const pixels = new Uint8Array(size * size * 4);
        try {
          renderer.render(scene, camera);
          gl.readPixels(0, 0, size, size, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
          return pixels;
        } finally { mesh.material.dispose(); }
      };
      const changed = (base, finish) => {
        const a = sample(base), b = sample({...base, ...finish});
        let count = 0;
        for (let i = 0; i < a.length; i += 4) {
          if (Math.abs(a[i]-b[i]) + Math.abs(a[i+1]-b[i+1]) + Math.abs(a[i+2]-b[i+2]) > 20) count++;
        }
        return count;
      };
      try {
        const brushed = changed({metalness:1, roughness:0.32}, {anisotropy:0.8, anisotropy_rotation_deg:45});
        const cloth = changed({metalness:0, roughness:0.8}, {sheen:1, sheen_color:'#ff3322', sheen_roughness:0.7});
        const coating = changed({metalness:0.5, roughness:0.28}, {iridescence:1, iridescence_ior:1.5, iridescence_thickness_nm:420});
        return {brushed, cloth, coating, glError: gl.getError()};
      } finally { geometry.dispose(); renderer.dispose(); renderer.forceContextLoss(); }
    });
    assert.ok(finishes.brushed > 80 && finishes.cloth > 80 && finishes.coating > 80 && finishes.glError === 0, JSON.stringify(finishes));
    console.log('PASS PBR finishes: visible brushed anisotropy, fabric sheen, and thin-film iridescence');

    const glass = await page.evaluate(async () => {
      const THREE = await import('three');
      const {HDRPipeline} = await import('./hdr.js');
      const {primitiveMaterial} = await import('./materials.js');
      const renderer = new THREE.WebGLRenderer({canvas:document.createElement('canvas')});
      renderer.toneMapping = THREE.ACESFilmicToneMapping;
      const scene = new THREE.Scene(), gl = renderer.getContext();
      const camera = new THREE.PerspectiveCamera(42, 1, 0.1, 40);
      camera.position.z = 3;
      const panelGeometry = new THREE.PlaneGeometry(2, 4);
      const panelMaterials = [new THREE.MeshBasicMaterial({color:0xffffff}),
        new THREE.MeshBasicMaterial({color:0x83f8ff})];
      for (const [i, material] of panelMaterials.entries()) {
        const panel = new THREE.Mesh(panelGeometry, material);
        panel.position.set((i ? 1 : -1) + 0.5, 0, -1);
        scene.add(panel);
      }
      const geometry = new THREE.SphereGeometry(0.85, 40, 24), sphere = new THREE.Mesh(geometry);
      sphere.material.dispose();
      scene.add(sphere);
      const pipeline = new HDRPipeline(renderer, {bloomStrength:0});
      const sample = (tier, settings) => {
        sphere.material = primitiveMaterial({color:[1,1,1], props:{ai_material:{
          roughness:0.03, metalness:0, ...settings,
        }}});
        try {
          pipeline.resize(tier, 96, 96, 1);
          if (Boolean(pipeline.sceneTarget) !== (tier === 2)) throw new Error('Expected HDR on Ultra and direct rendering on Low');
          pipeline.render(scene, camera);
          const pixels = new Uint8Array(96 * 96 * 4);
          gl.readPixels(0, 0, 96, 96, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
          return pixels;
        } finally { sphere.material.dispose(); }
      };
      const left = image => Array.from(image.slice((48 * 96 + 35) * 4, (48 * 96 + 35) * 4 + 3));
      try {
        const opaque = left(sample(2, {}));
        const flat = sample(2, {transmission:1, thickness:0});
        const thick = {transmission:1, thickness:1};
        const refracted = sample(2, thick);
        const absorb = {...thick, attenuation_color:'#80ff80', attenuation_distance:0.3};
        const tinted = left(sample(2, absorb)), directTint = left(sample(0, absorb));
        let bent = 0;
        for (let i = 0; i < flat.length; i += 4) if (Math.abs(flat[i] - refracted[i]) > 20) bent++;
        return {opaque, clear:left(refracted), tinted, directTint, bent, error:gl.getError()};
      } finally {
        pipeline.release(); geometry.dispose(); panelGeometry.dispose();
        panelMaterials.forEach(material => material.dispose());
        renderer.dispose(); renderer.forceContextLoss();
      }
    });
    assert.ok(glass.clear[1] > glass.opaque[1] + 100 && glass.bent > 60, JSON.stringify(glass));
    assert.ok(glass.tinted[1] > glass.tinted[0] + 30 && glass.directTint[1] > glass.directTint[0] + 30 && glass.error === 0, JSON.stringify(glass));
    console.log('PASS primitive glass: background refraction and absorption on HDR and direct rendering');

    const terrain = await page.evaluate(async () => {
      const THREE = await import('three');
      const {terrainDetailTexture} = await import('./terrain.js');
      const {terrainSurface} = await import('./surfaces.js');
      const scene = new THREE.Scene(), camera = new THREE.OrthographicCamera(-1.6,1.6,1.6,-1.6,0.1,200);
      camera.up.set(0,0,-1);
      const sun = new THREE.DirectionalLight(0xffffff,3);
      sun.position.set(4,1.5,-2);
      scene.add(sun);
      const detail = terrainDetailTexture(THREE);
      const base = new THREE.MeshStandardMaterial({roughness:0.96});
      const relief = base.clone();
      terrainSurface(THREE,base);
      terrainSurface(THREE,relief,0.6,detail,1);
      const wide = new THREE.PlaneGeometry(4,4).rotateX(-Math.PI/2), half = new THREE.PlaneGeometry(2,4).rotateX(-Math.PI/2);
      const full = new THREE.Mesh(wide,relief), left = new THREE.Mesh(half,relief), right = new THREE.Mesh(half,relief);
      left.position.x = -1; right.position.x = 1;
      scene.add(full,left,right);
      const size = 128, renderer = new THREE.WebGLRenderer({canvas:document.createElement('canvas')});
      renderer.domElement.dataset.quality = '2';
      renderer.setSize(size,size,false);
      const gl = renderer.getContext();
      const sample = (height, material, tiled = false) => {
        camera.position.y = height; camera.lookAt(0,0,0);
        full.material = material; full.visible = !tiled; left.visible = right.visible = tiled;
        renderer.render(scene,camera);
        const pixels = new Uint8Array(size*size*4);
        gl.readPixels(0,0,size,size,gl.RGBA,gl.UNSIGNED_BYTE,pixels);
        return pixels;
      };
      const changed = (a,b) => {
        let count = 0;
        for (let i = 0; i < a.length; i += 4) if (Math.abs(a[i]-b[i]) > 6) count++;
        return count;
      };
      try {
        const near = sample(3,relief);
        return {detail:changed(near,sample(3,base)), seam:changed(near,sample(3,relief,true)),
          far:changed(sample(120,base),sample(120,relief)), error:gl.getError()};
      } finally {
        base.dispose(); relief.dispose(); detail.dispose(); half.dispose(); wide.dispose();
        renderer.dispose(); renderer.forceContextLoss();
      }
    });
    assert.ok(terrain.detail > 300 && terrain.seam < 10 && terrain.far === 0 && terrain.error === 0, JSON.stringify(terrain));
    console.log('PASS terrain micro-relief: near-field shading, seamless tiles and distance fade');

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
        const ext = gl.getExtension('EXT_texture_filter_anisotropic');
        return {
          pixels: Array.from(ctx.getImageData(0, 0, copy.width, copy.height).data),
          error: gl.getError(), title: document.getElementById('title').textContent,
          maximumAnisotropy: ext ? gl.getParameter(ext.MAX_TEXTURE_MAX_ANISOTROPY_EXT) : 1,
          sampledAnisotropy: window.textureAnisotropy,
          cameraDistance: Math.hypot(...window.__engineStats.cameraPosition.map((v, i) => v - window.__engineStats.playerPosition[i])),
        };
      });
    };
    const materialImages = new Map();
    const collectMaterial = response => {
      if (response.url().startsWith(`${studio}/assets/material/`)) materialImages.set(response.url(), response.status());
    };
    page.on('response', collectMaterial);
    const [loaded, floor, reflected] = await Promise.all([
      page.waitForResponse(map), page.waitForResponse(response => response.url().startsWith(studio) && response.url().endsWith('.glb')), render(),
    ]);
    page.off('response', collectMaterial);
    await page.screenshot({ path: `${shots}/material-studio-contact.png` });
    assert.equal(loaded.status(), 200, 'the exported panorama is loaded from its bundle');
    assert.equal(floor.status(), 200, 'the textured floor loads from a bundled GLB');
    assert.equal(reflected.title, 'Materiallabor');
    assert.equal(reflected.error, 0);
    assert.deepEqual([...materialImages.values()], [200, 200, 200], 'studio loads the three bundled wall images');
    if (reflected.maximumAnisotropy > 1) {
      assert.ok(reflected.sampledAnisotropy.includes(Math.min(8, reflected.maximumAnisotropy)), 'the Ultra studio applies anisotropy to its WebGL sampler');
      console.log('PASS GLB textures: bundled textured floor applies the supported Ultra anisotropy');
    } else {
      assert.deepEqual(reflected.sampledAnisotropy, [], 'unsupported GPUs retain ordinary sampling');
      console.log('PASS GLB textures: bundled textured floor renders without anisotropy support');
    }
    const before = await page.evaluate(() => window.__engineStats.playerPosition);
    await page.keyboard.down('KeyD');
    await page.evaluate(() => advanceFrames(20));
    await page.keyboard.up('KeyD');
    const after = await page.evaluate(() => window.__engineStats.playerPosition);
    assert.ok(after?.[0] > before?.[0] + 0.25, await page.locator('#modal-text').textContent());
    await page.screenshot({ path: `${shots}/material-studio.png` });

    const background = `${studio}/scene.json`;
    const patchScene = callback => async route => {
      const response = await route.fetch();
      const scene = await response.json();
      callback(scene);
      await route.fulfill({ response, json: scene });
    };
    const noMaterialMaps = patchScene(scene => {
      delete scene.entities.find(entity => entity.name === 'rear-wall').props.ai_material;
    });
    await page.route(background, noMaterialMaps);
    const flatWall = await render();
    await page.unroute(background, noMaterialMaps);
    assert.equal(flatWall.error, 0);
    const materialDelta = reflected.pixels.reduce((sum, value, i) => sum + Math.abs(value - flatWall.pixels[i]), 0);
    assert.ok(materialDelta > 1000, `bundled primitive maps change the image: ${materialDelta}`);
    console.log('PASS primitive maps: bundled sRGB, normal and ORM images shade the tiled studio wall');
    const noContact = patchScene(scene => {
      scene.entities.find(entity => entity.tags.includes('player')).props.ai_graphics.contact_occlusion = 0;
    });
    await page.route(background, noContact);
    const open = await render();
    await page.screenshot({ path: `${shots}/material-studio-no-contact.png` });
    await page.unroute(background, noContact);
    assert.equal(open.error, 0);
    assert.notDeepEqual(open.pixels, reflected.pixels, 'the exported studio enables contact occlusion');
    const agxToneMapping = patchScene(scene => {
      scene.entities.find(entity => entity.tags.includes('player')).props.ai_graphics.tone_mapping = 'agx';
    });
    await page.route(background, agxToneMapping);
    const agxStudio = await render();
    await page.screenshot({ path: `${shots}/material-studio-agx.png` });
    await page.unroute(background, agxToneMapping);
    assert.equal(agxStudio.error, 0);
    assert.notDeepEqual(agxStudio.pixels, reflected.pixels, 'the exported ai_graphics setting selects AgX');
    console.log('PASS filmic output: authored AgX on WebGL, direct/HDR parity and bloom-free Ultra compositing');
    const without = type => patchScene(scene => {
      for (const entity of scene.entities) {
        const light = entity.props?.ai_light;
        if (light && (light.type ?? 'point') === type) delete entity.props.ai_light;
      }
    });
    const noPoints = without('point'), noSpots = without('spot');
    await page.route(background, noPoints);
    const noPointLighting = await render();
    await page.unroute(background, noPoints);
    await page.route(background, noSpots);
    const noSpotLighting = await render();
    await page.unroute(background, noSpots);
    const streamed = async lit => {
      const settings = patchScene(scene => {
        const player = scene.entities.find(entity => entity.tags.includes('player'));
        player.props.ai_world = {};
        player.props.ai_graphics.fog_density = 0;
        for (const entity of scene.entities) {
          if (!lit || entity.props?.ai_light?.type === 'spot') delete entity.props?.ai_light;
          else if (entity.props?.ai_light) entity.props.ai_light.range = 6;
        }
      });
      await page.route(background, settings);
      try { return await render(); }
      finally { await page.unroute(background, settings); }
    };
    const streamedLit = await streamed(true), streamedUnlit = await streamed(false);
    const showMap = patchScene(scene => {
      scene.entities.find(entity => entity.tags.includes('player')).props.ai_graphics.environment_background = true;
    });
    await page.route(background, showMap);
    const panorama = await render();
    await page.unroute(background, showMap);
    const showMapWithClouds = patchScene(scene => {
      const graphics = scene.entities.find(entity => entity.tags.includes('player')).props.ai_graphics;
      graphics.environment_background = true;
      graphics.sky_clouds = true;
    });
    await page.route(background, showMapWithClouds);
    const panoramaWithClouds = await render();
    await page.unroute(background, showMapWithClouds);
    assert.deepEqual(panoramaWithClouds.pixels, panorama.pixels, 'visible authored HDRI takes precedence over procedural clouds');
    const rotateMap = patchScene(scene => {
      const graphics = scene.entities.find(entity => entity.tags.includes('player')).props.ai_graphics;
      graphics.environment_rotation_deg = 90;
    });
    await page.route(background, rotateMap);
    const rotated = await render();
    await page.unroute(background, rotateMap);
    const skyRotation = await page.evaluate(async url => {
      const THREE = await import('three');
      const {loadEnvironment} = await import('./sky.js');
      const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera(45, 1, 0.1, 50);
      const renderer = new THREE.WebGLRenderer({canvas: document.createElement('canvas')});
      const gl = renderer.getContext(), pixel = new Uint8Array(4);
      renderer.setSize(32, 32, false);
      const maps = await Promise.all([loadEnvironment(url), loadEnvironment(url, 90)]);
      const red = (texture, azimuth) => {
        scene.background = texture;
        // Aim at the warm panel above the horizon rather than the studio camera's uniform ground.
        camera.lookAt(Math.cos(azimuth), Math.tan(0.46), Math.sin(azimuth));
        renderer.render(scene, camera);
        gl.readPixels(16, 16, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel);
        return pixel[0];
      };
      try {
        const before = -1.6, after = before + Math.PI / 2;
        return {before: maps.map(texture => red(texture, before)), after: maps.map(texture => red(texture, after)), error: gl.getError()};
      } finally { maps.forEach(texture => texture.dispose()); renderer.dispose(); renderer.forceContextLoss(); }
    }, map);
    await page.route(map, route => route.abort());
    const fallback = await render();
    assert.equal(panorama.error, 0);
    assert.equal(fallback.error, 0, 'a missing panorama falls back without WebGL errors');
    const delta = (a, b) => a.reduce((sum, color, i) => sum + Math.abs(color - b[i]), 0);
    const top = 40 * 2 * 4;
    const sky = ({pixels}) => [0, 40].flatMap(row => [
      ...pixels.slice(row * 4, (row + 8) * 4), ...pixels.slice((row + 32) * 4, (row + 40) * 4),
    ]);
    assert.equal(noPointLighting.error, 0);
    assert.equal(noSpotLighting.error, 0);
    assert.deepEqual(sky(reflected), sky(noPointLighting), 'point lights leave the procedural sky unchanged');
    assert.ok(delta(reflected.pixels.slice(top), noPointLighting.pixels.slice(top)) > 800, 'point lights illuminate nearby surfaces in the exported studio');
    assert.deepEqual(sky(reflected), sky(noSpotLighting), 'spotlights leave the procedural sky unchanged');
    assert.ok(delta(reflected.pixels.slice(top), noSpotLighting.pixels.slice(top)) > 300, 'the authored spotlight illuminates the exported studio');
    assert.ok(streamedLit.cameraDistance > 46, 'the streaming camera is outside the lights’ selection margin');
    assert.equal(streamedLit.error, 0);
    assert.equal(streamedUnlit.error, 0);
    assert.ok(delta(streamedLit.pixels.slice(top), streamedUnlit.pixels.slice(top)) > 100, 'short-range lights illuminate the streamed play area');
    assert.ok(delta(sky(panorama), sky(reflected)) > 300, 'the panorama can be visible behind the scene');
    assert.equal(rotated.error, 0);
    assert.deepEqual(sky(rotated), sky(reflected), 'rotation leaves the procedural sky alone');
    assert.ok(delta(rotated.pixels.slice(top), reflected.pixels.slice(top)) > 300, 'rotation changes the reflected studio lighting');
    assert.ok(skyRotation.before[0] > skyRotation.before[1] + 50 && skyRotation.after[1] > skyRotation.after[0] + 50 && skyRotation.error === 0, JSON.stringify(skyRotation));
    assert.deepEqual(sky(reflected), sky(fallback), 'reflection-only HDRI preserves the procedural background');
    assert.ok(delta(reflected.pixels.slice(top), fallback.pixels.slice(top)) > 800, 'the studio map changes the lighting, not just the background');
    console.log('PASS HDRI: offline panorama, rotation, reflection-only mode, visible background, movement and missing-map fallback');
    console.log('PASS point lights: exported fixtures light nearby surfaces without altering the sky');
    const atmosphere = await page.evaluate(async () => {
      const THREE = await import('three');
      const {applySky} = await import('./sky.js');
      const {graphicsConfig} = await import('./graphics-settings.js');
      const renderer = new THREE.WebGLRenderer({canvas: document.createElement('canvas')});
      renderer.setSize(64, 64, false);
      const gl = renderer.getContext(), pixel = new Uint8Array(4);
      const camera = new THREE.PerspectiveCamera(40, 1, 0.1, 20);
      const direction = new THREE.Vector3(1, 0.125, 0.15).normalize();
      const mirror = new THREE.Mesh(new THREE.PlaneGeometry(3, 3),
        new THREE.MeshStandardMaterial({color:0xffffff, roughness:0.12, metalness:1}));
      mirror.position.copy(direction).multiplyScalar(-3);
      mirror.lookAt(camera.position);
      const sample = enabled => {
        const scene = new THREE.Scene();
        const config = graphicsConfig({
          sky:'#303030', horizon:'#303030', ground:'#303030', sun_color:'#ffffff',
          sun_direction:[1, 0.11, 0], environment_intensity:1, sky_atmosphere:enabled,
        }, 'ultra');
        applySky(renderer, scene, config, new THREE.Vector3(...config.sunDirection));
        const read = view => {
          camera.lookAt(view);
          renderer.render(scene, camera);
          gl.readPixels(32, 32, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel);
          return [...pixel.slice(0, 3)];
        };
        try {
          const sky = read(direction);
          const opposite = read(mirror.position);
          scene.add(mirror);
          return {sky, opposite, reflected:read(mirror.position)};
        } finally { scene.background.dispose(); scene.environment.dispose(); }
      };
      try { return {clear:sample(false), hazy:sample({haze:0.7}), error:gl.getError()}; }
      finally { mirror.geometry.dispose(); mirror.material.dispose(); renderer.dispose(); renderer.forceContextLoss(); }
    });
    const {clear, hazy, error} = atmosphere;
    assert.ok(error === 0 && hazy.sky[0] > clear.sky[0] + 25
      && hazy.sky[0] > hazy.sky[2] + 15
      && hazy.reflected[0] > clear.reflected[0] + 20, JSON.stringify(atmosphere));
    assert.deepEqual(hazy.opposite, clear.opposite, 'a ray below the horizon keeps its authored color');
    console.log('PASS atmosphere: warm low-sun haze reaches the procedural sky and its PBR reflections');
  } finally {
    await page.close();
  }
}
