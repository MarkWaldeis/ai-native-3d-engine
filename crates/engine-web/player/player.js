import * as THREE from 'three';
import { GLTFLoader } from './vendor/addons/loaders/GLTFLoader.js';
import init, { WebGame } from './pkg/engine_web.js';
import { ActionLatch, viewMovement, followDistance } from './controls.js';
import { GameEffects } from './effects.js';
import { GameAudio, feedbackEvents, nextObjective, objectiveBearing } from './presentation.js';

const $ = id => document.getElementById(id);
const modal = $('modal');
const keys = new Set();
const joystick = { x: 0, z: 0, pointer: null };
const actionInput = new ActionLatch();
let running = false, ready = false, mode = 'start';
let game, source, scripts, latest, renderer, camera, world;
let ui = {};
let effects, lastFeedback, lastHUD = '';
const audio = new GameAudio();
let previous = 0, accumulator = 0, joystickCenter;
const objects = new Map();
const loader = new GLTFLoader();
const loading = new Set();
const followTarget = new THREE.Vector3();
const cameraOffset = new THREE.Vector3();
const lookDirection = new THREE.Vector3();
const shadows = new Map();
const shadowRay = new THREE.Raycaster();
let spawnHeight = 0;
const shadowCanvas = document.createElement('canvas');
shadowCanvas.width = shadowCanvas.height = 64;
const shadowContext = shadowCanvas.getContext('2d');
const gradient = shadowContext.createRadialGradient(32, 32, 0, 32, 32, 32);
gradient.addColorStop(0, '#ffffff99'); gradient.addColorStop(0.55, '#ffffff55'); gradient.addColorStop(1, '#ffffff00');
shadowContext.fillStyle = gradient; shadowContext.fillRect(0, 0, 64, 64);
const shadowTexture = new THREE.CanvasTexture(shadowCanvas);

function resetInput() {
  keys.clear(); joystick.x = 0; joystick.z = 0; joystick.pointer = null;
  actionInput.reset(); $('stick').style.transform = ''; $('action').classList.remove('pressed');
}

function fail(error) {
  running = false; ready = false; resetInput(); audio.suspend(); mode = 'error'; modal.hidden = false;
  modal.dataset.screen = 'error'; $('restart').hidden = true; $('pause').disabled = true;
  $('modal-badge').textContent = 'KURZE UNTERBRECHUNG';
  $('modal-title').textContent = 'Das Spiel konnte nicht starten.';
  $('modal-text').textContent = String(error?.message || error);
  $('start').disabled = false; $('start').textContent = 'Seite neu laden';
  $('modal-note').textContent = 'Bitte lade die Seite neu. Verwende einen aktuellen Browser mit WebGL 2.';
  console.error(error);
}

function material(color) {
  return new THREE.MeshStandardMaterial({ color: new THREE.Color(...(color || [0.8, 0.8, 0.8])), roughness: 0.55, metalness: 0.08 });
}

function mesh(geometry, color) {
  const result = new THREE.Mesh(geometry, material(color));
  result.castShadow = true; result.receiveShadow = true;
  return result;
}

