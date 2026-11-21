import * as THREE from 'three';
import { applyLook, combatCues, fpsInputs, reloadDip, weaponBob } from './fps_core.js';
import { ShooterFx, flashTexture } from './fx.js';
import { ShooterHud } from './hud.js';

export { fpsConfig } from './fps_core.js';

function capture(element, pointerId) { try { element.setPointerCapture(pointerId); } catch { /* synthetic or ended pointer */ } }

// First-person presentation layer: pointer lock/touch look, camera, viewmodel, FX and HUD.
// Movement, weapons, damage and AI stay in Rust/Rhai; this module only sends the contract inputs.
export class FirstPerson {
  constructor({ renderer, world, camera, config, ui, canvas, onUnlock, assets }) {
    Object.assign(this, { renderer, world, camera, config, ui, canvas, onUnlock, assets });
    this.coarse = matchMedia('(pointer: coarse)').matches;
    this.look = { yaw: config.yaw, pitch: 0 }; this.buttons = { fire: false, reload: false, sprint: false };
    this.firePointers = new Set(); this.lookPointer = null; this.lastLook = null;
    this.fx = new ShooterFx(world, this.coarse); this.hud = new ShooterHud(ui);
    this.eye = new THREE.Vector3(); this.lastEye = null; this.speed = 0; this.phase = 0; this.recoil = 0; this.kick = 0;
    this.reloadTotal = 0; this.flash = 0; this.fov = camera.fov = 75; camera.near = 0.05; camera.updateProjectionMatrix();
    document.body.classList.add('fps');
    this.buildViewmodel(); this.bind();
  }
  buildViewmodel() {
    this.vmScene = new THREE.Scene();
    this.vmCamera = new THREE.PerspectiveCamera(58, this.camera.aspect, 0.01, 10);
    this.vmScene.add(new THREE.HemisphereLight(0xe8f4ff, 0x3a3428, 2.2));
    const key = new THREE.DirectionalLight(0xfff0d8, 2.4); key.position.set(1, 2, 1); this.vmScene.add(key);
    this.weapon = new THREE.Group(); this.vmScene.add(this.weapon);
    this.rest = new THREE.Vector3(0.16, -0.15, -0.32);
    const flashMaterial = new THREE.SpriteMaterial({ map: flashTexture(), blending: THREE.AdditiveBlending, depthWrite: false, depthTest: false, transparent: true, toneMapped: false, fog: false });
    this.muzzleFlash = new THREE.Sprite(flashMaterial); this.muzzleFlash.scale.setScalar(0.07); this.muzzleFlash.renderOrder = 10; this.muzzleFlash.visible = false;
    this.muzzleLight = new THREE.PointLight(0xffb060, 0, 2); this.vmScene.add(this.muzzleLight);
    if (this.config.viewmodel) {
      this.assets.load(this.config.viewmodel).then(gltf => { const model = gltf.scene.clone(true); this.mountModel(model, true); }).catch(() => this.mountModel(this.fallbackRifle()));
    } else this.mountModel(this.fallbackRifle());
  }
  fallbackRifle() {
    const group = new THREE.Group(), dark = new THREE.MeshStandardMaterial({ color: 0x4a5258, roughness: 0.5, metalness: 0.25 });
    const wood = new THREE.MeshStandardMaterial({ color: 0x7a5a3e, roughness: 0.75 });
    const part = (w, h, d, x, y, z, m = dark) => { const mesh = new THREE.Mesh(new THREE.BoxGeometry(w, h, d), m); mesh.position.set(x, y, z); group.add(mesh); return mesh; };
    part(0.07, 0.09, 0.42, 0, 0, 0); part(0.03, 0.03, 0.34, 0, 0.02, -0.36); part(0.05, 0.16, 0.06, 0, -0.1, -0.06).rotation.x = 0.25;
    part(0.06, 0.12, 0.2, 0, -0.03, 0.26, wood); part(0.045, 0.11, 0.05, 0, -0.1, 0.08, wood).rotation.x = -0.3;
    part(0.03, 0.04, 0.1, 0, 0.07, -0.02); part(0.06, 0.05, 0.18, 0, -0.005, -0.2, wood);
    group.scale.setScalar(0.55); group.userData.muzzle = new THREE.Vector3(0, 0.02, -0.54);
    return group;
  }
  mountModel(model, authored = false) {
    if (this.model) this.weapon.remove(this.model);
    this.model = model; model.removeFromParent();
    if (authored) {
      const box = new THREE.Box3().setFromObject(model), size = box.getSize(new THREE.Vector3());
      const scale = 0.4 / Math.max(size.x, size.y, size.z, 1e-3); model.scale.multiplyScalar(scale);
      const center = new THREE.Box3().setFromObject(model).getCenter(new THREE.Vector3()); model.position.sub(center);
      model.userData.muzzle = new THREE.Vector3(0, 0.02, -0.2 / model.scale.x);
    }
    this.weapon.add(model);
    model.traverse(o => { if (o.isMesh) { o.castShadow = false; o.frustumCulled = false; } });
    model.updateMatrix(); this.muzzleFlash.position.copy(model.userData.muzzle).applyMatrix4(model.matrix); this.weapon.add(this.muzzleFlash);
  }
  bind() {
    const listen = (target, type, fn, options) => target.addEventListener(type, fn, options);
    listen(document, 'pointerlockchange', () => { if (!this.locked() && this.active) this.onUnlock?.(); });
    listen(this.canvas, 'mousedown', e => {
      if (!this.active || e.button !== 0) return;
      if (!this.locked() && !this.coarse) this.lock();
      this.buttons.fire = true;
    });
    listen(window, 'mouseup', e => { if (e.button === 0) this.buttons.fire = false; });
    listen(document, 'mousemove', e => { if (this.active && this.locked()) this.look = applyLook(this.look, e.movementX, e.movementY, this.config.mouse_sens); });
    this.touchLook = document.createElement('div'); this.touchLook.className = 'fps-look-zone'; this.canvas.after(this.touchLook);
    listen(this.touchLook, 'pointerdown', e => { if (!this.active) return; e.preventDefault(); this.lookPointer = e.pointerId; this.lastLook = [e.clientX, e.clientY]; capture(this.touchLook, e.pointerId); });
    listen(this.touchLook, 'pointermove', e => {
      if (e.pointerId !== this.lookPointer || !this.lastLook) return;
      this.look = applyLook(this.look, e.clientX - this.lastLook[0], e.clientY - this.lastLook[1], this.config.mouse_sens * 2.2); this.lastLook = [e.clientX, e.clientY];
    });
    for (const type of ['pointerup', 'pointercancel', 'lostpointercapture']) listen(this.touchLook, type, e => { if (e.pointerId === this.lookPointer) { this.lookPointer = null; this.lastLook = null; } });
    this.fireButton = document.createElement('button'); this.fireButton.className = 'fps-fire'; this.fireButton.textContent = 'FEUER'; this.fireButton.setAttribute('aria-label', 'Feuern');
    this.reloadButton = document.createElement('button'); this.reloadButton.className = 'fps-reload-button'; this.reloadButton.textContent = 'R'; this.reloadButton.setAttribute('aria-label', 'Nachladen');
    (document.getElementById('controls') || document.body).append(this.fireButton, this.reloadButton);
    listen(this.fireButton, 'pointerdown', e => {
      if (!this.active) return; e.preventDefault(); capture(this.fireButton, e.pointerId); this.firePointers.add(e.pointerId);
      this.lookPointer ??= e.pointerId; this.lastLook = this.lookPointer === e.pointerId ? [e.clientX, e.clientY] : this.lastLook;
    });
    listen(this.fireButton, 'pointermove', e => {
      if (e.pointerId !== this.lookPointer || !this.lastLook) return;
      this.look = applyLook(this.look, e.clientX - this.lastLook[0], e.clientY - this.lastLook[1], this.config.mouse_sens * 2.2); this.lastLook = [e.clientX, e.clientY];
    });
    for (const type of ['pointerup', 'pointercancel', 'lostpointercapture']) listen(this.fireButton, type, e => { this.firePointers.delete(e.pointerId); if (e.pointerId === this.lookPointer) { this.lookPointer = null; this.lastLook = null; } });
    listen(this.reloadButton, 'pointerdown', e => { if (this.active) { e.preventDefault(); this.buttons.reload = true; } });
    for (const type of ['pointerup', 'pointercancel']) listen(this.reloadButton, type, () => { this.buttons.reload = false; });
  }
  locked() { return document.pointerLockElement === this.canvas; }
  lock() { if (!this.coarse && this.canvas.requestPointerLock && !this.locked()) { try { const request = this.canvas.requestPointerLock(); request?.catch?.(() => {}); } catch { /* unsupported */ } } }
  start() { this.active = true; this.hud.visible(true); document.body.classList.add('fps-active'); this.lock(); }
  stop() { this.active = false; document.body.classList.remove('fps-active'); this.resetInput(); if (this.locked()) document.exitPointerLock?.(); }
  resetInput() { this.buttons = { fire: false, reload: false, sprint: false }; this.firePointers.clear(); this.lookPointer = null; this.lastLook = null; }
  /** Contract inputs for one simulation tick; `move` is local (x strafe, z: -1 forward). */
  gamepadLook(x, y, dt) {
    if (this.active) this.look = applyLook(this.look, x, y, dt);
  }
  inputs(move, jump, actions = {}) {
    return fpsInputs(move, this.look, { fire: actions.fire || this.buttons.fire || this.firePointers.size > 0,
      reload: actions.reload || this.buttons.reload, sprint: actions.sprint || this.buttons.sprint, jump });
  }
  apply(result, playerObject) {
    if (playerObject) playerObject.visible = false;
    const cues = combatCues(result, this.lastEye && this.lastEye.toArray());
    const muzzle = this.worldMuzzle();
    for (const fx of result.fx || []) {
      const own = this.lastEye && Math.hypot(fx.pos[0] - this.lastEye.x, fx.pos[1] - this.lastEye.y, fx.pos[2] - this.lastEye.z) < 1.5;
      if (own && fx.kind === 'muzzle') continue;
      this.fx.emit(fx, own && fx.kind === 'tracer' ? muzzle : null);
    }
    if (cues.shots) { this.recoil = Math.min(1, this.recoil + 0.5); this.kick += 0.012; this.flash = 0.05; }
    this.hud.cues(cues); this.hud.state(result.state);
    const reload = Number(result.state._reload) || 0;
    if (reload > 0 && this.reloadTotal === 0) this.reloadTotal = reload;
    if (reload <= 0) this.reloadTotal = 0;
    this.reloadRemaining = reload; this.dead = Number(result.state.health) <= 0;
  }
  worldMuzzle() {
    if (!this.model) return null;
    const point = this.model.userData.muzzle.clone(); this.model.localToWorld(point);
    point.applyMatrix4(this.vmCamera.matrixWorldInverse);
    return point.applyMatrix4(this.camera.matrixWorld).toArray();
  }
  updateCamera(player, elapsed, sprinting) {
    if (!player) return;
    this.eye.set(player.transform.position[0], player.transform.position[1] + this.config.eye_height, player.transform.position[2]);
    if (this.lastEye && elapsed > 0) {
      const horizontal = Math.hypot(this.eye.x - this.lastEye.x, this.eye.z - this.lastEye.z) / elapsed;
      this.speed += (Math.min(1, horizontal / this.config.sprint_speed) - this.speed) * Math.min(1, elapsed * 10);
    }
    this.lastEye = (this.lastEye || new THREE.Vector3()).copy(this.eye);
    this.phase += elapsed * (6 + this.speed * 8);
    this.kick = Math.max(0, this.kick - elapsed * 0.15);
    this.camera.position.copy(this.eye);
    if (this.dead) this.camera.position.y -= this.config.eye_height * 0.7;
    this.camera.rotation.set(this.look.pitch + this.kick, this.look.yaw, this.dead ? 0.4 : 0, 'YXZ');
    const fov = 75 + (sprinting && this.speed > 0.5 ? 7 : 0);
    if (Math.abs(this.camera.fov - fov) > 0.05) { this.camera.fov += (fov - this.camera.fov) * Math.min(1, elapsed * 8); this.camera.updateProjectionMatrix(); }
    this.camera.updateMatrixWorld();
  }
  frame(elapsed, markers) {
    this.fx.update(elapsed); this.hud.update(elapsed, this.look, this.speed);
    this.hud.drawMinimap(this.lastEye?.toArray(), this.look.yaw, markers);
    this.recoil = Math.max(0, this.recoil - elapsed * 6); this.flash = Math.max(0, this.flash - elapsed);
    const bob = weaponBob(this.phase, this.speed), dip = reloadDip(this.reloadRemaining, this.reloadTotal);
    this.weapon.position.set(this.rest.x + bob.x, this.rest.y + bob.y - dip * 0.07 - (this.dead ? 0.5 : 0), this.rest.z + this.recoil * 0.05);
    this.weapon.rotation.set(this.recoil * 0.12 - dip * 0.35, bob.x * 2, dip * 0.45);
    this.muzzleFlash.visible = this.flash > 0; this.muzzleFlash.material.rotation = Math.random() * Math.PI;
    if (this.muzzleFlash.visible) this.muzzleFlash.scale.setScalar(0.055 + Math.random() * 0.03);
    this.muzzleLight.intensity = this.flash > 0 ? 3 : 0;
    if (this.model) { const tip = this.model.userData.muzzle.clone(); this.model.localToWorld(tip); this.muzzleLight.position.copy(tip); }
  }
  /** Draws the viewmodel on top of the world without touching world depth. */
  renderOverlay() {
    if (this.dead) return;
    this.vmCamera.aspect = this.camera.aspect; this.vmCamera.updateProjectionMatrix(); this.vmCamera.updateMatrixWorld();
    const autoClear = this.renderer.autoClear; this.renderer.autoClear = false; this.renderer.clearDepth();
    this.renderer.render(this.vmScene, this.vmCamera); this.renderer.autoClear = autoClear;
  }
  reset() { this.fx.reset(); this.hud.reset(); this.look = { yaw: this.config.yaw, pitch: 0 }; this.lastEye = null; this.recoil = 0; this.kick = 0; this.reloadTotal = 0; this.dead = false; this.resetInput(); }
}
