import * as THREE from 'three';
import { GLTFLoader } from './vendor/addons/loaders/GLTFLoader.js';
import init, { WebGame } from './pkg/engine_web.js';

const $ = id => document.getElementById(id);
const modal = $('modal');
const keys = new Set();
const joystick = { x: 0, z: 0, pointer: null };
let action = false, running = false, ready = false, mode = 'start';
let game, source, scripts, latest, renderer, camera, world;
let previous = 0, accumulator = 0, joystickCenter;
const objects = new Map();
const loader = new GLTFLoader();
const loading = new Set();
const followTarget = new THREE.Vector3();
const cameraOffset = new THREE.Vector3();

function resetInput() {
  keys.clear(); joystick.x = 0; joystick.z = 0; joystick.pointer = null;
  action = false; $('stick').style.transform = ''; $('action').classList.remove('pressed');
}

function fail(error) {
  running = false; resetInput(); mode = 'error'; modal.hidden = false;
  $('modal-title').textContent = 'Das Spiel konnte nicht starten.';
  $('modal-text').textContent = String(error?.message || error);
  $('start').disabled = false; $('start').textContent = 'Seite neu laden';
  $('modal-note').textContent = 'Bitte einen aktuellen Browser mit WebGL 2 verwenden.';
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
}

function disposeObject(object) {
  world.remove(object);
  object.traverse(child => {
    child.geometry?.dispose();
    for (const mat of (Array.isArray(child.material) ? child.material : [child.material])) {
      if (!mat) continue;
      for (const value of Object.values(mat)) if (value?.isTexture) value.dispose();
      mat.dispose();
    }
  });
}

function apply(result) {
  latest = result;
  for (const id of result.removed) { const object = objects.get(id); if (object) disposeObject(object); objects.delete(id); }
  for (const entity of result.spawned) createObject(entity);
  for (const entity of result.transforms) {
    const object = objects.get(entity.id); if (!object) continue;
    const transform = entity.transform;
    object.position.fromArray(transform.position); object.scale.fromArray(transform.scale);
    object.rotation.set(...transform.rotation_deg.map(THREE.MathUtils.degToRad), 'ZYX');
  }
  const fragments = [];
  for (const [key, value] of Object.entries(result.state)) {
    if (key === 'won' || key === 'gewonnen') continue;
    const span = document.createElement('span');
    if (key === 'score') {
      const icon = document.createElement('i'); icon.className = 'coin-icon'; icon.setAttribute('aria-hidden', 'true'); span.append(icon);
      span.append(document.createTextNode('Münzen '));
      const number = document.createElement('b'); number.textContent = value; span.append(number);
      const total = source.entities.filter(e => e.tags?.includes('coin')).length;
      if (total) span.append(document.createTextNode(` / ${total}`));
    } else span.textContent = `${key}: ${typeof value === 'object' ? JSON.stringify(value) : value}`;
    fragments.push(span);
  }
  if (!fragments.length) { const span = document.createElement('span'); span.textContent = 'Spiel bereit'; fragments.push(span); }
  $('state').replaceChildren(...fragments);
  $('logs').textContent = result.logs.slice(-12).join('\n');
  if (running && (result.state.won === true || result.state.gewonnen === true)) showWin();
}

function showWin() {
  running = false; resetInput(); mode = 'restart'; modal.hidden = false;
  $('modal-badge').textContent = 'ALLE MÜNZEN GESAMMELT';
  $('modal-symbol').textContent = '✦'; $('modal-title').textContent = 'Gewonnen!';
  $('modal-text').textContent = `Geschafft! ${latest.state.score ?? ''} Münzen gesammelt und das Ziel erreicht. Noch eine Runde?`;
  $('start').textContent = 'Nochmal spielen'; $('start').disabled = false;
  $('modal-note').textContent = 'Dein Spiel läuft mit derselben Simulation wie im Editor.';
}

function controls() {
  let x = joystick.x + Number(keys.has('KeyD') || keys.has('ArrowRight')) - Number(keys.has('KeyA') || keys.has('ArrowLeft'));
  let z = joystick.z + Number(keys.has('KeyS') || keys.has('ArrowDown')) - Number(keys.has('KeyW') || keys.has('ArrowUp'));
  const length = Math.max(1, Math.hypot(x, z)); x /= length; z /= length;
  const pressed = Number(action || keys.has('Space'));
  return { move_x: x, move_z: z, jump: pressed, action: pressed };
}

function updateCamera(immediate = false) {
  const player = latest?.transforms.find(e => e.tags.includes('player'));
  if (!player) return;
  const target = new THREE.Vector3(...player.transform.position).add(new THREE.Vector3(0, 0.25, 0));
  followTarget.lerp(target, immediate ? 1 : 0.12);
  const offset = cameraOffset.clone().multiplyScalar(camera.aspect < 0.8 ? 1.2 : 0.85);
  camera.position.copy(followTarget).add(offset); camera.lookAt(followTarget);
}

function resize() {
  const width = innerWidth, height = innerHeight;
  renderer.setPixelRatio(Math.min(devicePixelRatio || 1, 2)); renderer.setSize(width, height, false);
  camera.aspect = width / height; camera.updateProjectionMatrix(); updateCamera(true);
}

