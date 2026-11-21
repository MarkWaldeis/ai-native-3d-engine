import * as THREE from 'three';
import { SlotPool, tracerSpan } from './fps_core.js';

// Pooled shooter effects: fixed-size GPU buffers, no per-shot allocation.
const PRESETS = {
  muzzle: { count: 6, layer: 'spark', color: [1, 0.85, 0.45], speed: 4, spread: 0.5, life: 0.07, gravity: 0, light: [1, 0.75, 0.35, 6, 0.06] },
  impact: { count: 10, layer: 'spark', color: [1, 0.78, 0.4], speed: 5, spread: 0.9, life: 0.35, gravity: 9, puffs: 2, puffColor: [0.42, 0.38, 0.33] },
  blood: { count: 14, layer: 'spark', color: [0.62, 0.02, 0.02], speed: 3.2, spread: 0.8, life: 0.5, gravity: 9, puffs: 2, puffColor: [0.45, 0.02, 0.02] },
  explosion: { count: 48, layer: 'spark', color: [1, 0.55, 0.12], speed: 11, spread: 1, life: 0.8, gravity: 6, puffs: 10, puffColor: [0.55, 0.52, 0.48], light: [1, 0.55, 0.2, 40, 0.25] },
};

function texture() {
  const canvas = document.createElement('canvas'); canvas.width = canvas.height = 32;
  const context = canvas.getContext('2d'), glow = context.createRadialGradient(16, 16, 0, 16, 16, 16);
  glow.addColorStop(0, '#fff'); glow.addColorStop(0.35, '#fffa'); glow.addColorStop(1, '#fff0');
  context.fillStyle = glow; context.fillRect(0, 0, 32, 32);
  return new THREE.CanvasTexture(canvas);
}

class ParticleLayer {
  constructor(scene, size, capacity, map, blending) {
    this.capacity = capacity; this.pool = new SlotPool(capacity);
    this.positions = new Float32Array(capacity * 3); this.colors = new Float32Array(capacity * 3);
    this.velocity = new Float32Array(capacity * 3); this.base = new Float32Array(capacity * 3);
    this.life = new Float32Array(capacity); this.age = new Float32Array(capacity); this.gravity = new Float32Array(capacity);
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.BufferAttribute(this.positions, 3).setUsage(THREE.DynamicDrawUsage));
    geometry.setAttribute('color', new THREE.BufferAttribute(this.colors, 3).setUsage(THREE.DynamicDrawUsage));
    this.points = new THREE.Points(geometry, new THREE.PointsMaterial({ size, map, vertexColors: true, transparent: true, depthWrite: false, blending, sizeAttenuation: true, fog: false }));
    this.points.frustumCulled = false; this.points.renderOrder = 5; scene.add(this.points);
    this.alive = 0;
  }
  spawn(pos, dir, color, speed, spread, life, gravity) {
    const i = this.pool.take(), j = i * 3;
    const rx = Math.random() * 2 - 1, ry = Math.random() * 2 - 1, rz = Math.random() * 2 - 1;
    this.positions.set(pos, j);
    this.velocity[j] = (dir[0] + rx * spread) * speed * (0.4 + Math.random() * 0.6);
    this.velocity[j + 1] = (dir[1] + ry * spread) * speed * (0.4 + Math.random() * 0.6);
    this.velocity[j + 2] = (dir[2] + rz * spread) * speed * (0.4 + Math.random() * 0.6);
    this.base.set(color, j); this.colors.set(color, j);
    this.life[i] = life * (0.6 + Math.random() * 0.4); this.age[i] = 0; this.gravity[i] = gravity;
  }
  update(dt) {
    let alive = 0;
    for (let i = 0; i < this.capacity; i++) {
      if (this.age[i] >= this.life[i]) { if (this.colors[i * 3] || this.colors[i * 3 + 1] || this.colors[i * 3 + 2]) this.colors.fill(0, i * 3, i * 3 + 3); continue; }
      alive++; this.age[i] += dt; const j = i * 3, fade = Math.max(0, 1 - this.age[i] / this.life[i]);
      this.velocity[j + 1] -= this.gravity[i] * dt;
      for (let k = 0; k < 3; k++) { this.positions[j + k] += this.velocity[j + k] * dt; this.colors[j + k] = this.base[j + k] * fade; }
      if (this.age[i] >= this.life[i]) this.colors.fill(0, j, j + 3);
    }
    this.alive = alive;
    this.points.geometry.attributes.position.needsUpdate = true; this.points.geometry.attributes.color.needsUpdate = true;
  }
  reset() { this.life.fill(0); this.age.fill(0); this.colors.fill(0); this.pool.reset(); }
}

