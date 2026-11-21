import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { registerHooks } from 'node:module';
import test from 'node:test';

registerHooks({ resolve(specifier, context, nextResolve) {
  return nextResolve(specifier === 'three' ? new URL('../crates/engine-web/player/vendor/three.module.js', import.meta.url).href : specifier, context);
} });
const THREE = await import('three');
const { graphicsConfig, hdrSettings, pointLightSettings } = await import('../crates/engine-web/player/graphics-settings.js');
const { ShadowGrid } = await import('../crates/engine-web/player/effects.js');
const { LocalLights } = await import('../crates/engine-web/player/local-lights.js');
const { primitiveMaterial } = await import('../crates/engine-web/player/materials.js');
const { environmentLighting, skyTexture } = await import('../crates/engine-web/player/sky.js');
const { RGBELoader } = await import('../crates/engine-web/player/vendor/addons/loaders/RGBELoader.js');
const { batchKey } = await import('../crates/engine-web/player/streaming.js');

test('graphics is opt-in, ignores malformed props, and accepts explicit quality overrides', () => {
  for (const props of [null, 'bad JSON', '[]', true]) assert.equal(graphicsConfig(props, 'auto'), null);
  assert.equal(graphicsConfig({}, 'legacy'), null);
  const config = graphicsConfig('{"sun_direction":[0,4,3],"exposure":200,"bloom_strength":0,"sky":"red"}', 'ultra', '#abcdef');
  assert.equal(config.quality, 2);
  assert.equal(config.exposure, 4);
  assert.equal(config.bloomStrength, 0);
  assert.equal(config.hdrAntialias, true);
  assert.equal(graphicsConfig({hdr_antialias:false}, 'ultra').hdrAntialias, false);
  assert.equal(config.sky, '#4d88cb');
  assert.equal(config.horizon, '#abcdef');
  assert.deepEqual(config.sunDirection, [0, 0.8, 0.6]);
  assert.equal(graphicsConfig(null, 'low').quality, 0);
  assert.equal(graphicsConfig({}, 'auto').quality, -1);
  assert.equal(graphicsConfig({environment_map:'https://example.com/photo.hdr'}, 'ultra').environmentMap, null);
  assert.equal(graphicsConfig({environment_map:'assets/environment.hdr'}, 'ultra').environmentMap, 'assets/environment.hdr');
  assert.equal(graphicsConfig({environment_background:false}, 'ultra').environmentBackground, false);
});

test('moving the sun camera does not move shadows by fractions of a texel', () => {
  for (const offset of [new THREE.Vector3(-6, 16, 9), new THREE.Vector3(0, 20, 0)]) {
    const light = new THREE.DirectionalLight();
    const shadow = light.shadow, camera = shadow.camera;
    Object.assign(camera, {left:-12, right:12, top:12, bottom:-12});
    camera.updateProjectionMatrix();
    const grid = new ShadowGrid(offset, camera.up);
    for (const resolution of [512, 1024, 2048]) {
      shadow.mapSize.setScalar(resolution);
      const step = 24 / resolution;
      const uv = (x, y) => {
        const origin = new THREE.Vector3().addScaledVector(grid.right, x * step).addScaledVector(grid.up, y * step);
        grid.align(light.target.position.copy(origin), camera, shadow.mapSize);
        light.position.copy(light.target.position).add(offset);
        light.updateMatrixWorld(); light.target.updateMatrixWorld();
        shadow.updateMatrices(light);
        return new THREE.Vector3(1, 1, 1).applyMatrix4(shadow.matrix);
      };
      const origin = uv(0, 0), subpixel = uv(0.4, -0.4), shifted = uv(1.2, -1.2);
      assert.ok(origin.distanceTo(subpixel) < 1e-10);
      assert.ok(Math.abs((origin.x - shifted.x) * resolution - 1) < 1e-6);
      assert.ok(Math.abs((origin.y - shifted.y) * resolution + 1) < 1e-6);
    }
  }
});

test('studio HDRI decodes HDR colors and lighting intensity does not alter the background', async () => {
  const file = await readFile(new URL('../examples/material_studio/studio.hdr', import.meta.url));
  const { data, width, height } = new RGBELoader().parse(file.buffer.slice(file.byteOffset, file.byteOffset + file.byteLength));
  assert.deepEqual([width, height], [256, 128]);
  const texture = new THREE.DataTexture(data, width, height, THREE.RGBAFormat, THREE.HalfFloatType);
  const peak = data.reduce((index, value, i) => value > data[index] ? i : index, 0);
  const { fromHalfFloat } = THREE.DataUtils;
  const bright = fromHalfFloat(data[peak]);
  assert.ok(bright > 10, 'studio softboxes retain unclipped HDR radiance');
  const dimmed = environmentLighting(texture, 0.5);
  assert.ok(Math.abs(fromHalfFloat(dimmed.image.data[peak]) - bright / 2) < 0.01);
  assert.equal(fromHalfFloat(texture.image.data[peak]), bright);
  assert.equal(environmentLighting(texture, 1), texture);
});