async function restart() {
  running = false; ready = false; resetInput();
  for (const object of objects.values()) disposeObject(object); objects.clear();
  apply(JSON.parse(game.load(JSON.stringify(source), JSON.stringify(scripts))));
  await Promise.all([...loading]);
  camera.position.fromArray(source.camera.position); camera.lookAt(...source.camera.target); updateCamera(true);
  accumulator = 0; previous = performance.now(); ready = true;
}

function startRunning() {
  resetInput(); modal.hidden = true; mode = 'pause'; running = true; previous = performance.now(); accumulator = 0;
  $('pause').textContent = 'Ⅱ'; $('pause').setAttribute('aria-label', 'Spiel pausieren');
}

$('start').addEventListener('click', async () => {
  try {
    if (mode === 'error') { location.reload(); return; }
    if (!ready) return;
    if (mode === 'restart') { $('start').disabled = true; await restart(); }
    startRunning();
  } catch (error) { fail(error); }
});
$('pause').addEventListener('click', () => {
  if (!ready || mode === 'restart') return;
  if (running) {
    running = false; resetInput(); mode = 'pause'; modal.hidden = false;
    $('modal-badge').textContent = 'DURCHATMEN'; $('modal-title').textContent = 'Kurze Pause';
    $('modal-text').textContent = 'Dein Spiel wartet auf dich.'; $('start').textContent = 'Weiterspielen'; $('start').disabled = false;
  } else startRunning();
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
window.addEventListener('keydown', e => { if (moveKeys.includes(e.code)) { e.preventDefault(); if (running) keys.add(e.code); } });
window.addEventListener('keyup', e => keys.delete(e.code));
window.addEventListener('blur', resetInput);
document.addEventListener('visibilitychange', () => { resetInput(); previous = performance.now(); accumulator = 0; });
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
$('action').addEventListener('pointerdown', e => { if (running) { e.preventDefault(); $('action').setPointerCapture(e.pointerId); action = true; $('action').classList.add('pressed'); } });
for (const event of ['pointerup', 'pointercancel', 'lostpointercapture']) $('action').addEventListener(event, () => { action = false; $('action').classList.remove('pressed'); });
window.addEventListener('resize', () => { resetInput(); if (renderer && camera) resize(); });

function frame(now) {
  requestAnimationFrame(frame);
  if (!renderer || !ready) return;
  try {
    const elapsed = document.hidden ? 0 : Math.min((now - previous) / 1000, 0.1); previous = now;
    if (running) {
      accumulator += elapsed;
      while (accumulator >= 1 / 60 && running) { apply(JSON.parse(game.tick(JSON.stringify(controls())))); accumulator -= 1 / 60; }
    }
    updateCamera(); renderer.render(world, camera);
  } catch (error) { ready = false; fail(error); }
}

async function fetchJson(path) { const response = await fetch(path); if (!response.ok) throw new Error(`${path}: HTTP ${response.status}`); return response.json(); }
async function boot() {
  const context = $('game').getContext('webgl2', { antialias: true, alpha: false });
  if (!context) throw new Error('WebGL 2 ist nicht verfügbar. Bitte Safari oder Chrome aktualisieren.');
  renderer = new THREE.WebGLRenderer({ canvas: $('game'), context, antialias: true });
  renderer.shadowMap.enabled = true; renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  renderer.outputColorSpace = THREE.SRGBColorSpace; renderer.setClearColor(0xa0bdd5);
  world = new THREE.Scene(); world.background = new THREE.Color(0xa0bdd5);
  [source, scripts] = await Promise.all([fetchJson('./scene.json'), fetchJson('./scripts.json'), init()]);
  game = new WebGame();
  camera = new THREE.PerspectiveCamera(source.camera.fov_deg, innerWidth / innerHeight, 0.05, 2000);
  cameraOffset.fromArray(source.camera.position).sub(new THREE.Vector3(...source.camera.target));
  if (cameraOffset.length() < 0.1) cameraOffset.set(7, 6, 9);
  const ambient = new THREE.HemisphereLight(0xe2f3ff, 0x304438, 1.8); world.add(ambient);
  const sun = new THREE.DirectionalLight(0xfff4de, 2.5); sun.position.set(-3, 10, 6); sun.castShadow = true;
  sun.shadow.mapSize.set(1024, 1024); sun.shadow.camera.left = sun.shadow.camera.bottom = -15; sun.shadow.camera.right = sun.shadow.camera.top = 15;
  sun.shadow.normalBias = 0.03; world.add(sun);
  resize(); await restart(); $('pause').disabled = false;
  $('start').disabled = false; $('start').textContent = 'Spielen';
  const coinGame = source.entities.some(e => e.tags?.includes('coin'));
  $('title').textContent = coinGame ? 'Münzenpfad' : source.name || 'Web-Spiel';
  if (!coinGame) {
    $('modal-title').textContent = 'Deine Welt ist bereit.';
    $('modal-text').textContent = 'Steuere mit dem Joystick oder WASD / Pfeiltasten. Die Aktionstaste entspricht der Leertaste.';
    $('mission').textContent = 'Deine Welt. Direkt im Browser.';
  }
  if (matchMedia('(pointer: coarse)').matches) $('help').textContent = 'Joystick: Bewegen · Taste: Aktion';
  requestAnimationFrame(frame);
}
boot().catch(fail);
