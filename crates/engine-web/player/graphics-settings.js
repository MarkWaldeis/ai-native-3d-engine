function object(value) {
  if (typeof value === 'string') {
    try { value = JSON.parse(value); } catch { return null; }
  }
  return value && typeof value === 'object' && !Array.isArray(value) ? value : null;
}

function bounded(value, fallback, min, max) {
  return Number.isFinite(value) ? Math.min(max, Math.max(min, value)) : fallback;
}

function hex(value, fallback) {
  return typeof value === 'string' && /^#[0-9a-f]{6}$/i.test(value) ? value : fallback;
}

const defaultSun = [-6, 16, 9];

function direction(value) {
  if (!Array.isArray(value) || value.length !== 3 || !value.every(Number.isFinite)) return defaultSun;
  const length = Math.hypot(...value);
  return length > 0 && Number.isFinite(length) ? value.map(v => v / length) : defaultSun;
}

export function graphicsConfig(authored, preset, skyColor) {
  const value = object(authored);
  const quality = ['low', 'balanced', 'ultra'].indexOf(preset);
  if (preset === 'legacy' || (!value && quality < 0)) return null;
  const config = value ?? {};
  return {
    quality,
    sky: hex(config.sky, '#4d88cb'),
    horizon: hex(config.horizon, hex(skyColor, '#b8cddd')),
    ground: hex(config.ground, '#626457'),
    sunColor: hex(config.sun_color, '#ffe5c1'),
    sunDirection: direction(config.sun_direction),
    sunIntensity: bounded(config.sun_intensity, 3, 0, 12),
    environmentIntensity: bounded(config.environment_intensity, 0.66, 0, 3),
    exposure: bounded(config.exposure, 1, 0.1, 4),
    bloomStrength: bounded(config.bloom_strength, 0.22, 0, 2),
    bloomThreshold: bounded(config.bloom_threshold, 1.1, 0, 8),
  };
}

export function hdrSettings(level, supported, strength, width, height, pixelRatio) {
  const enabled = level > 0 && supported && strength > 0;
  return {
    enabled,
    pixelRatio: enabled ? Math.min(pixelRatio, Math.sqrt(2_600_000 / (width * height))) : pixelRatio,
    bloomScale: level === 2 ? 0.5 : 0.25,
  };
}

export function materialSettings(authored) {
  const value = object(authored);
  return {
    roughness: bounded(value?.roughness, 0.55, 0, 1),
    metalness: bounded(value?.metalness, 0.08, 0, 1),
    emissive: hex(value?.emissive, '#000000').toLowerCase(),
    emissiveIntensity: bounded(value?.emissive_intensity, 1, 0, 32),
    clearcoat: bounded(value?.clearcoat, 0, 0, 1),
    clearcoatRoughness: bounded(value?.clearcoat_roughness, 0.2, 0, 1),
  };
}