test('point lights resolve flexible props and remain bounded as emitters move or disappear', () => {
  assert.equal(pointLightSettings('invalid'), null);
  assert.equal(pointLightSettings({intensity:0}), null);
  assert.deepEqual(pointLightSettings('{"color":"#ffAA00","intensity":1200,"range":-2,"offset":[1,0,0]}'), {
    color:'#ffaa00', intensity:1000, range:0.25, offset:[1,0,0],
  });
  const scene = new THREE.Scene(), pool = new LocalLights(scene), transforms = new Map();
  for (let id = 9; id > 0; id--) {
    pool.register({id, props:{ai_light:{offset:[1,0,0], intensity:id}}});
    transforms.set(id, {transform:{position:[id,0,0], scale:[2,1,1], rotation_deg:[0,0,90]}});
  }
  pool.register({id:10, visible:false, props:{ai_light:{intensity:1}}});
  const eye = new THREE.Vector3();
  pool.update(eye, transforms, 2);
  assert.deepEqual(scene.children.map(light => light.intensity), [1,2,3,4,5,6,7,8]);
  assert.ok(scene.children[0].position.distanceTo(new THREE.Vector3(1,2,0)) < 1e-10);
  assert.equal(pool.sources.has(10), false);
  pool.update(eye, transforms, 1);
  assert.equal(scene.children.length, 4);
  pool.update(eye, transforms, 0);
  const slots = [...scene.children];
  assert.deepEqual(slots.map(light => light.intensity), [1,2]);
  transforms.get(1).transform.position[0] += 1000;
  pool.remove(2);
  pool.update(eye, transforms, 0);
  assert.deepEqual(scene.children, slots, 'view changes and removal do not rebuild the shared GPU light slots');
  assert.deepEqual(slots.map(light => light.intensity), [3,4]);
  pool.update(eye.set(100, 0, 0), transforms, 0);
  assert.deepEqual(slots.map(light => light.intensity), [0,0]);
  pool.reset();
  assert.equal(scene.children.length, 0);
});

test('HDR respects fallback, render-pixel budget, and quality levels', () => {
  for (const [level, supported, strength] of [[0, true, 1], [2, false, 1], [2, true, 0]]) {
    assert.deepEqual(hdrSettings(level, supported, strength, 2560, 1440, 1.5), {
      enabled: false, pixelRatio: 1.5, bloomScale: level === 2 ? 0.5 : 0.25,
    });
  }
  const high = hdrSettings(2, true, 0.2, 2560, 1440, 1.5);
  assert.ok(high.enabled && high.pixelRatio ** 2 * 2560 * 1440 <= 2_600_001);
  assert.equal(high.bloomScale, 0.5);
  assert.equal(hdrSettings(1, true, 1, 500, 500, 1).bloomScale, 0.25);
});

test('material settings control real Three.js materials and normalized instancing keys', () => {
  const entity = { kind: 'box', size: [1, 1, 1], color: [0.2, 0.4, 0.8], props: { ai_material: {
    roughness: -4, metalness: 1, emissive: '#00AAff', emissive_intensity: 6, clearcoat: 0.8,
  } } };
  const material = primitiveMaterial(entity);
  assert.ok(material.isMeshPhysicalMaterial);
  assert.equal(material.roughness, 0);
  assert.equal(material.metalness, 1);
  assert.equal(material.clearcoat, 0.8);
  assert.equal(material.emissive.getHexString(), '00aaff');
  assert.equal(material.emissiveIntensity, 6);
  assert.deepEqual(material.color.toArray(), entity.color);
  const equivalent = { ...entity, props: { ai_material: JSON.stringify({ ...entity.props.ai_material, roughness: 0 }) } };
  assert.equal(batchKey(equivalent), batchKey(entity));
  const legacy = { ...entity, props: {} };
  assert.notEqual(batchKey(legacy), batchKey(entity));
  assert.equal(batchKey(legacy), batchKey({ ...legacy, props: { ai_material: 'invalid' } }));
  assert.equal(primitiveMaterial(legacy).isMeshPhysicalMaterial, undefined);
  for (const props of [entity.props, legacy.props]) assert.equal(batchKey({ kind: 'gltf', path: 'a.glb', props }), batchKey({ kind: 'gltf', path: 'a.glb' }));
});

test('procedural sky stores linear HDR radiance and moves the solar highlight with the authored sun', () => {
  const config = graphicsConfig({ sun_direction: [1, 1, 0] }, 'ultra');
  const texture = skyTexture(config, new THREE.Vector3(...config.sunDirection));
  const { data, width, height } = texture.image;
  const red = (x, y) => THREE.DataUtils.fromHalfFloat(data[(y * width + x) * 4]);
  assert.equal(texture.mapping, THREE.EquirectangularReflectionMapping);
  assert.equal(texture.colorSpace, THREE.LinearSRGBColorSpace);
  assert.ok(red(width / 2, height * 0.75) > 2, 'the +X 45° sun is HDR');
  assert.ok(red(0, height * 0.75) < 1, 'opposite side has no glare');
});
