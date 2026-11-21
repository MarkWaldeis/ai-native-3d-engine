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
  if (Array.isArray(value) && value.length === 3 && value.every(v => Number.isFinite(v) && v >= 0 && v <= 1)) {
    return '#' + value.map(v => Math.round(v * 255).toString(16).padStart(2, '0')).join('');
  }
  return typeof value === 'string' && /^#[0-9a-f]{6}$/i.test(value) ? value : fallback;
}

const skyPresets = {
  tropical: {sky:'#246a9b', horizon:'#d5cbb0', ground:'#586447', sun_color:'#ffe2af'},
};

const defaultSun = [-6, 16, 9];

function direction(value, fallback = defaultSun) {
  if (!Array.isArray(value) || value.length !== 3 || !value.every(Number.isFinite)) return fallback;
  const length = Math.hypot(...value);
  return length > 0 && Number.isFinite(length) ? value.map(v => v / length) : fallback;
}

export function graphicsConfig(authored, preset, skyColor, deprecatedAlias) {
  const primary = object(authored), alias = object(deprecatedAlias);
  const value = primary || alias ? {...primary,...alias} : null;
  if (preset === 'high' || preset === 'medium') preset = 'balanced';
  const quality = ['low', 'balanced', 'ultra'].indexOf(preset);
  if (preset === 'legacy' || (!value && quality < 0)) return null;
  const skyDefaults = skyPresets[value?.sky_preset] || skyPresets[value?.sky];
  const config = {...daylightDefaults(value?.time_of_day),...skyDefaults,...value};
  if (skyPresets[value?.sky]) config.sky = skyDefaults.sky;
  const foliage = object(config.foliage), water = object(config.water), clouds = object(config.sky_clouds);
  const atmosphere = object(config.sky_atmosphere);
  const fog = object(config.height_fog);
  const horizon = hex(config.horizon, hex(skyColor, '#b8cddd'));
  return {
    quality,
    sky: hex(config.sky, '#4d88cb'),
    skyClouds: config.sky_clouds === true || clouds ? {
      coverage: bounded(clouds?.coverage, 0.45, 0, 1),
      scale: bounded(clouds?.scale, 1, 0.25, 4),
      opacity: bounded(clouds?.opacity, 0.85, 0, 1),
      seed: Math.trunc(bounded(clouds?.seed, 0, 0, 65535)),
    } : null,
    horizon,
    heightFog: config.height_fog === true || fog ? {
      density: bounded(fog?.density, 0.018, 0, 0.2),
      baseHeight: bounded(fog?.base_height, 0, -10000, 10000),
      falloff: bounded(fog?.falloff, 18, 1, 1000),
      color: hex(fog?.color, horizon),
    } : null,
    skyAtmosphere: config.sky_atmosphere === true || atmosphere ? {
      strength: bounded(atmosphere?.strength, 1, 0, 1),
      haze: bounded(atmosphere?.haze, 0.35, 0, 1),
    } : null,
    ground: hex(config.ground, '#626457'),
    sunColor: hex(config.sun_color, '#ffe5c1'),
    sunDirection: direction(config.sun_direction),
    sunIntensity: bounded(config.sun_intensity, 3, 0, 12),
    shadowExtent: bounded(config.shadow_extent, 24, 8, 160),
    shadowExtents: ['low','balanced','ultra'].map(t=>bounded(object(config.shadow_extent)?.[t],bounded(config.shadow_extent,24,8,160),8,160)),
    shadowMapSizes: ['low','balanced','ultra'].map((t,i)=>2**Math.round(Math.log2(bounded(object(config.shadow_map_size)?.[t],512<<i,256,4096)))),
    environmentIntensity: bounded(config.environment_intensity, 0.66, 0, 3),
    environmentMap: config.environment_map === 'assets/environment.hdr' ? config.environment_map : null,
    environmentBackground: config.environment_background !== false,
    environmentRotation: Number.isFinite(config.environment_rotation_deg) ? config.environment_rotation_deg % 360 : 0,
    exposure: bounded(config.exposure, 1, 0.1, 4),
    autoExposure: autoExposureSettings(config.auto_exposure),
    toneMapping: config.tone_mapping === 'agx' ? 'agx' : 'aces',
    colorGrade: colorGradeSettings(config.color_grade),
    logarithmicDepth: config.logarithmic_depth === true,
    renderScale: bounded(config.render_scale, 1, 0.5, 1),
    hdrAntialias: config.hdr_antialias !== false,
    contactStrength: bounded(config.contact_occlusion, 0, 0, 1),
    contactRadius: bounded(config.contact_radius, 1.2, 0.1, 3),
    bloomStrength: bounded(config.bloom_strength, 0.22, 0, 2),
    bloomThreshold: bounded(config.bloom_threshold, 1.1, 0, 8),
    bloomScatter: bounded(config.bloom_scatter, 0, 0, 1),
    foliage: config.foliage === true || foliage ? {density:bounded(foliage?.density,1,0,4),trees:bounded(foliage?.trees,1,0,1),distance:bounded(foliage?.distance,112,16,128)} : false,
    clouds: config.clouds === true || object(config.clouds) ? {coverage:bounded(object(config.clouds)?.coverage,0.48,0,1)} : false,
    haze: config.haze === true || object(config.haze) ? {density:bounded(object(config.haze)?.density,0.002,0,0.02),height:bounded(object(config.haze)?.height,18,-1000,10000),falloff:bounded(object(config.haze)?.falloff,0.045,0.001,1)} : false,
    water: water ? {level:bounded(water.level,0,-10000,10000),color:hex(water.color,'#126e61')} : false,
    fogDensity: Number.isFinite(config.fog_density) ? bounded(config.fog_density,0.004,0,0.2) : undefined,
  };
}

