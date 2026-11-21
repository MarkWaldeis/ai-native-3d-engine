import * as THREE from 'three';
import { qualitySettings } from './presentation.js';
import { HDRPipeline } from './hdr.js';
import { applySky, loadEnvironment } from './sky.js';
import { LocalLights } from './local-lights.js';
import { NatureVisuals } from './nature.js';
import { adaptQuality } from './nature-settings.js';
import { Atmosphere } from './atmosphere.js';

export class ShadowGrid {
  constructor(sunOffset, up) {
    const basis = new THREE.Matrix4().lookAt(sunOffset, new THREE.Vector3(), up);
    this.right = new THREE.Vector3().setFromMatrixColumn(basis, 0);
    this.up = new THREE.Vector3().setFromMatrixColumn(basis, 1);
  }

  align(target, camera, mapSize) {
    const x = target.dot(this.right), y = target.dot(this.up);
    const stepX = (camera.right - camera.left) / mapSize.x;
    const stepY = (camera.top - camera.bottom) / mapSize.y;
    return target
      .addScaledVector(this.right, Math.round(x / stepX) * stepX - x)
      .addScaledVector(this.up, Math.round(y / stepY) * stepY - y);
  }
}

export class GameEffects {
  constructor(renderer, scene, skyColor, graphics) {
    this.renderer = renderer; this.scene = scene; this.time = 0;
    renderer.shadowMap.autoUpdate = false; this.shadowFrame = 0;
    this.inverseRotation = new THREE.Quaternion();
    this.graphics = graphics;
    this.localLights = graphics ? new LocalLights(scene) : null;
    this.level = graphics?.quality >= 0 ? graphics.quality : matchMedia('(pointer: coarse)').matches ? 1 : 2;
    this.startLevel = this.level; this.fast = 0;
    this.sunOffset = new THREE.Vector3(...(graphics?.sunDirection || [-6, 16, 9]));
    const shadowScale = (graphics?.shadowExtent ?? 24) / 24, shadowHalf = 12 * shadowScale;
    if (graphics) this.sunOffset.setLength(20 * shadowScale);
    this.slow = 0; this.sampleTime = 0; this.frames = 0;
    const canvas = document.createElement('canvas'); canvas.width = 16; canvas.height = 256;
    const context = canvas.getContext('2d'), gradient = context.createLinearGradient(0, 0, 0, 256);
    gradient.addColorStop(0, graphics?.sky || '#3b7298'); gradient.addColorStop(0.5, graphics?.horizon || skyColor || '#aecbd5');
    gradient.addColorStop(1, graphics?.ground || '#f1d7b0'); context.fillStyle = gradient; context.fillRect(0, 0, 16, 256);
    scene.background = new THREE.CanvasTexture(canvas); scene.background.colorSpace = THREE.SRGBColorSpace;
    scene.fog = new THREE.Fog(graphics?.horizon || skyColor || '#b6ced8', 12, 55);
    renderer.toneMapping = graphics?.toneMapping === 'agx' ? THREE.AgXToneMapping : THREE.ACESFilmicToneMapping;
    renderer.toneMappingExposure = graphics?.exposure ?? 1.12;
    if (graphics) this.hdr = new HDRPipeline(renderer, graphics);
    scene.add(new THREE.HemisphereLight(graphics?.sky || 0xd9f4ff, graphics?.ground || 0x546445, graphics ? 1.25 : 2));
    this.sun = new THREE.DirectionalLight(graphics?.sunColor || 0xffe6b5, graphics?.sunIntensity ?? 3.2); this.sun.castShadow = this.sun.intensity > 0;
    Object.assign(this.sun.shadow.camera, {
      left: -shadowHalf, right: shadowHalf, top: shadowHalf, bottom: -shadowHalf,
      near: 0.5 * shadowScale, far: 55 * shadowScale,
    });
    this.sun.shadow.normalBias = 0.045; this.sun.shadow.bias = -0.00015;
    if (graphics) this.shadowGrid = new ShadowGrid(this.sunOffset, this.sun.shadow.camera.up);
    scene.add(this.sun, this.sun.target);
    if (!graphics) {
      const fill = new THREE.DirectionalLight(0x9fc8ff, 0.7); fill.position.set(4, 5, -8); scene.add(fill);
    }
    const glowCanvas = document.createElement('canvas'); glowCanvas.width = glowCanvas.height = 64;
    const glowContext = glowCanvas.getContext('2d'), glow = glowContext.createRadialGradient(32,32,0,32,32,32);
    glow.addColorStop(0,'#ffffff'); glow.addColorStop(0.2,'#ffffffbb'); glow.addColorStop(1,'#ffffff00');
    glowContext.fillStyle = glow; glowContext.fillRect(0,0,64,64); this.glowTexture = new THREE.CanvasTexture(glowCanvas); this.glowTexture.userData.shared = true;
    this.particles = [];
    this.positions = new Float32Array(96 * 3); this.colors = new Float32Array(96 * 3);
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.BufferAttribute(this.positions,3).setUsage(THREE.DynamicDrawUsage));
    geometry.setAttribute('color', new THREE.BufferAttribute(this.colors,3).setUsage(THREE.DynamicDrawUsage));
    this.points = new THREE.Points(geometry, new THREE.PointsMaterial({size:0.19,map:this.glowTexture,transparent:true,vertexColors:true,depthWrite:false,blending:THREE.AdditiveBlending}));
    this.points.frustumCulled = false; scene.add(this.points); geometry.setDrawRange(0,0);
    this.configure();
    if (graphics?.fogDensity !== undefined) scene.fog = new THREE.FogExp2(graphics.horizon, graphics.fogDensity);
    if (graphics?.foliage || graphics?.water) this.nature = new NatureVisuals(scene, graphics, this.level);
    if (graphics?.haze || graphics?.clouds) this.atmosphere = new Atmosphere(scene, graphics, this.level);
  }
  async prepareEnvironment() {
    if (!this.hdr?.supported) return;
    const url = this.graphics.environmentMap;
    let environment;
    if (url && (this.graphics.environmentIntensity || this.graphics.environmentBackground)) {
      try { environment = await loadEnvironment(url, this.graphics.environmentRotation); }
      catch (error) { console.warn('Environment map unavailable; using the procedural sky.', error); }
    }
    applySky(this.renderer, this.scene, this.graphics, this.sunOffset.clone().normalize(), environment);
  }
  configure() {
    const settings = qualitySettings(this.level, devicePixelRatio);
    const shadowSize = this.graphics ? Math.min(this.renderer.capabilities.maxTextureSize, this.graphics.shadowMapSizes[this.level]) : settings.shadowSize;
    if(this.graphics) {
      const extent=this.graphics.shadowExtents[this.level],scale=extent/24;
      Object.assign(this.sun.shadow.camera,{left:-extent/2,right:extent/2,top:extent/2,bottom:-extent/2,near:0.5*scale,far:55*scale});
      this.sun.shadow.camera.updateProjectionMatrix();this.sunOffset.setLength(20*scale);
    }
    if (this.hdr) this.hdr.resize(this.level, innerWidth, innerHeight, settings.pixelRatio);
    else { this.renderer.setPixelRatio(settings.pixelRatio); this.renderer.setSize(innerWidth,innerHeight,false); }
    if (this.sun.shadow.mapSize.x !== shadowSize) {
      this.sun.shadow.map?.dispose(); this.sun.shadow.map = null;
      this.sun.shadow.mapSize.setScalar(shadowSize);
    }
    this.renderer.domElement.dataset.quality = String(this.level);
    this.renderer.domElement.dataset.graphics = this.hdr?.sceneTarget ? 'hdr' : this.graphics ? 'standard' : 'legacy';
    this.renderer.shadowMap.needsUpdate = true;
    if (this.nature && this.nature.level !== this.level) this.nature.setQuality(this.level);
    this.atmosphere?.setQuality(this.level);
  }
  render(camera) {
    if (this.hdr) this.hdr.render(this.scene, camera, this.frameDelta);
    else this.renderer.render(this.scene, camera);
  }
  decorate(group) {
    if (!group.userData.tags.some(tag => ['portal','goal'].includes(tag))) return;
    const halo = new THREE.Sprite(new THREE.SpriteMaterial({map:this.glowTexture,color:0x72ffe0,transparent:true,opacity:0.16,depthWrite:false,blending:THREE.AdditiveBlending}));
    halo.scale.set(2.6,2.6,1); group.add(halo); group.userData.halo = halo;
  }
  burst(position, crystal = false) {
    for (let i=0;i<24;i++) {
      const angle = i * 2.39996, speed = 0.8 + (i % 5) * 0.28;
      this.particles.push({position:position.clone(),velocity:new THREE.Vector3(Math.cos(angle)*speed,1+(i%4)*0.5,Math.sin(angle)*speed),age:0,color:new THREE.Color(crystal ? 0x8cffe0 : 0xffd16b)});
    }
    this.particles = this.particles.slice(-96);
  }
  reset() {
    this.hdr?.resetExposure();
    this.localLights?.reset();
    this.nature?.clearTiles();
    this.particles = []; this.points.geometry.setDrawRange(0,0); this.sampleTime = this.frames = this.slow = this.fast = 0;
  }
  update(elapsed, objects, camera, active, frameElapsed = elapsed, shadowTarget) {
    this.frameDelta = Math.min(frameElapsed, 0.25);
    this.time += elapsed;
    if (this.level > 0 || ++this.shadowFrame % 2 === 0) this.renderer.shadowMap.needsUpdate = true;
    for (const [id, group] of objects) {
      const collectible = group.userData.tags.some(tag => tag === 'coin' || tag === 'crystal');
      if (collectible) {
        group.children[0].position.set(0,Math.sin(this.time*2.1+id)*0.09,0).applyQuaternion(this.inverseRotation.copy(group.quaternion).invert()).divide(group.scale);
        group.children[0].rotation.y = this.time*0.65;
      }
      if (group.userData.halo) group.userData.halo.material.opacity = 0.16 + Math.sin(this.time*2)*0.04;
    }
    this.sun.target.position.copy(camera.position).addScaledVector(camera.getWorldDirection(new THREE.Vector3()), 10);
    this.sun.target.position.y = 0;
    if (shadowTarget) this.sun.target.position.copy(shadowTarget);
    this.shadowGrid?.align(this.sun.target.position, this.sun.shadow.camera, this.sun.shadow.mapSize);
    this.sun.position.copy(this.sun.target.position).add(this.sunOffset);
    this.nature?.update(elapsed, camera);
    this.atmosphere?.update(elapsed);
    this.particles = this.particles.filter(p => p.age < 0.85);
    this.particles.forEach((p,i) => {
      p.age += elapsed; p.velocity.y -= elapsed*3; p.position.addScaledVector(p.velocity,elapsed);
      p.position.toArray(this.positions,i*3); p.color.clone().multiplyScalar(Math.max(0,1-p.age/0.85)).toArray(this.colors,i*3);
    });
    this.points.geometry.attributes.position.needsUpdate = true; this.points.geometry.attributes.color.needsUpdate = true;
    this.points.geometry.setDrawRange(0,this.particles.length);
    if (active && frameElapsed > 0) {
      this.sampleTime += frameElapsed; this.frames++;
      if (this.sampleTime >= 3) {
        const fps = this.frames/this.sampleTime;
        this.renderer.domElement.dataset.fps = fps.toFixed(1);
        if (this.graphics) {
          if (this.graphics.quality < 0 && adaptQuality(this, fps, this.sampleTime, matchMedia('(pointer: coarse)').matches ? 30 : 42)) this.configure();
        } else {
          this.slow = fps < 42 ? this.slow + 1 : 0;
          if (this.slow >= 2 && this.level > 0) { this.level--; this.configure(); this.slow = 0; }
        }
        this.sampleTime = this.frames = 0;
      }
    }
  }
}
