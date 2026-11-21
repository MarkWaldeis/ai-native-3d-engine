import * as THREE from 'three';
import { SlotPool } from './fps_core.js';

export const FX_BUDGETS = Object.freeze([
  Object.freeze({ sparks: 96, puffs: 12, flashes: 2, fire: 2, decals: 0, tracers: 8, lights: 0, density: 0.35 }),
  Object.freeze({ sparks: 320, puffs: 40, flashes: 6, fire: 6, decals: 12, tracers: 24, lights: 1, density: 0.65 }),
  Object.freeze({ sparks: 768, puffs: 96, flashes: 12, fire: 12, decals: 24, tracers: 48, lights: 3, density: 1 }),
]);
const UP = [0, 1, 0], FIRE = [2, 0.65, 0.12];

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

// Soft star-shaped flash: bright core, short spikes, fully transparent edges.
export function flashTexture() {
  const size = 64, half = size / 2, canvas = document.createElement('canvas'); canvas.width = canvas.height = size;
  const context = canvas.getContext('2d'), glow = context.createRadialGradient(half, half, 0, half, half, half);
  glow.addColorStop(0, '#fffbe8'); glow.addColorStop(0.18, '#ffd27acc'); glow.addColorStop(0.5, '#ff8a2440'); glow.addColorStop(1, '#ff6a0000');
  context.fillStyle = glow; context.fillRect(0, 0, size, size);
  context.globalCompositeOperation = 'lighter'; context.translate(half, half);
  for (let i = 0; i < 6; i++) {
    context.rotate(Math.PI / 3);
    const spike = context.createLinearGradient(0, 0, half * 0.95, 0);
    spike.addColorStop(0, '#ffe7a8aa'); spike.addColorStop(1, '#ff8a2400');
    context.fillStyle = spike; context.beginPath(); context.moveTo(0, -2.5); context.lineTo(half * 0.95, 0); context.lineTo(0, 2.5); context.fill();
  }
  const map = new THREE.CanvasTexture(canvas); map.colorSpace = THREE.SRGBColorSpace;
  return map;
}