export function sunShadowSettings(authored) {
  const value = object(authored);
  return { castShadow: value?.cast !== false, receiveShadow: value?.receive !== false };
}

function autoExposureSettings(authored) {
  const value = object(authored);
  if (authored !== true && !value) return null;
  return {
    minEV: bounded(value?.min_ev, -2, -4, 0),
    maxEV: bounded(value?.max_ev, 2, 0, 4),
    brightenSpeed: bounded(value?.brighten_speed, 1.25, 0.1, 8),
    darkenSpeed: bounded(value?.darken_speed, 3, 0.1, 8),
  };
}

function colorGradeSettings(authored) {
  const value = object(authored);
  if (!value) return null;
  const saturation = bounded(value.saturation, 1, 0, 2);
  const contrast = bounded(value.contrast, 1, 0, 2);
  return saturation !== 1 || contrast !== 1 ? {saturation, contrast} : null;
}

export function hdrSettings(level, supported, strength, width, height, pixelRatio, antialias = false, contactOcclusion = false, resolveEffects = false) {
  const bloom = level > 0 && supported && strength > 0;
  const contact = level === 2 && supported && contactOcclusion;
  const enabled = bloom || contact || (supported && ((level === 2 && antialias) || (level > 0 && resolveEffects)));
  return {
    enabled, bloom, contact,
    pixelRatio: enabled ? Math.min(pixelRatio, Math.sqrt(2_600_000 / (width * height))) : pixelRatio,
    bloomScale: level === 2 ? 0.5 : 0.25,
  };
}

export function materialMapSettings(authored) {
  const value = object(authored);
  if (!value) return null;
  const path = key => typeof value[key] === 'string' && /^assets\/material\/\d+\.png$/.test(value[key]) ? value[key] : null;
  const map = path('base_color_map'), normalMap = path('normal_map'), ormMap = path('orm_map');
  if (!map && !normalMap && !ormMap) return null;
  const repeat = value.uv_repeat;
  return {
    map, normalMap, ormMap,
    repeat: Array.isArray(repeat) && repeat.length === 2
      ? repeat.map(n => bounded(n, 1, 1 / 16, 64)) : [1, 1],
    normalScale: normalMap ? bounded(value.normal_scale, 1, 0, 2) : 1,
  };
}

export function materialSettings(authored, packedOrm = false) {
  const value = object(authored);
  const anisotropy = bounded(value?.anisotropy, 0, 0, 1);
  const sheen = bounded(value?.sheen, 0, 0, 1);
  const iridescence = bounded(value?.iridescence, 0, 0, 1);
  const transmission = bounded(value?.transmission, 0, 0, 1);
  const film = bounded(value?.iridescence_thickness_nm, 400, 100, 1200);
  return {
    roughness: bounded(value?.roughness, packedOrm ? 1 : 0.55, 0, 1),
    metalness: bounded(value?.metalness, packedOrm ? 1 : 0.08, 0, 1),
    emissive: hex(value?.emissive, '#000000').toLowerCase(),
    emissiveIntensity: bounded(value?.emissive_intensity, 1, 0, 32),
    clearcoat: bounded(value?.clearcoat, 0, 0, 1),
    clearcoatRoughness: bounded(value?.clearcoat_roughness, 0.2, 0, 1),
    ...(anisotropy > 0 ? {
      anisotropy,
      anisotropyRotation: (Number.isFinite(value.anisotropy_rotation_deg) ? value.anisotropy_rotation_deg % 360 : 0) * Math.PI / 180,
    } : {}),
    ...(sheen > 0 ? {
      sheen, sheenColor: hex(value.sheen_color, '#ffffff').toLowerCase(),
      sheenRoughness: bounded(value.sheen_roughness, 0.6, 0, 1),
    } : {}),
    ...(iridescence > 0 ? {
      iridescence, iridescenceIOR: bounded(value.iridescence_ior, 1.3, 1, 2.5),
      iridescenceThicknessRange: [film, film],
    } : {}),
    ...(transmission > 0 ? {
      transmission, ior: bounded(value.ior, 1.5, 1, 2.333),
      thickness: bounded(value.thickness, 0.25, 0, 50),
      attenuationColor: hex(value.attenuation_color, '#ffffff').toLowerCase(),
      ...(Number.isFinite(value.attenuation_distance)
        ? {attenuationDistance: bounded(value.attenuation_distance, 1, 0.01, 1000)} : {}),
    } : {}),
  };
}

export const MAX_LOCAL_LIGHT_RANGE = 64;

export function localLightSettings(authored) {
  const value = object(authored);
  if (!value) return null;
  const intensity = bounded(value.intensity, 16, 0, 1000);
  if (intensity === 0) return null;
  const type = value.type ?? 'point';
  if (type !== 'point' && type !== 'spot') return null;
  const offset = Array.isArray(value.offset) && value.offset.length === 3 && value.offset.every(Number.isFinite)
    ? value.offset.map(component => bounded(component, 0, -32, 32))
    : [0, 0, 0];
  const angle = bounded(value.outer_angle, 35, 1, 89);
  return {
    type,
    color: hex(value.color, '#ffffff').toLowerCase(),
    intensity,
    range: bounded(value.range, 8, 0.25, MAX_LOCAL_LIGHT_RANGE),
    offset,
    ...(type === 'spot' ? {
      castShadow: value.cast_shadow === true,
      axis: direction(value.direction, [0, 0, -1]),
      angle: angle * Math.PI / 180,
      penumbra: 1 - bounded(value.inner_angle, angle * 0.65, 0, angle * 0.999) / angle,
    } : {}),
  };
}
