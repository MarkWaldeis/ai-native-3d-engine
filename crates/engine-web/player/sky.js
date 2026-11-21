import * as THREE from 'three';

function equirectangularTexture(data, width, height, flipY = false) {
  const texture = new THREE.DataTexture(data, width, height, THREE.RGBAFormat, THREE.HalfFloatType);
  texture.colorSpace = THREE.LinearSRGBColorSpace;
  texture.mapping = THREE.EquirectangularReflectionMapping;
  texture.minFilter = texture.magFilter = THREE.LinearFilter;
  texture.wrapS = THREE.RepeatWrapping;
  texture.flipY = flipY;
  texture.needsUpdate = true;
  return texture;
}

function hash(x, z, seed) {
  let h = Math.imul(x, 0x9e3779b1) ^ Math.imul(z, 0x85ebca6b) ^ Math.imul(seed, 0xc2b2ae35);
  h = Math.imul(h ^ (h >>> 16), 0x7feb352d);
  h = Math.imul(h ^ (h >>> 15), 0x846ca68b);
  return ((h ^ (h >>> 16)) >>> 0) / 4294967295;
}

function noise(x, z, seed) {
  const ix = Math.floor(x), iz = Math.floor(z);
  const dx = x - ix, dz = z - iz;
  const u = dx * dx * (3 - 2 * dx), v = dz * dz * (3 - 2 * dz);
  const mix = THREE.MathUtils.lerp;
  return mix(mix(hash(ix, iz, seed), hash(ix + 1, iz, seed), u),
    mix(hash(ix, iz + 1, seed), hash(ix + 1, iz + 1, seed), u), v);
}

function cloudDensity(x, z, seed) {
  return noise(x, z, seed) * 0.60 + noise(x * 2.05, z * 2.05, seed + 1) * 0.24
    + noise(x * 4.27, z * 4.27, seed + 2) * 0.11 + noise(x * 8.6, z * 8.6, seed + 3) * 0.05;
}

export function skyTexture(config, sunDirection) {
  const width = 512, height = 256;
  const pixels = new Uint16Array(width * height * 4);
  const sky = new THREE.Color(config.sky), ground = new THREE.Color(config.ground);
  const horizon = new THREE.Color(config.horizon), sun = new THREE.Color(config.sunColor);
  const color = new THREE.Color();
  const clouds = config.skyClouds, cloudy = clouds?.coverage > 0 && clouds?.opacity > 0;
  const threshold = cloudy ? THREE.MathUtils.lerp(0.83, 0.28, clouds.coverage) : 1;
  const cloudLight = 0.42 + Math.max(0, sunDirection.y) * Math.min(1.5, config.sunIntensity / 3);
  const cloudTint = new THREE.Color().setRGB(
    0.78 + 0.22 * sun.r, 0.78 + 0.22 * sun.g, 0.78 + 0.22 * sun.b).multiplyScalar(cloudLight);
  const cloudColor = new THREE.Color();
  const half = THREE.DataUtils.toHalfFloat;
  const longitude = Array.from({length:width}, (_, x) => {
    const angle = ((x + 0.5) / width - 0.5) * 2 * Math.PI;
    return [Math.cos(angle), Math.sin(angle)];
  });
  for (let y = 0; y < height; y++) {
    const latitude = ((y + 0.5) / height - 0.5) * Math.PI;
    const elevation = Math.sin(latitude), horizontal = Math.cos(latitude);
    const cloudFade = cloudy ? THREE.MathUtils.smoothstep(elevation, 0.05, 0.25) : 0;
    const cloudDistance = cloudFade ? 3.5 * horizontal * clouds.scale / elevation : 0;
    for (let x = 0; x < width; x++) {
      const [cos, sin] = longitude[x];
      const alignment = horizontal * (cos * sunDirection.x + sin * sunDirection.z) + elevation * sunDirection.y;
      color.copy(horizon).lerp(elevation < 0 ? ground : sky, Math.pow(Math.abs(elevation), 0.45));
      if (elevation > 0) {
        let opacity = 0;
        if (cloudFade) {
          const px = 13.7 + cloudDistance * cos, pz = 8.1 + cloudDistance * sin;
          const density = cloudDensity(px, pz, clouds.seed);
          opacity = THREE.MathUtils.smoothstep(density, threshold - 0.04, threshold + 0.11)
            * clouds.opacity * cloudFade;
          if (opacity > 0) {
            const ahead = cloudDensity(px + 0.23 * sunDirection.x, pz + 0.23 * sunDirection.z, clouds.seed);
            const litEdge = THREE.MathUtils.clamp((density - ahead) * 3 + 0.5, 0, 1);
            cloudColor.copy(cloudTint).multiplyScalar(0.65 + 0.55 * litEdge);
            cloudColor.lerp(horizon, 0.5 * (1 - cloudFade));
            color.lerp(cloudColor, opacity);
          }
        }
        const glow = Math.max(0, alignment) ** 28 * 0.23;
        const disk = THREE.MathUtils.smoothstep(alignment, 0.9995, 0.99982) * 3;
        const sunlight = (glow + disk) * config.sunIntensity / 3 * (1 - opacity);
        color.r += sun.r * sunlight; color.g += sun.g * sunlight; color.b += sun.b * sunlight;
      }
      const i = (y * width + x) * 4;
      pixels[i] = half(color.r); pixels[i + 1] = half(color.g); pixels[i + 2] = half(color.b); pixels[i + 3] = half(1);
    }
  }
  return equirectangularTexture(pixels, width, height);
}