class ParticleLayer {
  constructor(scene, capacity, map, blending) {
    this.capacity = this.limit = capacity; this.pool = new SlotPool(capacity);
    this.positions = new Float32Array(capacity * 3); this.colors = new Float32Array(capacity * 3);
    this.velocity = new Float32Array(capacity * 3);
    this.life = new Float32Array(capacity); this.age = new Float32Array(capacity); this.gravity = new Float32Array(capacity);
    this.alpha = new Float32Array(capacity); this.sizes = new Float32Array(capacity); this.growth = new Float32Array(capacity);
    this.startSize = new Float32Array(capacity); this.opacity = new Float32Array(capacity); this.fresh = new Uint8Array(capacity);
    this.angles = new Float32Array(capacity);
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.BufferAttribute(this.positions, 3).setUsage(THREE.DynamicDrawUsage));
    geometry.setAttribute('color', new THREE.BufferAttribute(this.colors, 3).setUsage(THREE.DynamicDrawUsage));
    geometry.setAttribute('particleAlpha', new THREE.BufferAttribute(this.alpha, 1).setUsage(THREE.DynamicDrawUsage));
    geometry.setAttribute('radius', new THREE.BufferAttribute(this.sizes, 1).setUsage(THREE.DynamicDrawUsage));
    geometry.setAttribute('angle', new THREE.BufferAttribute(this.angles, 1).setUsage(THREE.DynamicDrawUsage));
    const material = new THREE.ShaderMaterial({
      uniforms: { map: { value: map }, scale: { value: 450 }, core: { value: blending === THREE.AdditiveBlending ? 0.35 : 0 } },
      vertexShader: `#include <common>
        #include <logdepthbuf_pars_vertex>
        attribute vec3 color; attribute float particleAlpha; attribute float radius; attribute float angle;
        uniform float scale; varying vec3 tint; varying float alpha; varying float rotation;
        void main(){vec4 p=modelViewMatrix*vec4(position,1.);gl_Position=projectionMatrix*p;
          gl_PointSize=clamp(radius*scale/max(.05,-p.z),1.,96.);tint=color;alpha=particleAlpha;rotation=angle;
          #include <logdepthbuf_vertex>
        }`,
      fragmentShader: `#include <logdepthbuf_pars_fragment>
        uniform sampler2D map; uniform float core; varying vec3 tint; varying float alpha; varying float rotation;
        void main(){vec2 p=gl_PointCoord-.5;float c=cos(rotation),s=sin(rotation);
          vec2 uv=mat2(c,-s,s,c)*p+.5;float a=texture2D(map,uv).a*alpha;
          if(any(lessThan(uv,vec2(0.)))||any(greaterThan(uv,vec2(1.)))||a<.01)discard;
          #include <logdepthbuf_fragment>
          gl_FragColor=vec4(mix(tint,vec3(2.),core*pow(max(0.,1.-length(p)*2.),4.)),a);}`,
      transparent: true, depthWrite: false, blending, toneMapped: false, fog: false,
    });
    this.points = new THREE.Points(geometry, material);
    this.points.frustumCulled = false; this.points.renderOrder = 5; scene.add(this.points);
    this.alive = 0; this.points.visible = false; this.dirty = false;
  }
  spawn(pos, dir, color, speed, spread, life, gravity, size, growth = 0, opacity = 1) {
    if (!this.limit) return;
    const i = this.pool.take(), j = i * 3;
    const rx = Math.random() * 2 - 1, ry = Math.random() * 2 - 1, rz = Math.random() * 2 - 1;
    this.positions.set(pos, j);
    this.velocity[j] = (dir[0] + rx * spread) * speed * (0.4 + Math.random() * 0.6);
    this.velocity[j + 1] = (dir[1] + ry * spread) * speed * (0.4 + Math.random() * 0.6);
    this.velocity[j + 2] = (dir[2] + rz * spread) * speed * (0.4 + Math.random() * 0.6);
    this.colors.set(color, j);
    this.life[i] = life * (0.6 + Math.random() * 0.4); this.age[i] = 0; this.gravity[i] = gravity;
    this.startSize[i] = this.sizes[i] = size * (0.75 + Math.random() * 0.5); this.growth[i] = growth;
    this.opacity[i] = this.alpha[i] = opacity; this.angles[i] = Math.random() * Math.PI * 2; this.fresh[i] = 1;
    this.dirty = true;
  }
  update(dt) {
    let alive = 0;
    for (let i = 0; i < this.limit; i++) {
      if (this.age[i] >= this.life[i]) continue;
      const step = this.fresh[i] ? 0 : dt; this.fresh[i] = 0; this.age[i] += step;
      const j = i * 3, fade = Math.max(0, 1 - this.age[i] / this.life[i]);
      this.velocity[j + 1] -= this.gravity[i] * step;
      for (let k = 0; k < 3; k++) this.positions[j + k] += this.velocity[j + k] * step;
      this.sizes[i] = this.startSize[i] + this.growth[i] * this.age[i]; this.alpha[i] = this.opacity[i] * fade * fade;
      if (fade > 0) alive++;
    }
    if (alive || this.alive || this.dirty) {
      for (const attribute of Object.values(this.points.geometry.attributes)) attribute.needsUpdate = true;
    }
    this.alive = alive; this.dirty = false; this.points.visible = alive > 0;
    this.points.geometry.setDrawRange(0, this.pool.used);
  }
  reset(limit = this.limit) {
    this.limit = this.pool.size = limit; this.pool.reset(); this.life.fill(0); this.age.fill(0); this.alpha.fill(0); this.fresh.fill(0);
    this.alive = 0; this.points.visible = false; this.points.geometry.setDrawRange(0, 0);
  }
}

