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

    const contact = await page.evaluate(async () => {
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
      const renderer = new THREE.WebGLRenderer({canvas: document.createElement('canvas')});
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
    });
    assert.ok(contact.darkened > 50 && contact.flat > 5000 && contact.sky > 500, JSON.stringify(contact));
    assert.deepEqual([contact.half, contact.draws, contact.released, contact.fallback, contact.glError], [97, 4, true, true, 0]);
    console.log('PASS contact occlusion: local grounding, unchanged open surfaces and sky, and GPU/tier fallback');

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
    const [loaded, floor, reflected] = await Promise.all([
      page.waitForResponse(map), page.waitForResponse(response => response.url().startsWith(studio) && response.url().endsWith('.glb')), render(),
    ]);
    await page.screenshot({ path: `${shots}/material-studio-contact.png` });
    assert.equal(loaded.status(), 200, 'the exported panorama is loaded from its bundle');
    assert.equal(floor.status(), 200, 'the textured floor loads from a bundled GLB');
    assert.equal(reflected.title, 'Materiallabor');
    assert.equal(reflected.error, 0);
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
  } finally {
    await page.close();
  }
}