export class ShooterFx {
  constructor(scene, coarse = false) {
    const map = texture();
    this.sparks = new ParticleLayer(scene, 0.14, coarse ? 768 : 2048, map, THREE.AdditiveBlending);
    this.puffs = new ParticleLayer(scene, 0.9, coarse ? 96 : 256, map, THREE.NormalBlending);
    this.puffs.points.material.opacity = 0.3;
    this.tracerCount = 64; this.tracerPool = new SlotPool(this.tracerCount);
    this.tracerPositions = new Float32Array(this.tracerCount * 6); this.tracerColors = new Float32Array(this.tracerCount * 6);
    this.tracerAge = new Float32Array(this.tracerCount).fill(1);
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.BufferAttribute(this.tracerPositions, 3).setUsage(THREE.DynamicDrawUsage));
    geometry.setAttribute('color', new THREE.BufferAttribute(this.tracerColors, 3).setUsage(THREE.DynamicDrawUsage));
    this.tracers = new THREE.LineSegments(geometry, new THREE.LineBasicMaterial({ vertexColors: true, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending, fog: false }));
    this.tracers.frustumCulled = false; scene.add(this.tracers);
    this.lights = [0, 1, 2, 3].map(() => { const light = new THREE.PointLight(0xffaa55, 0, 12, 2); scene.add(light); return { light, life: 0, age: 1, peak: 0 }; });
    this.lightPool = new SlotPool(this.lights.length);
    this.emitted = 0;
  }
  /** Renders one `{kind,pos,dir}` request from Rhai `ctx.emit_fx`. Tracer `dir` spans start→end. */
  emit(fx, muzzleOverride) {
    this.emitted++;
    const pos = fx.kind === 'muzzle' && muzzleOverride ? muzzleOverride : fx.pos, dir = fx.dir || [0, 1, 0];
    if (fx.kind === 'tracer') { const span = tracerSpan(dir); this.tracer(muzzleOverride || pos, [fx.pos[0] + span[0], fx.pos[1] + span[1], fx.pos[2] + span[2]]); return; }
    const preset = PRESETS[fx.kind]; if (!preset) return;
    const length = Math.hypot(...dir) || 1, unit = dir.map(v => v / length);
    for (let i = 0; i < preset.count; i++) this.sparks.spawn(pos, unit, preset.color, preset.speed, preset.spread, preset.life, preset.gravity);
    for (let i = 0; i < (preset.puffs || 0); i++) this.puffs.spawn(pos, [0, 1, 0], preset.puffColor, preset.speed * 0.15, 1, preset.life * 2.5, -0.5);
    if (preset.light) {
      const slot = this.lights[this.lightPool.take()], [r, g, b, peak, life] = preset.light;
      slot.light.color.setRGB(r, g, b); slot.light.position.fromArray(pos); slot.peak = peak; slot.life = life; slot.age = 0;
    }
  }
  tracer(from, to) {
    const i = this.tracerPool.take(), j = i * 6;
    this.tracerPositions.set(from, j); this.tracerPositions.set(to, j + 3); this.tracerAge[i] = 0;
  }
  update(dt) {
    this.sparks.update(dt); this.puffs.update(dt);
    for (let i = 0; i < this.tracerCount; i++) {
      const fade = Math.max(0, 1 - this.tracerAge[i] / 0.09); this.tracerAge[i] += dt; const j = i * 6;
      this.tracerColors[j] = fade * 0.5; this.tracerColors[j + 1] = fade * 0.42; this.tracerColors[j + 2] = fade * 0.22;
      this.tracerColors[j + 3] = fade; this.tracerColors[j + 4] = fade * 0.85; this.tracerColors[j + 5] = fade * 0.45;
    }
    this.tracers.geometry.attributes.position.needsUpdate = true; this.tracers.geometry.attributes.color.needsUpdate = true;
    for (const slot of this.lights) { slot.age += dt; slot.light.intensity = slot.age < slot.life ? slot.peak * (1 - slot.age / slot.life) : 0; }
  }
  stats() { return { sparks: this.sparks.alive, puffs: this.puffs.alive, emitted: this.emitted }; }
  reset() { this.sparks.reset(); this.puffs.reset(); this.tracerAge.fill(1); for (const slot of this.lights) { slot.age = 1; slot.light.intensity = 0; } }
}
