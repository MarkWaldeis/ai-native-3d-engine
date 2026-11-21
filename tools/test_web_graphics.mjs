import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { registerHooks } from 'node:module';
import test from 'node:test';

registerHooks({ resolve(specifier, context, nextResolve) {
  return nextResolve(specifier === 'three' ? new URL('../crates/engine-web/player/vendor/three.module.js', import.meta.url).href : specifier, context);
} });
const THREE = await import('three');
const { graphicsConfig, hdrSettings, localLightSettings } = await import('../crates/engine-web/player/graphics-settings.js');
const { ShadowGrid } = await import('../crates/engine-web/player/effects.js');
const { LocalLights } = await import('../crates/engine-web/player/local-lights.js');
const { primitiveMaterial, textureAnisotropy, tuneAssetTextures } = await import('../crates/engine-web/player/materials.js');
const { environmentLighting, rotatePanorama, skyTexture } = await import('../crates/engine-web/player/sky.js');
const { RGBELoader } = await import('../crates/engine-web/player/vendor/addons/loaders/RGBELoader.js');
const { batchKey } = await import('../crates/engine-web/player/streaming.js');

test('the first-person renderer links against the shared web asset loader', async () => {
  const { FirstPerson } = await import('../crates/engine-web/player/fps.js');
  assert.equal(typeof FirstPerson, 'function');
});

