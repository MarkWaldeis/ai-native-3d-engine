import * as THREE from 'three';

export function skyTexture(config, sunDirection, intensity = 1) {
  const width = 512, height = 256;
  const pixels = new Uint16Array(width * height * 4);
  const sky = new THREE.Color(config.sky), ground = new THREE.Color(config.ground);
  const horizon = new THREE.Color(config.horizon), sun = new THREE.Color(config.sunColor);
  const color = new THREE.Color();
  const half = THREE.DataUtils.toHalfFloat;
  for (let y = 0; y < height; y++) {
    const latitude = ((y + 0.5) / height - 0.5) * Math.PI;
    const elevation = Math.sin(latitude), horizontal = Math.cos(latitude);
    for (let x = 0; x < width; x++) {
      const longitude = ((x + 0.5) / width - 0.5) * 2 * Math.PI;
      const alignment = horizontal * (Math.cos(longitude) * sunDirection.x + Math.sin(longitude) * sunDirection.z) + elevation * sunDirection.y;
      color.copy(horizon).lerp(elevation < 0 ? ground : sky, Math.pow(Math.abs(elevation), 0.45));
      if (elevation > 0) {
        const glow = Math.max(0, alignment) ** 28 * 0.23;
        const disk = THREE.MathUtils.smoothstep(alignment, 0.9995, 0.99982) * 3;
        const sunlight = (glow + disk) * config.sunIntensity / 3;
        color.r += sun.r * sunlight; color.g += sun.g * sunlight; color.b += sun.b * sunlight;
      }
      const i = (y * width + x) * 4;
      pixels[i] = half(color.r * intensity); pixels[i + 1] = half(color.g * intensity); pixels[i + 2] = half(color.b * intensity); pixels[i + 3] = half(1);
    }
  }
  const texture = new THREE.DataTexture(pixels, width, height, THREE.RGBAFormat, THREE.HalfFloatType);
  texture.colorSpace = THREE.LinearSRGBColorSpace;
  texture.mapping = THREE.EquirectangularReflectionMapping;
  texture.minFilter = texture.magFilter = THREE.LinearFilter;
  texture.wrapS = THREE.RepeatWrapping;
  texture.needsUpdate = true;
  return texture;
}

export function applySky(renderer, scene, config, sunDirection) {
  const texture = skyTexture(config, sunDirection);
  scene.background?.dispose();
  scene.background = texture;
  if (!config.environmentIntensity) return;
  const generator = new THREE.PMREMGenerator(renderer);
  const lighting = config.environmentIntensity === 1 ? texture : skyTexture(config, sunDirection, config.environmentIntensity);
  try {
    scene.environment = generator.fromEquirectangular(lighting).texture;
  } finally {
    if (lighting !== texture) lighting.dispose();
    generator.dispose();
  }
}
