import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { registerHooks } from 'node:module';
import test from 'node:test';

registerHooks({ resolve(specifier, context, nextResolve) {
  return nextResolve(specifier === 'three' ? new URL('../crates/engine-web/player/vendor/three.module.js', import.meta.url).href : specifier, context);
} });
const THREE = await import('three');
const { graphicsConfig, hdrSettings } = await import('../crates/engine-web/player/graphics-settings.js');
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