function createObject(entity) {
  const group = new THREE.Group();
  let object;
  switch (entity.kind) {
    case 'box': object = mesh(new THREE.BoxGeometry(...entity.size), entity.color); break;
    case 'plane':
      object = mesh(new THREE.PlaneGeometry(entity.size, entity.size), entity.color);
      object.rotation.x = -Math.PI / 2; break;
    case 'sphere': object = mesh(new THREE.SphereGeometry(entity.radius, entity.segments, Math.max(8, Math.floor(entity.segments / 2))), entity.color); break;
    case 'cylinder': object = mesh(new THREE.CylinderGeometry(entity.radius, entity.radius, entity.height, entity.segments), entity.color); break;
    case 'wall_with_door': {
      object = new THREE.Group();
      const dw = Math.min(entity.door_width, entity.width), dh = Math.min(entity.door_height, entity.height);
      const side = (entity.width - dw) / 2;
      for (const sign of [-1, 1]) if (side > 0) {
        const piece = mesh(new THREE.BoxGeometry(side, entity.height, 0.15), entity.color);
        piece.position.set(sign * (dw + side) / 2, entity.height / 2, 0); object.add(piece);
      }
      if (entity.height > dh) {
        const lintel = mesh(new THREE.BoxGeometry(dw, entity.height - dh, 0.15), entity.color);
        lintel.position.y = (entity.height + dh) / 2; object.add(lintel);
      }
      break;
    }
    case 'gltf': {
      object = new THREE.Group();
      const task = loader.loadAsync(entity.path).then(gltf => {
        gltf.scene.traverse(child => { if (child.isMesh) { child.castShadow = true; child.receiveShadow = true; } });
        object.add(gltf.scene);
      });
      loading.add(task); task.finally(() => loading.delete(task)).catch(fail);
      break;
    }
    default: throw new Error(`Unbekannte Geometrie: ${entity.kind}`);
  }
  group.add(object); group.visible = entity.visible !== false;
  group.userData.tags = entity.tags || []; world.add(group); objects.set(entity.id, group);
  effects.decorate(group);
  group.userData.support = !!entity.collider && entity.rigidbody?.type !== 'dynamic' && !group.userData.tags.includes('trigger') && !group.userData.tags.includes('player');
  if (group.userData.tags.includes('player')) {
    const shadow = new THREE.Mesh(new THREE.PlaneGeometry(1.25, 1.25), new THREE.MeshBasicMaterial({
      color: 0x183a38, map: shadowTexture, transparent: true, depthWrite: false,
      polygonOffset: true, polygonOffsetFactor: -2,
    }));
    shadow.renderOrder = 2; shadow.visible = false; world.add(shadow); shadows.set(entity.id, shadow);
  }
}

function disposeObject(object) {
  world.remove(object);
  object.traverse(child => {
    child.geometry?.dispose();
    for (const mat of (Array.isArray(child.material) ? child.material : [child.material])) {
      if (!mat) continue;
      for (const value of Object.values(mat)) if (value?.isTexture && !value.userData.shared) value.dispose();
      mat.dispose();
    }
  });
}

function apply(result) {
  const feedback = feedbackEvents(lastFeedback, result.state);
  for (const id of result.removed) {
    const object = objects.get(id);
    if (object?.userData.tags.some(tag => tag === 'coin' || tag === 'crystal')) effects.burst(object.position, object.userData.tags.includes('crystal'));
  }
  latest = result;
  for (const id of result.removed) { const object = objects.get(id); if (object) disposeObject(object); objects.delete(id); }
  for (const entity of result.spawned) createObject(entity);
  for (const entity of result.transforms) {
    const object = objects.get(entity.id); if (!object) continue;
    const transform = entity.transform;
    object.position.fromArray(transform.position); object.scale.fromArray(transform.scale);
    object.rotation.set(...transform.rotation_deg.map(THREE.MathUtils.degToRad), 'ZYX');
  }
  if (feedback.collect) {
    audio.play('collect');
    $('state').classList.remove('collected'); void $('state').offsetWidth; $('state').classList.add('collected');
  }
  if (feedback.jump) audio.play('jump');
  if (feedback.win) audio.play('win');
  lastFeedback = { ...result.state };
  const hud = JSON.stringify(Object.entries(result.state).filter(([key]) => key !== 'won' && key !== 'gewonnen' && !key.startsWith('_') && (!ui.hud_keys || ui.hud_keys.includes(key))));
  if (hud !== lastHUD) {
    lastHUD = hud;
    const fragments = [];
    for (const [key, value] of Object.entries(result.state)) {
      if (key === 'won' || key === 'gewonnen' || key.startsWith('_') || (ui.hud_keys && !ui.hud_keys.includes(key))) continue;
      const span = document.createElement('span');
      if (key === 'score') {
        const icon = document.createElement('i'); icon.className = 'coin-icon'; icon.setAttribute('aria-hidden', 'true'); span.append(icon);
        span.append(document.createTextNode(`${ui.score_label || 'Münzen'} `));
        const number = document.createElement('b'); number.textContent = value; span.append(number);
        const total = source.entities.filter(e => e.tags?.includes(ui.collectible_tag || 'coin')).length;
        if (total) span.append(document.createTextNode(` / ${total}`));
      } else span.textContent = `${key}: ${typeof value === 'object' ? JSON.stringify(value) : value}`;
      fragments.push(span);
    }
    if (!fragments.length) { const span = document.createElement('span'); span.textContent = 'Spiel bereit'; fragments.push(span); }
    $('state').replaceChildren(...fragments);
  }
  $('logs').textContent = result.logs.slice(-12).join('\n');
  if (running && (result.state.won === true || result.state.gewonnen === true)) showWin();
}