export class ShooterFx {
  constructor(scene, coarse = false) {
    const map = texture(), smoke = smokeTexture();
    this.sparks = new ParticleLayer(scene, 768, map, THREE.AdditiveBlending);
    this.puffs = new ParticleLayer(scene, 96, smoke, THREE.NormalBlending);
    this.flashes = new ParticleLayer(scene, 12, flashTexture(), THREE.AdditiveBlending);
    this.fire = new ParticleLayer(scene, 12, map, THREE.AdditiveBlending);
    this.layers = [this.sparks, this.puffs, this.flashes, this.fire];
    this.direction = new Float32Array(3);
    this.tracerCount = 48; this.tracerPool = new SlotPool(this.tracerCount);
    this.tracerPositions = new Float32Array(this.tracerCount * 6); this.tracerColors = new Float32Array(this.tracerCount * 6);
    this.tracerFrom = new Float32Array(this.tracerCount * 3); this.tracerDelta = new Float32Array(this.tracerCount * 3);
    this.tracerAge = new Float32Array(this.tracerCount).fill(1); this.tracerFresh = new Uint8Array(this.tracerCount);
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.BufferAttribute(this.tracerPositions, 3).setUsage(THREE.DynamicDrawUsage));
    geometry.setAttribute('color', new THREE.BufferAttribute(this.tracerColors, 3).setUsage(THREE.DynamicDrawUsage));
    this.tracers = new THREE.LineSegments(geometry, new THREE.LineBasicMaterial({ vertexColors: true, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending, fog: false, toneMapped: false }));
    this.tracers.frustumCulled = false; scene.add(this.tracers);
    this.lights = [0, 1, 2].map(() => { const light = new THREE.PointLight(0xffaa55, 0, 12, 2); scene.add(light); return { light, life: 0, age: 1, peak: 0, fresh: false }; });
    this.lightPool = new SlotPool(this.lights.length);
    this.decalPool = new SlotPool(24); this.decalAge = new Float32Array(24).fill(6); this.decalAlpha = new Float32Array(24); this.decalFresh = new Uint8Array(24);
    const plane = new THREE.PlaneGeometry(1, 1);
    plane.setAttribute('decalAlpha', new THREE.InstancedBufferAttribute(this.decalAlpha, 1).setUsage(THREE.DynamicDrawUsage));
    this.decals = new THREE.InstancedMesh(plane, new THREE.ShaderMaterial({
      uniforms: { map: { value: smoke } },
      vertexShader: `#include <common>
        #include <logdepthbuf_pars_vertex>
        attribute float decalAlpha; varying vec2 vUv; varying float alpha;
        void main(){vUv=uv;alpha=decalAlpha;gl_Position=projectionMatrix*modelViewMatrix*instanceMatrix*vec4(position,1.);
          #include <logdepthbuf_vertex>
        }`,
      fragmentShader: `#include <logdepthbuf_pars_fragment>
        uniform sampler2D map;varying vec2 vUv;varying float alpha;
        void main(){float a=texture2D(map,vUv).a*alpha;if(a<.01)discard;
          #include <logdepthbuf_fragment>
          gl_FragColor=vec4(.055,.045,.035,a);}`,
      transparent: true, depthWrite: false, side: THREE.DoubleSide, polygonOffset: true, polygonOffsetFactor: -2, polygonOffsetUnits: -2,
    }), 24);
    this.decals.frustumCulled = false; this.decals.instanceMatrix.setUsage(THREE.DynamicDrawUsage); scene.add(this.decals);
    this.normal = new THREE.Vector3(); this.point = new THREE.Vector3(); this.scale = new THREE.Vector3(); this.rotation = new THREE.Quaternion(); this.matrix = new THREE.Matrix4(); this.axis = new THREE.Vector3(0, 0, 1);
    this.emitted = 0; this.setQuality(coarse ? 0 : 1);
  }
  /** Renders one `{kind,pos,dir}` request from Rhai `ctx.emit_fx`. Tracer `dir` spans start→end. */
  emit(fx, muzzleOverride, localMuzzle = false) {
    if (!fx || fx.kind !== 'tracer' && !Object.hasOwn(PRESETS, fx.kind)) return false;
    const pos = fx.kind === 'muzzle' && muzzleOverride ? muzzleOverride : fx.pos, dir = fx.dir || UP;
    for (let k = 0; k < 3; k++) if (!Number.isFinite(pos?.[k]) || !Number.isFinite(dir[k])) return false;
    this.emitted++;
    if (fx.kind === 'tracer') { this.tracer(muzzleOverride || pos, pos, dir); return true; }
    const preset = PRESETS[fx.kind], length = Math.hypot(dir[0], dir[1], dir[2]) || 1, unit = this.direction, budget = FX_BUDGETS[this.level];
    for (let k = 0; k < 3; k++) unit[k] = dir[k] / length;
    if (!localMuzzle) {
      for (let i = 0; i < Math.ceil(preset.count * budget.density); i++) this.sparks.spawn(pos, unit, preset.color, preset.speed, preset.spread, preset.life, preset.gravity, .09);
      for (let i = 0; i < Math.ceil((preset.puffs || 0) * budget.density); i++) this.puffs.spawn(pos, UP, preset.puffColor, fx.kind === 'blood' ? .5 : .35, 1, fx.kind === 'explosion' ? 2.4 : .8, -.15, fx.kind === 'explosion' ? 1.3 : .35, fx.kind === 'explosion' ? 1.8 : .65, .55);
      if (fx.kind === 'muzzle') this.flashes.spawn(pos, unit, preset.color, 0, 0, .055, 0, .55);
      if (fx.kind === 'explosion') for (let i = 0; i < Math.ceil(3 * budget.density); i++) this.fire.spawn(pos, UP, FIRE, 1.2, .5, .4, 0, 1.8, 2.5);
      if (fx.kind === 'impact' && budget.decals) this.mark(pos, unit);
    }
    if (preset.light && budget.lights) {
      const slot = this.lights[this.lightPool.take()], [r, g, b, peak, life] = preset.light;
      slot.light.color.setRGB(r, g, b); slot.light.position.fromArray(pos); slot.peak = peak; slot.life = life; slot.age = 0; slot.fresh = true;
    }
    return true;
  }
  tracer(from, pos, dir) {
    const length = Math.hypot(dir[0], dir[1], dir[2]), scale = length > 0 && length <= 1.001 ? 30 / length : 1;
    const i = this.tracerPool.take(), j = i * 3;
    for (let k = 0; k < 3; k++) { this.tracerFrom[j + k] = from[k]; this.tracerDelta[j + k] = pos[k] + dir[k] * scale - from[k]; }
    this.tracerAge[i] = 0; this.tracerFresh[i] = 1;
  }
  mark(pos, dir) {
    const i = this.decalPool.take(); this.normal.fromArray(dir); if (!this.normal.lengthSq()) this.normal.set(0, 1, 0);
    this.normal.normalize(); this.point.fromArray(pos).addScaledVector(this.normal, .018); this.rotation.setFromUnitVectors(this.axis, this.normal);
    this.scale.setScalar(.14 + Math.random() * .10); this.matrix.compose(this.point, this.rotation, this.scale); this.decals.setMatrixAt(i, this.matrix);
    this.decals.instanceMatrix.needsUpdate = true; this.decalAge[i] = 0; this.decalAlpha[i] = .7; this.decalFresh[i] = 1;
  }
  setQuality(level) {
    level = Number.isInteger(level) && level >= 0 && level <= 2 ? level : 0;
    if (this.level === level) return;
    this.level = level; const budget = FX_BUDGETS[level];
    this.sparks.reset(budget.sparks); this.puffs.reset(budget.puffs); this.flashes.reset(budget.flashes); this.fire.reset(budget.fire);
    this.tracerPool.size = budget.tracers; this.lightPool.size = budget.lights; this.decalPool.size = budget.decals;
    this.reset();
    for (let i = 0; i < this.lights.length; i++) this.lights[i].light.visible = i < budget.lights;
  }
  update(dt) {
    dt = Number.isFinite(dt) ? Math.max(0, Math.min(.1, dt)) : 0;
    for (const layer of this.layers) layer.update(dt);
    let tracers = 0, decals = 0;
    for (let i = 0; i < this.tracerPool.size; i++) {
      if (this.tracerAge[i] >= .12) continue;
      this.tracerAge[i] += this.tracerFresh[i] ? 0 : dt; this.tracerFresh[i] = 0;
      const fade = Math.max(0, 1 - this.tracerAge[i] / .12), j = i * 6, p = i * 3;
      const length = Math.hypot(this.tracerDelta[p], this.tracerDelta[p + 1], this.tracerDelta[p + 2]) || 1;
      const head = Math.min(length, 4 + length * this.tracerAge[i] / .12), tail = Math.max(0, head - 4);
      for (let k = 0; k < 3; k++) { this.tracerPositions[j + k] = this.tracerFrom[p + k] + this.tracerDelta[p + k] * tail / length; this.tracerPositions[j + 3 + k] = this.tracerFrom[p + k] + this.tracerDelta[p + k] * head / length; }
      this.tracerColors[j] = fade * .15; this.tracerColors[j + 1] = fade * .08; this.tracerColors[j + 2] = 0;
      this.tracerColors[j + 3] = fade * 2; this.tracerColors[j + 4] = fade * 1.5; this.tracerColors[j + 5] = fade * .65;
      if (fade > 0) tracers++;
    }
    if (tracers || this.tracers.visible) { this.tracers.geometry.attributes.position.needsUpdate = true; this.tracers.geometry.attributes.color.needsUpdate = true; }
    this.tracers.visible = tracers > 0; this.tracers.geometry.setDrawRange(0, this.tracerPool.used * 2);
    for (let i = 0; i < this.decalPool.size; i++) {
      if (this.decalAge[i] >= 6) continue;
      this.decalAge[i] += this.decalFresh[i] ? 0 : dt; this.decalFresh[i] = 0;
      this.decalAlpha[i] = .7 * Math.max(0, Math.min(1, 6 - this.decalAge[i]));
      if (this.decalAlpha[i] > 0) decals++;
    }
    if (decals || this.decals.visible) this.decals.geometry.attributes.decalAlpha.needsUpdate = true;
    this.decals.count = this.decalPool.used; this.decals.visible = decals > 0;
    for (const slot of this.lights) { slot.age += slot.fresh ? 0 : dt; slot.fresh = false; slot.light.intensity = slot.age < slot.life ? slot.peak * (1 - slot.age / slot.life) : 0; }
  }
  resize(height) { for (const layer of this.layers) layer.points.material.uniforms.scale.value = height / 2; }
  stats() { return { sparks: this.sparks.alive, puffs: this.puffs.alive, flashes: this.flashes.alive, fire: this.fire.alive, decals: this.decals.visible ? this.decals.count : 0, lights: this.lights.filter(s => s.light.visible && s.light.intensity > 0).length, emitted: this.emitted }; }
  reset() {
    for (const layer of this.layers) layer.reset(); this.tracerAge.fill(1); this.tracerPool.reset(); this.tracers.visible = false;
    this.decalAge.fill(6); this.decalAlpha.fill(0); this.decalPool.reset(); this.decals.visible = false; this.decals.count = 0; this.lightPool.reset();
    for (const slot of this.lights) { slot.age = 1; slot.light.intensity = 0; slot.fresh = false; }
  }
}

function smokeTexture() {
  const canvas = document.createElement('canvas'); canvas.width = canvas.height = 64;
  const context = canvas.getContext('2d');
  for (let i = 0; i < 9; i++) {
    const a = i * 2.4, x = 32 + Math.cos(a) * 10, y = 32 + Math.sin(a) * 10;
    const glow = context.createRadialGradient(x, y, 0, x, y, 21);
    glow.addColorStop(0, '#ffffff66'); glow.addColorStop(.45, '#ffffff44'); glow.addColorStop(1, '#ffffff00');
    context.fillStyle = glow; context.fillRect(0, 0, 64, 64);
  }
  return new THREE.CanvasTexture(canvas);
}
