import { daylightDefaults } from './nature-settings.js';

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

export function graphicsConfig(authored, preset, skyColor, deprecatedAlias) {
  const primary = object(authored), alias = object(deprecatedAlias);
  const value = primary || alias ? {...primary,...alias} : null;
  if (preset === 'high' || preset === 'medium') preset = 'balanced';
  const quality = ['low', 'balanced', 'ultra'].indexOf(preset);
  if (preset === 'legacy' || (!value && quality < 0)) return null;
  const config = {...daylightDefaults(value?.time_of_day),...value};
  const foliage = object(config.foliage), water = object(config.water);
  return {
    quality,
    sky: hex(config.sky, '#4d88cb'),
    horizon: hex(config.horizon, hex(skyColor, '#b8cddd')),
    ground: hex(config.ground, '#626457'),
    sunColor: hex(config.sun_color, '#ffe5c1'),
    sunDirection: direction(config.sun_direction),
    sunIntensity: bounded(config.sun_intensity, 3, 0, 12),
    environmentIntensity: bounded(config.environment_intensity, 0.66, 0, 3),
    environmentMap: config.environment_map === 'assets/environment.hdr' ? config.environment_map : null,
    environmentBackground: config.environment_background !== false,
    exposure: bounded(config.exposure, 1, 0.1, 4),
    toneMapping: config.tone_mapping === 'agx' ? 'agx' : 'aces',
    hdrAntialias: config.hdr_antialias !== false,
    contactStrength: bounded(config.contact_occlusion, 0, 0, 1),
    contactRadius: bounded(config.contact_radius, 1.2, 0.1, 3),
    bloomStrength: bounded(config.bloom_strength, 0.22, 0, 2),
    bloomThreshold: bounded(config.bloom_threshold, 1.1, 0, 8),
    foliage: config.foliage === true || foliage ? {density:bounded(foliage?.density,1,0,4),distance:bounded(foliage?.distance,64,16,128)} : false,
    water: water ? {level:bounded(water.level,0,-10000,10000),color:hex(water.color,'#126e61')} : false,
    fogDensity: Number.isFinite(config.fog_density) ? bounded(config.fog_density,0.004,0,0.2) : undefined,
  };
}

export function hdrSettings(level, supported, strength, width, height, pixelRatio, antialias = false, contactOcclusion = false) {
  const bloom = level > 0 && supported && strength > 0;
  const contact = level === 2 && supported && contactOcclusion;
  const enabled = bloom || contact || (level === 2 && supported && antialias);
  return {
    enabled, bloom, contact,
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

export const MAX_POINT_LIGHT_RANGE = 64;

export function pointLightSettings(authored) {
  const value = object(authored);
  if (!value) return null;
  const intensity = bounded(value.intensity, 16, 0, 1000);
  if (intensity === 0) return null;
  const offset = Array.isArray(value.offset) && value.offset.length === 3 && value.offset.every(Number.isFinite)
    ? value.offset.map(component => bounded(component, 0, -32, 32))
    : [0, 0, 0];
  return {
    color: hex(value.color, '#ffffff').toLowerCase(),
    intensity,
    range: bounded(value.range, 8, 0.25, MAX_POINT_LIGHT_RANGE),
    offset,
  };
}