function showWin() {
  running = false; resetInput(); mode = 'restart'; modal.hidden = false;
  $('restart').hidden = true;
  $('modal-badge').textContent = ui.win_badge || 'ALLE MÜNZEN GESAMMELT';
  $('modal-symbol').textContent = '✦'; $('modal-title').textContent = 'Gewonnen!';
  $('modal-text').textContent = ui.win_text || `Geschafft! ${latest.state.score ?? ''} Münzen gesammelt und das Ziel erreicht. Noch eine Runde?`;
  $('start').textContent = 'Nochmal spielen'; $('start').disabled = false;
  $('modal-note').textContent = 'Dein Spiel läuft mit derselben Simulation wie im Editor.';
  modal.dataset.screen = 'win';
}

function movement() {
  let x = joystick.x + Number(keys.has('KeyD') || keys.has('ArrowRight')) - Number(keys.has('KeyA') || keys.has('ArrowLeft'));
  let z = joystick.z + Number(keys.has('KeyS') || keys.has('ArrowDown')) - Number(keys.has('KeyW') || keys.has('ArrowUp'));
  const move = viewMovement(x, z, cameraOffset.toArray());
  return new THREE.Vector3(move.x, 0, move.z);
}

function controls() {
  const move = movement();
  const pressed = actionInput.sample();
  return { move_x: move.x, move_z: move.z, jump: pressed, action: pressed };
}

function updateCamera(immediate = false, elapsed = 1 / 60) {
  const player = latest?.transforms.find(e => e.tags.includes('player'));
  if (!player) return;
  const direction = movement();
  if (direction.length() > 0.15) lookDirection.lerp(direction.normalize(), immediate ? 1 : 1 - Math.exp(-elapsed * 6));
  const target = new THREE.Vector3(...player.transform.position);
  target.y = Math.max(target.y, spawnHeight - 1) + 0.25;
  target.addScaledVector(lookDirection, camera.aspect < 0.8 ? 1.5 : 1.1);
  const blend = immediate || target.distanceTo(followTarget) > 6 ? 1 : 1 - Math.exp(-elapsed * 9);
  const y = THREE.MathUtils.lerp(followTarget.y, target.y, immediate ? 1 : 1 - Math.exp(-elapsed * 3));
  followTarget.lerp(target, blend); followTarget.y = y;
  camera.fov = 55; camera.updateProjectionMatrix();
  const distance = followDistance(camera.aspect, camera.fov);
  const offset = new THREE.Vector3(cameraOffset.x, 0, cameraOffset.z).normalize().multiplyScalar(distance * 0.74);
  offset.y = distance * 0.67;
  camera.position.copy(followTarget).add(offset); camera.lookAt(followTarget);
}

function updateShadows() {
  world.updateMatrixWorld(true);
  const support = [...objects.values()].filter(object => object.userData.support);
  for (const [id, shadow] of shadows) {
    const player = objects.get(id);
    if (!player) { world.remove(shadow); shadow.geometry.dispose(); shadow.material.dispose(); shadows.delete(id); continue; }
    shadowRay.set(player.position.clone(), new THREE.Vector3(0, -1, 0)); shadowRay.far = 20;
    const hit = shadowRay.intersectObjects(support, true).find(hit => hit.face.normal.clone().transformDirection(hit.object.matrixWorld).y > 0.5);
    shadow.visible = !!hit;
    if (hit) {
      const normal = hit.face.normal.clone().transformDirection(hit.object.matrixWorld);
      shadow.position.copy(hit.point).addScaledVector(normal, 0.018);
      shadow.quaternion.setFromUnitVectors(new THREE.Vector3(0, 0, 1), normal);
      shadow.material.opacity = 0.85 / (1 + hit.distance * 0.3);
      shadow.scale.setScalar(1 + Math.min(hit.distance, 4) * 0.08);
    }
  }
}

