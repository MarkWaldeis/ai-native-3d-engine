// Pure first-person helpers (no DOM/three.js) shared by fps.js and the node tests.
export const FX_KINDS = ['muzzle', 'tracer', 'impact', 'blood', 'explosion'];
export const PITCH_LIMIT = 1.45;
const DEFAULTS = { eye_height: 0.75, move_speed: 5, sprint_speed: 8.5, jump_speed: 5.5, mouse_sens: 0.0022 };

export function travelSpeedInput(search) {
  const value = Number(new URLSearchParams(search).get('speed'));
  return Number.isFinite(value) && value > 0 ? value : 1;
}

export function defeated(state) {
  return typeof state.lost === 'boolean' ? state.lost : Number(state.health) <= 0;
}

function parse(value) {
  if (typeof value !== 'string') return value;
  try { return JSON.parse(value); } catch { return null; }
}

/** Returns the normalized `ai_fps` config of the player entity, or null for non-FPS scenes. */
export function fpsConfig(entities) {
  const player = entities.find(e => e.tags?.includes('player'));
  const raw = parse(player?.props?.ai_fps);
  if (!raw || typeof raw !== 'object') return null;
  const config = { ...DEFAULTS };
  for (const key of Object.keys(DEFAULTS)) if (Number.isFinite(Number(raw[key]))) config[key] = Number(raw[key]);
  const viewmodel = player.props.ai_viewmodel;
  config.viewmodel = typeof viewmodel === 'string' && viewmodel ? viewmodel : null;
  config.yaw = THREE_DEG * (player.transform?.rotation_deg?.[1] || 0);
  return config;
}
const THREE_DEG = Math.PI / 180;

/** Applies a pointer delta (pixels) to absolute yaw/pitch in radians; pitch is clamped. */
export function applyLook(look, dx, dy, sens) {
  const yaw = wrapAngle(look.yaw - dx * sens);
  const pitch = Math.max(-PITCH_LIMIT, Math.min(PITCH_LIMIT, look.pitch - dy * sens));
  return { yaw, pitch };
}

export function wrapAngle(angle) {
  const turn = Math.PI * 2;
  return ((angle + Math.PI) % turn + turn) % turn - Math.PI;
}

/** Exact per-tick input map of the shooter contract (move is local: x strafe, z back/forward). */
export function fpsInputs(move, look, buttons) {
  let { x, z } = move;
  const length = Math.hypot(x, z);
  if (length > 1) { x /= length; z /= length; }
  return {
    move_x: x, move_z: z, look_yaw: look.yaw, look_pitch: look.pitch,
    fire: buttons.fire ? 1 : 0, reload: buttons.reload ? 1 : 0,
    sprint: buttons.sprint ? 1 : 0, jump: buttons.jump ? 1 : 0,
  };
}

/** View direction for yaw/pitch, matching Rhai `ctx.aim_dir()`. */
export function lookDir(yaw, pitch) {
  return [-Math.sin(yaw) * Math.cos(pitch), Math.sin(pitch), -Math.cos(yaw) * Math.cos(pitch)];
}

/** Compass heading in degrees (0 = north = -Z, 90 = east = +X). */
export function heading(yaw) {
  return ((-yaw * 180 / Math.PI) % 360 + 360) % 360;
}

/** Projects a world XZ point onto a player-centred, heading-up minimap; null when outside. */
export function minimapPoint(player, yaw, target, range, size) {
  const dx = target[0] - player[0], dz = target[2] - player[2];
  const cos = Math.cos(yaw), sin = Math.sin(yaw);
  const right = dx * cos - dz * sin, forward = -dx * sin - dz * cos;
  if (Math.hypot(right, forward) > range) return null;
  const scale = size / 2 / range;
  return { x: size / 2 + right * scale, y: size / 2 - forward * scale };
}

/** Ring-buffer slot allocator: reuses the oldest slot once the pool is full. */
export class SlotPool {
  constructor(size) { this.size = size; this.next = 0; this.used = 0; }
  take() { const slot = this.next; this.next = (this.next + 1) % this.size; this.used = Math.min(this.size, this.used + 1); return slot; }
  reset() { this.next = 0; this.used = 0; }
}

/** Splits snapshot fx/events into presentation cues for the local player. */
export function combatCues(result, eye) {
  const cues = { shots: 0, hits: 0, kills: [], damageTaken: 0, killedTarget: false };
  for (const fx of result.fx || []) {
    if (fx.kind === 'muzzle' && eye && Math.hypot(fx.pos[0] - eye[0], fx.pos[1] - eye[1], fx.pos[2] - eye[2]) < 1.5) cues.shots++;
  }
  for (const event of result.events || []) {
    if (event.type === 'damage' && event.by_player && !event.to_player) { cues.hits++; if (event.killed) cues.killedTarget = true; }
    if (event.type === 'damage' && event.to_player) cues.damageTaken += Number(event.amount) || 0;
    if (event.type === 'kill') cues.kills.push(killText(event));
  }
  return cues;
}

export function killText(event) {
  if (event.text) return String(event.text);
  const source = event.by_player ? 'Du' : event.source_name || 'Unbekannt';
  return `${source} ✕ ${event.target_name || 'Ziel'}`;
}

/** Weapon sway/bob offset for a walk phase (radians) and normalized speed 0..1. */
export function weaponBob(phase, speed) {
  return { x: Math.sin(phase) * 0.012 * speed, y: -Math.abs(Math.cos(phase)) * 0.016 * speed };
}

/** Reload dip: 0 at start/end, 1 in the middle of the reload. */
export function reloadDip(remaining, total) {
  if (!(remaining > 0) || !(total > 0)) return 0;
  const t = 1 - Math.min(1, remaining / total);
  return Math.sin(Math.PI * t);
}

/** Tracer `dir` is the segment start→end; a unit direction is drawn as a 30 m streak. */
export function tracerSpan(dir, fallback = 30) {
  const length = Math.hypot(dir[0], dir[1], dir[2]);
  if (!(length > 0)) return [0, 0, 0];
  return length <= 1.001 ? dir.map(v => v / length * fallback) : dir;
}