export function environmentLighting(texture, intensity, maxWidth = 1024) {
  const { data, width, height } = texture.image;
  // Keep PMREM's two floating-point convolution targets bounded even for a 4K sky.
  const w = Math.min(maxWidth, width), h = Math.round(w * height / width);
  if (w === width && intensity === 1) return texture;
  const pixels = new Uint16Array(w * h * 4);
  const { toHalfFloat, fromHalfFloat } = THREE.DataUtils, opaque = toHalfFloat(1);
  for (let y = 0; y < h; y++) {
    const y0 = Math.floor(y * height / h), y1 = Math.floor((y + 1) * height / h);
    for (let x = 0; x < w; x++) {
      const x0 = Math.floor(x * width / w), x1 = Math.floor((x + 1) * width / w);
      let r = 0, g = 0, b = 0;
      for (let sy = y0; sy < y1; sy++) for (let sx = x0; sx < x1; sx++) {
        const i = (sy * width + sx) * 4;
        r += fromHalfFloat(data[i]); g += fromHalfFloat(data[i + 1]); b += fromHalfFloat(data[i + 2]);
      }
      const scale = intensity / ((y1 - y0) * (x1 - x0)), i = (y * w + x) * 4;
      pixels[i] = toHalfFloat(Math.min(65504, r * scale));
      pixels[i + 1] = toHalfFloat(Math.min(65504, g * scale));
      pixels[i + 2] = toHalfFloat(Math.min(65504, b * scale));
      pixels[i + 3] = opaque;
    }
  }
  return equirectangularTexture(pixels, w, h, texture.flipY);
}

export function rotatePanorama(data, width, degrees) {
  const pixels = Math.round(width * (degrees % 360) / 360);
  const shift = ((pixels % width + width) % width) * 4;
  if (!shift) return;
  const stride = width * 4, tail = new Uint16Array(shift);
  for (let start = 0; start < data.length; start += stride) {
    const end = start + stride, wrap = end - shift;
    tail.set(data.subarray(wrap, end));
    data.copyWithin(start + shift, start, wrap);
    data.set(tail, start);
  }
}

export async function loadEnvironment(url, rotation = 0) {
  const [{ RGBELoader }, response] = await Promise.all([
    import('./vendor/addons/loaders/RGBELoader.js'), fetch(url, { mode: 'same-origin' }),
  ]);
  if (!response.ok) throw new Error(`HDR map: HTTP ${response.status}`);
  const { data, width, height } = new RGBELoader().parse(await response.arrayBuffer());
  rotatePanorama(data, width, rotation);
  return equirectangularTexture(data, width, height, true);
}

export function applySky(renderer, scene, config, sunDirection, environment) {
  let background = environment && config.environmentBackground ? environment : skyTexture(config, sunDirection);
  if (background.image.width > renderer.capabilities.maxTextureSize) {
    background = environmentLighting(background, 1, renderer.capabilities.maxTextureSize);
  }
  scene.background?.dispose();
  scene.background = background;
  const source = environment ?? background;
  if (!config.environmentIntensity) {
    if (source !== background) source.dispose();
    return;
  }
  const lighting = environmentLighting(source, config.environmentIntensity);
  const generator = new THREE.PMREMGenerator(renderer);
  try {
    scene.environment = generator.fromEquirectangular(lighting).texture;
  } finally {
    if (lighting !== background) lighting.dispose();
    if (source !== lighting && source !== background) source.dispose();
    generator.dispose();
  }
}
