import * as THREE from 'three';
import { pointLightSettings } from './graphics-settings.js';

const SLOT_BUDGET = [2, 4, 8];
const NEAR_MARGIN = 24;
const FADE_DISTANCE = 16;

export class LocalLights {
  constructor(scene) {
    this.scene = scene;
    this.sources = new Map();
    this.slots = [];
    this.chosen = [];
    this.scale = new THREE.Vector3();
    this.offset = new THREE.Vector3();
    this.rotation = new THREE.Euler(0, 0, 0, 'ZYX');
  }
  register(entity) {
    const config = pointLightSettings(entity.props?.ai_light);
    if (config && entity.visible !== false) {
      this.sources.set(entity.id, { id: entity.id, config, position: new THREE.Vector3(), distance: 0, fade: 0 });
    }
  }
  remove(id) { this.sources.delete(id); }
  reset() {
    this.sources.clear();
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
  update(cameraPosition, transforms, level) {
    const budget = Math.min(SLOT_BUDGET[level], this.sources.size);
    this.resize(budget);
    if (!budget) return;
    const chosen = this.chosen;
    chosen.length = 0;
    for (const source of this.sources.values()) {
      const { id, config } = source;
      const entity = transforms.get(id);
      if (!entity) continue;
      const { position, rotation_deg, scale } = entity.transform;
      source.position.fromArray(position);
      if (config.offset.some(Boolean)) {
        const { degToRad } = THREE.MathUtils;
        this.rotation.set(degToRad(rotation_deg[0]), degToRad(rotation_deg[1]), degToRad(rotation_deg[2]));
        source.position.add(this.offset.fromArray(config.offset).multiply(this.scale.fromArray(scale)).applyEuler(this.rotation));
      }
      const distance = source.position.distanceTo(cameraPosition);
      const fade = 1 - THREE.MathUtils.smoothstep(distance, config.range + NEAR_MARGIN, config.range + NEAR_MARGIN + FADE_DISTANCE);
      if (!fade) continue;
      let index = chosen.length;
      while (index > 0 && (distance < chosen[index - 1].distance || (distance === chosen[index - 1].distance && id < chosen[index - 1].id))) index--;
      if (index >= budget) continue;
      source.distance = distance; source.fade = fade;
      chosen.splice(index, 0, source);
      if (chosen.length > budget) chosen.pop();
    }
    this.slots.forEach((light, index) => {
      const source = chosen[index];
      light.intensity = source ? source.config.intensity * source.fade : 0;
      if (!source) return;
      light.color.set(source.config.color);
      light.distance = source.config.range;
      light.position.copy(source.position);
    });
  }
}