function resize() {
  const width = innerWidth, height = innerHeight;
  effects.configure();
  camera.aspect = width / height; camera.updateProjectionMatrix(); updateCamera(true);
}

async function restart() {
  running = false; ready = false; resetInput();
  lastFeedback = null; lastHUD = ''; effects.reset(); $('state').classList.remove('collected');
  for (const object of objects.values()) disposeObject(object); objects.clear();
  for (const shadow of shadows.values()) { world.remove(shadow); shadow.geometry.dispose(); shadow.material.dispose(); } shadows.clear();
  apply(JSON.parse(game.load(JSON.stringify(source), JSON.stringify(scripts))));
  await Promise.all([...loading]);
  if (mode === 'error') throw new Error('Die Grafik ist nicht verfügbar. Bitte lade die Seite neu.');
  const player = latest.transforms.find(e => e.tags.includes('player'));
  if (player) {
    spawnHeight = player.transform.position[1];
    lookDirection.fromArray(source.camera.target).sub(new THREE.Vector3(...player.transform.position)); lookDirection.y = 0;
    if (lookDirection.length() > 0.1) lookDirection.normalize();
  }
  camera.position.fromArray(source.camera.position); camera.lookAt(...source.camera.target); updateCamera(true);
  accumulator = 0; previous = performance.now(); ready = true;
}

function startRunning() {
  audio.unlock();
  $('restart').hidden = true; $('start').disabled = false;
  resetInput(); modal.hidden = true; mode = 'pause'; running = true; previous = performance.now(); accumulator = 0;
  $('pause').textContent = 'Ⅱ'; $('pause').setAttribute('aria-label', 'Spiel pausieren');
}

$('start').addEventListener('click', async () => {
  try {
    audio.unlock();
    if (mode === 'error') { location.reload(); return; }
    if (!ready) return;
    if (mode === 'restart') { $('start').disabled = true; await restart(); }
    startRunning();
  } catch (error) { fail(error); }
});
function pause() {
  running = false; resetInput(); accumulator = 0; audio.suspend(); mode = 'pause'; modal.hidden = false;
  modal.dataset.screen = 'pause';
  $('modal-badge').textContent = 'DURCHATMEN'; $('modal-title').textContent = 'Kurze Pause';
  $('modal-symbol').textContent = 'Ⅱ';
  $('modal-text').textContent = 'Dein Abenteuer wartet auf dich. Spiele in deinem Tempo weiter.';
  $('start').textContent = 'Weiterspielen'; $('start').disabled = false;
  $('modal-note').textContent = 'Kein Zeitdruck. Dein Fortschritt bleibt erhalten.';
  $('restart').hidden = false;
}
$('restart').addEventListener('click', async () => {
  if (!ready || mode === 'error') return;
  audio.unlock(); $('restart').disabled = true; $('start').disabled = true;
  try { await restart(); startRunning(); } catch (error) { fail(error); }
  finally { $('restart').disabled = false; }
});
$('pause').addEventListener('click', () => {
  if (!ready || mode === 'restart') return;
  if (running) {
    pause();
  } else startRunning();
});
$('mute').addEventListener('click', () => {
  audio.muted = !audio.muted;
  if (!document.hidden) audio.unlock();
  $('mute').textContent = audio.muted ? 'Ton aus' : 'Ton an';
  $('mute').setAttribute('aria-pressed', String(audio.muted));
});
$('logs-toggle').addEventListener('click', () => { $('logs').hidden = !$('logs').hidden; });
$('fullscreen').addEventListener('click', async () => {
  try {
    if (document.fullscreenElement) await document.exitFullscreen();
    else if (document.documentElement.requestFullscreen) await document.documentElement.requestFullscreen();
    else { $('mission').textContent = 'Tipp: Über „Teilen“ zum Home-Bildschirm hinzufügen.'; }
  } catch { $('mission').textContent = 'Vollbild ist in diesem Browser nicht verfügbar.'; }
});

