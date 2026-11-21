import * as THREE from 'three';
import { MAX_POINT_LIGHT_RANGE, pointLightSettings } from './graphics-settings.js';

const SLOT_BUDGET = [2, 4, 8];
const NEAR_MARGIN = 24;
const FADE_DISTANCE = 16;
const CELL_SIZE = 64;
const SEARCH_RADIUS = MAX_POINT_LIGHT_RANGE + NEAR_MARGIN + FADE_DISTANCE;
const cell = value => Math.floor(value / CELL_SIZE);
const cellKey = (x, z) => `${x},${z}`;

export class LocalLights {
  constructor(scene) {
    this.scene = scene;
    this.sources = new Map();
    this.cells = new Map();
    this.slots = [];
    this.chosen = [];
    this.scale = new THREE.Vector3();
    this.offset = new THREE.Vector3();
    this.rotation = new THREE.Euler(0, 0, 0, 'ZYX');
  }
  register(entity) {
    const config = pointLightSettings(entity.props?.ai_light);
    if (config && entity.visible !== false) {
      this.remove(entity.id);
      this.sources.set(entity.id, {
        id: entity.id, config, position: new THREE.Vector3(), color: new THREE.Color(config.color),
        key: null, distanceSq: 0,
      });
      if (entity.transform) this.move(entity.id, entity.transform);
    }
  }
  move(id, { position, rotation_deg, scale }) {
    const source = this.sources.get(id);
    if (!source) return;
    source.position.fromArray(position);
    if (source.config.offset.some(Boolean)) {
      const { degToRad } = THREE.MathUtils;
      this.rotation.set(degToRad(rotation_deg[0]), degToRad(rotation_deg[1]), degToRad(rotation_deg[2]));
      source.position.add(this.offset.fromArray(source.config.offset).multiply(this.scale.fromArray(scale)).applyEuler(this.rotation));
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
    if (source) this.detach(source);
    this.sources.delete(id);
  }
  reset() {
    this.sources.clear();
    this.cells.clear();
    this.chosen.length = 0;
    this.resize(0);
  }
  resize(count) {
    while (this.slots.length < count) {
      const light = new THREE.PointLight(0xffffff, 0);
      this.scene.add(light);
      this.slots.push(light);
    }
    while (this.slots.length > count) this.slots.pop().removeFromParent();
  }
  update(focus, level) {
    const budget = Math.min(SLOT_BUDGET[level], this.sources.size);
    this.resize(budget);
    const chosen = this.chosen;
    chosen.length = 0;
    if (!budget) return;
    const { x, z } = focus;
    const minX = cell(x - SEARCH_RADIUS), minZ = cell(z - SEARCH_RADIUS);
    const maxX = cell(x + SEARCH_RADIUS), maxZ = cell(z + SEARCH_RADIUS);
    if (![minX, minZ, maxX, maxZ].every(Number.isSafeInteger)) {
      for (const light of this.slots) light.intensity = 0;
      return;
    }
    for (let cx = minX; cx <= maxX; cx++) {
      for (let cz = minZ; cz <= maxZ; cz++) {
        const bucket = this.cells.get(cellKey(cx, cz));
        if (!bucket) continue;
        for (const source of bucket) {
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
    this.slots.forEach((light, index) => {
      const source = chosen[index];
      if (!source) { light.intensity = 0; return; }
      const { range, intensity } = source.config;
      light.intensity = intensity * (1 - THREE.MathUtils.smoothstep(
        Math.sqrt(source.distanceSq), range + NEAR_MARGIN, range + NEAR_MARGIN + FADE_DISTANCE,
      ));
      light.color.copy(source.color);
      light.distance = range;
      light.position.copy(source.position);
    });
  }
}
