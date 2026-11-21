import * as THREE from 'three';
import { MAX_LOCAL_LIGHT_RANGE, localLightSettings } from './graphics-settings.js';

const SLOT_BUDGET = { point: [2, 4, 8], spot: [1, 2, 4] };
const TYPES = Object.keys(SLOT_BUDGET);
const NEAR_MARGIN = 24;
const FADE_DISTANCE = 16;
const CELL_SIZE = 64;
const SEARCH_RADIUS = MAX_LOCAL_LIGHT_RANGE + NEAR_MARGIN + FADE_DISTANCE;
const cell = value => Math.floor(value / CELL_SIZE);
const cellKey = (x, z) => `${x},${z}`;

export class LocalLights {
  constructor(scene) {
    this.scene = scene;
    this.sources = new Map();
    this.cells = new Map();
    this.slots = { point: [], spot: [] };
    this.chosen = { point: [], spot: [] };
    this.counts = { point: 0, spot: 0 };
    this.scale = new THREE.Vector3();
    this.offset = new THREE.Vector3();
    this.rotation = new THREE.Euler(0, 0, 0, 'ZYX');
  }
  register(entity) {
    const config = localLightSettings(entity.props?.ai_light);
    this.remove(entity.id);
    if (config && entity.visible !== false) {
      this.sources.set(entity.id, {
        id: entity.id, config, position: new THREE.Vector3(), color: new THREE.Color(config.color),
        direction: config.type === 'spot' ? new THREE.Vector3().fromArray(config.axis) : null,
        key: null, distanceSq: 0,
      });
      this.counts[config.type]++;
      if (entity.transform) this.move(entity.id, entity.transform);
    }
  }
  move(id, { position, rotation_deg, scale }) {
    const source = this.sources.get(id);
    if (!source) return;
    source.position.fromArray(position);
    if (source.direction || source.config.offset.some(Boolean)) {
      const { degToRad } = THREE.MathUtils;
      this.rotation.set(degToRad(rotation_deg[0]), degToRad(rotation_deg[1]), degToRad(rotation_deg[2]));
      source.position.add(this.offset.fromArray(source.config.offset).multiply(this.scale.fromArray(scale)).applyEuler(this.rotation));
      source.direction?.fromArray(source.config.axis).applyEuler(this.rotation);
    }
    const key = cellKey(cell(source.position.x), cell(source.position.z));
    if (key === source.key) return;
    this.detach(source);
    let bucket = this.cells.get(key);
    if (!bucket) this.cells.set(key, bucket = new Set());
    bucket.add(source);
    source.key = key;
  }
  detach(source) {
    const bucket = this.cells.get(source.key);
    if (!bucket) return;
    bucket.delete(source);
    if (!bucket.size) this.cells.delete(source.key);
  }
  remove(id) {
    const source = this.sources.get(id);
    if (source) {
      this.detach(source);
      this.counts[source.config.type]--;
    }
    this.sources.delete(id);
  }
  reset() {
    this.sources.clear();
    this.cells.clear();
    for (const type of TYPES) {
      this.chosen[type].length = 0;
      this.counts[type] = 0;
      this.resize(type, 0);
    }
  }
  resize(type, count) {
    const slots = this.slots[type];
    while (slots.length < count) {
      const light = type === 'spot' ? new THREE.SpotLight(0xffffff, 0) : new THREE.PointLight(0xffffff, 0);
      this.scene.add(light);
      if (light.target) this.scene.add(light.target);
      slots.push(light);
    }
    while (slots.length > count) {
      const light = slots.pop();
      light.target?.removeFromParent();
      light.removeFromParent();
    }
  }
  update(focus, level) {
    for (const type of TYPES) {
      this.resize(type, Math.min(SLOT_BUDGET[type][level], this.counts[type]));
      this.chosen[type].length = 0;
    }
    if (!this.sources.size) return;
    const { x, z } = focus;
    const minX = cell(x - SEARCH_RADIUS), minZ = cell(z - SEARCH_RADIUS);
    const maxX = cell(x + SEARCH_RADIUS), maxZ = cell(z + SEARCH_RADIUS);
    if (![minX, minZ, maxX, maxZ].every(Number.isSafeInteger)) {
      for (const type of TYPES) for (const light of this.slots[type]) light.intensity = 0;
      return;
    }
    for (let cx = minX; cx <= maxX; cx++) {
      for (let cz = minZ; cz <= maxZ; cz++) {
        const bucket = this.cells.get(cellKey(cx, cz));
        if (!bucket) continue;
        for (const source of bucket) {
          const { type } = source.config;
          const budget = this.slots[type].length;
          const chosen = this.chosen[type];
          const reach = source.config.range + NEAR_MARGIN + FADE_DISTANCE;
          const distanceSq = source.position.distanceToSquared(focus);
          if (distanceSq >= reach * reach) continue;
          let index = chosen.length;
          while (index > 0 && (distanceSq < chosen[index - 1].distanceSq || (distanceSq === chosen[index - 1].distanceSq && source.id < chosen[index - 1].id))) index--;
          if (index >= budget) continue;
          source.distanceSq = distanceSq;
          chosen.splice(index, 0, source);
          if (chosen.length > budget) chosen.pop();
        }
      }
    }
    for (const type of TYPES) this.slots[type].forEach((light, index) => {
      const source = this.chosen[type][index];
      if (!source) { light.intensity = 0; return; }
      const { range, intensity } = source.config;
      light.intensity = intensity * (1 - THREE.MathUtils.smoothstep(
        Math.sqrt(source.distanceSq), range + NEAR_MARGIN, range + NEAR_MARGIN + FADE_DISTANCE,
      ));
      light.color.copy(source.color);
      light.distance = range;
      light.position.copy(source.position);
      if (light.isSpotLight) {
        light.angle = source.config.angle;
        light.penumbra = source.config.penumbra;
        light.target.position.copy(source.position).add(source.direction);
      }
    });
  }
}