test('graphics is opt-in, ignores malformed props, and accepts explicit quality overrides', () => {
  for (const props of [null, 'bad JSON', '[]', true]) assert.equal(graphicsConfig(props, 'auto'), null);
  assert.equal(graphicsConfig({}, 'legacy'), null);
  const config = graphicsConfig('{"sun_direction":[0,4,3],"exposure":200,"bloom_strength":0,"sky":"red"}', 'ultra', '#abcdef');
  assert.equal(config.quality, 2);
  assert.equal(config.exposure, 4);
  assert.equal(config.toneMapping, 'aces');
  assert.equal(graphicsConfig({tone_mapping:'unknown'}, 'ultra').toneMapping, 'aces');
  assert.equal(graphicsConfig('{"tone_mapping":"agx"}', 'ultra').toneMapping, 'agx');
  assert.equal(config.bloomStrength, 0);
  assert.equal(config.hdrAntialias, true);
  assert.equal(graphicsConfig({hdr_antialias:false}, 'ultra').hdrAntialias, false);
  assert.equal(config.contactStrength, 0);
  assert.equal(config.contactRadius, 1.2);
  assert.equal(config.shadowExtent, 24);
  assert.equal(graphicsConfig({shadow_extent:1000}, 'ultra').shadowExtent, 160);
  assert.equal(graphicsConfig({shadow_extent:0}, 'ultra').shadowExtent, 8);
  assert.equal(graphicsConfig({shadow_extent:'90'}, 'ultra').shadowExtent, 24);
  assert.equal(graphicsConfig({contact_occlusion:9}, 'ultra').contactStrength, 1);
  assert.equal(graphicsConfig({contact_radius:0}, 'ultra').contactRadius, 0.1);
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

test('HDRI yaw rotates each scanline without crossing latitude or clipping the linear samples', () => {
  assert.equal(graphicsConfig({environment_rotation_deg: -450}, 'ultra').environmentRotation, -90);
  assert.equal(graphicsConfig({environment_rotation_deg: '90'}, 'ultra').environmentRotation, 0);
  const row = (...red) => red.flatMap(r => [r, r + 1, r + 2, 15360]);
  const unrotated = new Uint16Array([...row(1, 2, 3, 31743), ...row(15, 16, 17, 18)]);
  const rotated = unrotated.slice();
  rotatePanorama(rotated, 4, 450);
  assert.deepEqual([...rotated], [...row(31743, 1, 2, 3), ...row(18, 15, 16, 17)]);
  rotatePanorama(rotated, 4, -90);
  assert.deepEqual(rotated, unrotated);
  rotatePanorama(rotated, 4, 360);
  assert.deepEqual(rotated, unrotated);
});

test('point lights resolve flexible props and remain bounded as emitters move or disappear', () => {
  assert.equal(localLightSettings('invalid'), null);
  assert.equal(localLightSettings({intensity:0}), null);
  assert.deepEqual(localLightSettings('{"color":"#ffAA00","intensity":1200,"range":-2,"offset":[1,0,0]}'), {
    type:'point', color:'#ffaa00', intensity:1000, range:0.25, offset:[1,0,0],
  });
  const scene = new THREE.Scene(), pool = new LocalLights(scene);
  const transform = x => ({position:[x,0,0], scale:[2,1,1], rotation_deg:[0,0,90]});
  for (let id = 9; id > 0; id--) {
    pool.register({id, transform:transform(id), props:{ai_light:{offset:[1,0,0], intensity:id}}});
  }
  pool.register({id:10, visible:false, props:{ai_light:{intensity:1}}});
  const eye = new THREE.Vector3();
  pool.update(eye, 2);
  assert.deepEqual(scene.children.map(light => light.intensity), [1,2,3,4,5,6,7,8]);
  assert.ok(scene.children[0].position.distanceTo(new THREE.Vector3(1,2,0)) < 1e-10);
  assert.equal(pool.sources.has(10), false);
  pool.update(eye, 1);
  assert.equal(scene.children.length, 4);
  pool.update(eye, 0);
  const slots = [...scene.children];
  assert.deepEqual(slots.map(light => light.intensity), [1,2]);
  pool.move(1, transform(-1000));
  pool.remove(2);
  pool.update(eye, 0);
  assert.deepEqual(scene.children, slots, 'view changes and removal do not rebuild the shared GPU light slots');
  assert.deepEqual(slots.map(light => light.intensity), [3,4]);
  pool.update(eye.set(100, 0, 0), 0);
  assert.deepEqual(slots.map(light => light.intensity), [0,0]);
  pool.update(eye.set(-1000, 0, 0), 0);
  assert.deepEqual(slots.map(light => light.intensity), [1,0]);
  pool.update(eye.set(Number.MAX_VALUE, 0, 0), 0);
  assert.deepEqual(slots.map(light => light.intensity), [0,0]);
  assert.equal(pool.cells.size, 2);
  pool.remove(1);
  assert.equal(pool.cells.size, 1);
  pool.reset();
  assert.equal(scene.children.length, 0);
});

test('spot lights keep an independent budget, follow local orientation, and remove their targets', () => {
  const config = localLightSettings({type:'spot', outer_angle:200, inner_angle:200, direction:[0,-3,0]});
  assert.deepEqual(config.axis, [0,-1,0]);
  assert.equal(config.angle, 89 * Math.PI / 180);
  assert.ok(config.penumbra > 0 && config.penumbra < 0.01);
  assert.deepEqual(localLightSettings({type:'spot', direction:[0,0,0]}).axis, [0,0,-1]);
  assert.equal(localLightSettings({type:'unknown'}), null);

  const scene = new THREE.Scene(), pool = new LocalLights(scene);
  const transform = (x, yaw) => ({position:[x,1,0], scale:[1,2,3], rotation_deg:[0,yaw,0]});
  for (let id = 7; id >= 2; id--) {
    pool.register({id, transform:transform(2, 90), props:{ai_light:{type:'spot', intensity:id, offset:[0,1,0]}}});
  }
  pool.register({id:1, transform:transform(3, 0), props:{ai_light:{}}});
  const focus = new THREE.Vector3();
  pool.update(focus, 2);
  assert.deepEqual(pool.slots.spot.map(light => light.intensity), [2,3,4,5]);
  assert.equal(pool.slots.point.length, 1);
  const spot = pool.slots.spot[0];
  assert.deepEqual(spot.position.toArray(), [2,3,0]);
  assert.ok(spot.target.position.distanceTo(new THREE.Vector3(1,3,0)) < 1e-10);
  assert.equal(spot.target.parent, scene);
  pool.move(2, transform(2, -90));
  pool.update(focus, 0);
  assert.deepEqual(pool.slots.spot, [spot]);
  assert.ok(spot.target.position.distanceTo(new THREE.Vector3(3,3,0)) < 1e-10);
  assert.equal(scene.children.length, 3);
  pool.register({id:2, transform:transform(2, 0), props:{ai_light:{type:'spot', intensity:0}}});
  pool.update(focus, 0);
  assert.equal(spot.intensity, 3);
  pool.reset();
  assert.equal(scene.children.length, 0);
});

test('spotlight axes follow nonuniform and mirrored entity scales', () => {
  const pool = new LocalLights(new THREE.Scene()), origin = new THREE.Vector3();
  const transform = scale => ({position:[0,0,0], scale, rotation_deg:[0,0,0]});
  pool.register({id:1, transform:transform([-2,1,0.5]), props:{ai_light:{type:'spot', direction:[1,0,-1]}}});
  pool.update(origin, 0);
  const spot = pool.slots.spot[0];
  assert.ok(spot.target.position.distanceTo(new THREE.Vector3(-2,0,-0.5).normalize()) < 1e-10);
  pool.move(1, transform([0,1,0]));
  pool.update(origin, 0);
  assert.ok(spot.target.position.distanceTo(new THREE.Vector3(1,0,-1).normalize()) < 1e-10);
});

test('HDR respects fallback, render-pixel budget, and quality levels', () => {
  for (const [level, supported, strength] of [[0, true, 1], [2, false, 1], [2, true, 0]]) {
    assert.deepEqual(hdrSettings(level, supported, strength, 2560, 1440, 1.5), {
      enabled: false, bloom: false, contact: false, pixelRatio: 1.5, bloomScale: level === 2 ? 0.5 : 0.25,
    });
  }
  const high = hdrSettings(2, true, 0.2, 2560, 1440, 1.5);
  assert.ok(high.enabled && high.pixelRatio ** 2 * 2560 * 1440 <= 2_600_001);
  assert.equal(high.bloomScale, 0.5);
  assert.equal(high.bloom, true);
  assert.deepEqual(hdrSettings(2, true, 0, 500, 500, 1, true), {
    enabled: true, bloom: false, contact: false, pixelRatio: 1, bloomScale: 0.5,
  });
  assert.equal(hdrSettings(1, true, 0, 500, 500, 1, true).enabled, false);
  assert.ok(hdrSettings(2, true, 0, 500, 500, 1, false, true).contact);
  assert.equal(hdrSettings(2, false, 1, 500, 500, 1, true, true).contact, false);
  assert.equal(hdrSettings(1, true, 1, 500, 500, 1, true, true).contact, false);
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

test('optional physical finishes resolve in Three.js and separate streamed material batches', () => {
  const entity = settings => ({kind:'sphere', radius:1, props:{ai_material:settings}});
  const features = {
    anisotropy:0.7, anisotropy_rotation_deg:450, sheen:2, sheen_color:'#FFAA99', sheen_roughness:0.8,
    iridescence:0.6, iridescence_ior:3, iridescence_thickness_nm:40,
  };
  const material = primitiveMaterial(entity(features));
  assert.ok(material.isMeshPhysicalMaterial);
  assert.equal(material.anisotropy, 0.7);
  assert.equal(material.anisotropyRotation, Math.PI / 2);
  assert.equal(material.sheen, 1);
  assert.equal(material.sheenColor.getHexString(), 'ffaa99');
  assert.equal(material.sheenRoughness, 0.8);
  assert.equal(material.iridescence, 0.6);
  assert.equal(material.iridescenceIOR, 2.5);
  assert.deepEqual(material.iridescenceThicknessRange, [100,100]);
  for (const finish of ['anisotropy','sheen','iridescence']) {
    assert.ok(primitiveMaterial(entity({[finish]:0.5})).isMeshPhysicalMaterial);
    assert.notEqual(batchKey(entity(features)), batchKey(entity({...features,[finish]:0})));
  }
  assert.equal(batchKey(entity(features)), batchKey(entity(JSON.stringify({...features,anisotropy_rotation_deg:90}))));
  assert.equal(batchKey(entity({sheen_color:'#ff0000',anisotropy_rotation_deg:20})), batchKey(entity({})));
});

test('GLB texture sampling respects the GPU and existing texture filters', () => {
  assert.deepEqual([0,1,2].map(level => textureAnisotropy(level, 16)), [1,4,8]);
  assert.equal(textureAnisotropy(2, 2), 2);
  assert.equal(textureAnisotropy(2, 0), 1);
  const map = new THREE.Texture();
  const nearest = new THREE.Texture();
  nearest.magFilter = THREE.NearestFilter;
  const noMips = new THREE.Texture();
  noMips.generateMipmaps = false;
  const linear = new THREE.Texture();
  linear.minFilter = THREE.LinearFilter;
  const material = new THREE.MeshStandardMaterial({ map, normalMap: nearest, roughnessMap: noMips, metalnessMap: linear });
  const mesh = new THREE.Mesh(new THREE.BoxGeometry(), material);
  const group = new THREE.Group();
  group.add(mesh, mesh.clone());
  tuneAssetTextures(group, 1);
  assert.equal(map.version, 0, 'legacy and low quality keep the original textures');
  tuneAssetTextures(group, 8);
  assert.equal(map.anisotropy, 8);
  assert.equal(map.version, 1, 'a shared texture is uploaded only once');
  assert.ok(group.children.every(child => child.userData.sharedAsset));
  for (const texture of [nearest, noMips, linear]) assert.equal(texture.version, 0);
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

test('opt-in atmospheric scattering responds to sun elevation, haze and azimuth', () => {
  const morning = [1, 0.11, 0], noon = [1, 1.2, 0];
  const sample = (options, direction) => {
    const config = graphicsConfig({
      sky:'#303030', horizon:'#303030', ground:'#303030', sun_color:'#ffffff',
      sun_direction:direction, sky_atmosphere:options,
    }, 'ultra');
    return skyTexture(config, new THREE.Vector3(...config.sunDirection));
  };
  const color = (texture, azimuth, elevation) => {
    const {data,width,height} = texture.image;
    const x = Math.floor((azimuth / (2 * Math.PI) + 0.5) * width);
    const y = Math.floor((elevation / Math.PI + 0.5) * height);
    return [...data.subarray(4 * (y * width + x), 4 * (y * width + x) + 3)].map(THREE.DataUtils.fromHalfFloat);
  };
  assert.deepEqual(graphicsConfig({sky_atmosphere:{strength:8,haze:-1}}, 'ultra').skyAtmosphere,
    {strength:1,haze:0});
  assert.deepEqual(sample(false, morning).image.data, sample({strength:0}, morning).image.data);
  const sunset = sample({haze:0.7}, morning), dry = sample({haze:0}, morning), day = sample(true, noon);
  const near = color(sunset, 0.12, 0.12), away = color(sunset, 1.9, 0.12);
  assert.ok(near[0] > near[2] + 0.04, 'low sunlight warms the sky');
  assert.ok(near[0] > away[0] + 0.06, 'forward haze follows the sun');
  assert.ok(near[0] > color(dry, 0.12, 0.12)[0] + 0.06, 'more haze increases the halo');
  const zenith = color(day, 1.5, 1.45);
  assert.ok(zenith[2] > zenith[0] + 0.015, 'daytime overhead sky scatters more blue');
  assert.deepEqual(color(sunset, 0.12, -0.12), color(dry, 0.12, -0.12));
  assert.deepEqual(sample(true, [1,-1,0]).image.data, sample(false, [1,-1,0]).image.data);
});