const moveKeys = ['KeyW', 'KeyA', 'KeyS', 'KeyD', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Space'];
window.addEventListener('keydown', e => { if (moveKeys.includes(e.code)) { e.preventDefault(); if (running) { if (e.code === 'Space') actionInput.press('keyboard'); keys.add(e.code); } } });
window.addEventListener('keyup', e => { keys.delete(e.code); if (e.code === 'Space') actionInput.release('keyboard'); });
window.addEventListener('blur', resetInput);
document.addEventListener('visibilitychange', () => {
  if (document.hidden && running) pause();
  else if (document.hidden) audio.suspend();
  resetInput(); previous = performance.now(); accumulator = 0;
});
document.addEventListener('gesturestart', e => e.preventDefault(), { passive: false });
document.addEventListener('contextmenu', e => e.preventDefault());
const pad = $('joystick');
function moveJoystick(e) {
  if (joystick.pointer !== e.pointerId) return;
  e.preventDefault(); const radius = pad.clientWidth * 0.33;
  const dx = e.clientX - joystickCenter.x, dz = e.clientY - joystickCenter.y;
  const length = Math.max(radius, Math.hypot(dx, dz));
  joystick.x = dx / length; joystick.z = dz / length;
  $('stick').style.transform = `translate(${joystick.x * radius}px,${joystick.z * radius}px)`;
}
pad.addEventListener('pointerdown', e => {
  if (!running || joystick.pointer !== null) return;
  joystick.pointer = e.pointerId; pad.setPointerCapture(e.pointerId);
  const rect = pad.getBoundingClientRect(); joystickCenter = { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }; moveJoystick(e);
});
pad.addEventListener('pointermove', moveJoystick);
for (const event of ['pointerup', 'pointercancel', 'lostpointercapture']) pad.addEventListener(event, e => {
  if (e.pointerId === joystick.pointer) { joystick.pointer = null; joystick.x = joystick.z = 0; $('stick').style.transform = ''; }
});
$('action').addEventListener('pointerdown', e => { if (running) { e.preventDefault(); $('action').setPointerCapture(e.pointerId); actionInput.press(e.pointerId); $('action').classList.add('pressed'); } });
for (const event of ['pointerup', 'pointercancel', 'lostpointercapture']) $('action').addEventListener(event, e => {
  actionInput.release(e.pointerId, event === 'pointercancel');
  $('action').classList.toggle('pressed', actionInput.held.size > 0);
});
window.addEventListener('resize', () => { resetInput(); if (renderer && camera) resize(); });

function frame(now) {
  requestAnimationFrame(frame);
  if (!renderer || !ready) return;
  try {
    const rawElapsed = (now - previous) / 1000;
    const elapsed = Math.min(rawElapsed, 0.1); previous = now;
    if (document.hidden) return;
    if (running) {
      accumulator += elapsed;
      while (accumulator >= 1 / 60 && running) { apply(JSON.parse(game.tick(JSON.stringify(controls())))); accumulator -= 1 / 60; }
    }
    updateCamera(false, elapsed); effects.update(elapsed, objects, camera, running, rawElapsed);
    updateShadows(); updateObjective(); renderer.render(world, camera);
  } catch (error) { ready = false; fail(error); }
}

function updateObjective() {
  const entries = [...objects].filter(([, object]) => object.visible).map(([id, object]) => ({id,tags:object.userData.tags,position:object.position.toArray()}));
  const player = entries.find(entity => entity.tags.includes('player'));
  const target = player && nextObjective(entries, player.position);
  $('objective').hidden = !target;
  if (!target) return;
  const position = new THREE.Vector3(...target.position), projected = position.clone().project(camera);
  const behind = position.clone().sub(camera.position).dot(camera.getWorldDirection(new THREE.Vector3())) < 0;
  const origin = new THREE.Vector3(...player.position).project(camera), sign = behind ? -1 : 1;
  const dx = (projected.x * sign - origin.x) * innerWidth, dy = (projected.y * sign - origin.y) * innerHeight;
  $('objective-arrow').style.transform = `rotate(${objectiveBearing(dx, dy)}rad)`;
  const label = target.tags.includes('crystal') ? 'Nächster Kristall' : target.tags.includes('coin') ? 'Nächste Münze' : target.tags.includes('portal') ? 'Zum Portal' : 'Zum Ziel';
  const text = `${label} · ${Math.ceil(position.distanceTo(new THREE.Vector3(...player.position)))} m`;
  if ($('objective-text').textContent !== text) $('objective-text').textContent = text;
}

async function fetchJson(path) { const response = await fetch(path); if (!response.ok) throw new Error(`${path}: HTTP ${response.status}`); return response.json(); }
async function boot() {
  $('game').addEventListener('webglcontextlost', event => {
    event.preventDefault(); fail(new Error('Die Grafikverbindung wurde unterbrochen. Bitte lade die Seite neu.'));
  });
  const context = $('game').getContext('webgl2', { antialias: true, alpha: false });
  if (!context) throw new Error('WebGL 2 ist nicht verfügbar. Bitte Safari oder Chrome aktualisieren.');
  renderer = new THREE.WebGLRenderer({ canvas: $('game'), context, antialias: true });
  renderer.debug.onShaderError = () => { throw new Error('Die Grafik konnte nicht berechnet werden. Bitte lade die Seite neu.'); };
  renderer.shadowMap.enabled = true; renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  renderer.outputColorSpace = THREE.SRGBColorSpace; renderer.setClearColor(0xa0bdd5);
  world = new THREE.Scene();
  [source, scripts] = await Promise.all([fetchJson('./scene.json'), fetchJson('./scripts.json'), init()]);
  if (mode === 'error') throw new Error('Die Grafik ist nicht verfügbar. Bitte lade die Seite neu.');
  const authoredUI = source.entities.find(e => e.tags?.includes('player'))?.props?.ai_ui;
  ui = typeof authoredUI === 'string' ? JSON.parse(authoredUI) : authoredUI || {};
  effects = new GameEffects(renderer, world, ui.sky_color);
  game = new WebGame();
  camera = new THREE.PerspectiveCamera(source.camera.fov_deg, innerWidth / innerHeight, 0.05, 2000);
  cameraOffset.fromArray(source.camera.position).sub(new THREE.Vector3(...source.camera.target));
  if (cameraOffset.length() < 0.1) cameraOffset.set(7, 6, 9);
  resize(); await restart(); $('pause').disabled = false;
  $('start').disabled = false; $('start').textContent = 'Spielen';
  const coinGame = source.entities.some(e => e.tags?.includes('coin'));
  $('title').textContent = coinGame ? 'Münzenpfad' : source.name || 'Web-Spiel';
  if (!coinGame) {
    $('modal-title').textContent = 'Deine Welt ist bereit.';
    $('modal-text').textContent = 'Steuere mit dem Joystick oder WASD / Pfeiltasten. Die Aktionstaste entspricht der Leertaste.';
    $('mission').textContent = 'Deine Welt. Direkt im Browser.';
  }
  if (ui.title) $('title').textContent = ui.title;
  if (ui.intro_title) $('modal-title').textContent = ui.intro_title;
  if (ui.intro_text) $('modal-text').textContent = ui.intro_text;
  if (ui.mission) $('mission').textContent = ui.mission;
  if (ui.badge) $('modal-badge').textContent = ui.badge;
  if (ui.action_label) $('action').lastChild.textContent = ui.action_label;
  document.title = `${$('title').textContent} · AI Engine`;
  if (matchMedia('(pointer: coarse)').matches) $('help').textContent = 'Joystick: Bewegen · Taste: Aktion';
  requestAnimationFrame(frame);
}
boot().catch(fail);
