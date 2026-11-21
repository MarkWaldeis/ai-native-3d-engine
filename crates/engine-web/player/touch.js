import { ACTIONS } from './input.js';

export const TOUCH_LABELS = { move_left: 'Nach links', move_right: 'Nach rechts', move_forward: 'Vorwärts', move_back: 'Rückwärts', action: 'Aktion / Springen', fire: 'Feuern', reload: 'Nachladen', sprint: 'Sprinten', pause: 'Pause', look_x: 'Horizontal zielen', look_y: 'Vertikal zielen' };
const anchors = ['bottom-right', 'right', 'bottom-left', 'top-right'];
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const finite = value => typeof value === 'number' && Number.isFinite(value);

export function parseTouchConfig(value) {
  const config = { buttons: [], joystick: { side: 'left', size: 120 }, hide_default_action: false }, warnings = [];
  const warn = key => warnings.push(`Touch: ${key} ist ungültig.`);
  if (value === undefined || value === null) return { config, warnings };
  try {
    if (typeof value === 'string') value = JSON.parse(value);
    if (!object(value)) throw new Error();
    if (value.buttons !== undefined) {
      if (!Array.isArray(value.buttons)) warn('buttons');
      else {
        if (value.buttons.length > 12) warnings.push('Touch: höchstens 12 Zusatztasten.');
        for (const raw of value.buttons.slice(0, 12)) {
          if (!object(raw) || !ACTIONS.includes(raw.action)) { warn('button.action'); continue; }
          const button = { action: raw.action, label: TOUCH_LABELS[raw.action], anchor: 'bottom-right', size: 'md', hold: true };
          const valid = { label: v => typeof v === 'string' && !!v.trim(), icon: v => typeof v === 'string', anchor: v => anchors.includes(v), size: v => ['md', 'lg'].includes(v), hold: v => typeof v === 'boolean' };
          for (const [key, check] of Object.entries(valid)) if (raw[key] !== undefined) {
            if (check(raw[key])) button[key] = raw[key]; else warn(`button.${key}`);
          }
          if (['look_x', 'look_y'].includes(button.action) && !button.hold) {
            button.hold = true; warnings.push('Touch: Look-Aktionen benötigen hold:true.');
          }
          config.buttons.push(button);
        }
      }
    }
    if (value.joystick !== undefined) {
      if (!object(value.joystick)) warn('joystick');
      else {
        if (value.joystick.side !== undefined) {
          if (['left', 'right'].includes(value.joystick.side)) config.joystick.side = value.joystick.side; else warn('joystick.side');
        }
        const size = value.joystick.size;
        if (size !== undefined) {
          if (['md', 'lg'].includes(size)) config.joystick.size = size === 'md' ? 120 : 150;
          else if (finite(size) && size >= 80 && size <= 180) config.joystick.size = size;
          else warn('joystick.size');
        }
      }
    }
    if (value.hide_default_action !== undefined) {
      if (typeof value.hide_default_action === 'boolean') config.hide_default_action = value.hide_default_action; else warn('hide_default_action');
    }
  } catch { warnings.push('Touch: Konfiguration konnte nicht gelesen werden.'); }
  return { config, warnings };
}

export function layoutTouch(config, { width, height, insets = {}, fps = false, obstacles = [] }) {
  const left = (insets.left || 0) + 12, right = width - (insets.right || 0) - 12;
  const top = (insets.top || 0) + (height < 500 ? 110 : 160), bottom = height - (insets.bottom || 0) - (height < 500 ? 44 : 64);
  const size = Math.min(config.joystick.size, height < 500 ? 100 : 180, right - left);
  const result = [], occupied = obstacles.filter(b => [b.x, b.y, b.width, b.height].every(Number.isFinite) && b.width > 0 && b.height > 0);
  const opposite = config.joystick.side === 'left' ? 'bottom-right' : 'bottom-left';
  const entries = [{ id: 'joystick', size, extraHeight: 22, anchor: `bottom-${config.joystick.side}` },
    ...(config.hide_default_action ? [] : [{ id: 'action', size: height < 500 ? 64 : 78, anchor: opposite }]),
    ...(fps ? [{ id: 'fire', size: height < 500 ? 72 : 84, anchor: opposite }, { id: 'reload', size: 56, anchor: opposite }] : []),
    ...config.buttons.map((button, i) => ({ id: `touch-${i}`, size: button.size === 'lg' ? 72 : 56, anchor: button.anchor }))];
  for (const entry of entries) {
    let best, score = Infinity;
    const occupiedHeight = entry.size + (entry.extraHeight || 0);
    const tx = entry.anchor === 'bottom-left' ? left : right - entry.size;
    const ty = entry.anchor === 'top-right' ? top : entry.anchor === 'right' ? (top + bottom - occupiedHeight) / 2 : bottom - occupiedHeight;
    for (let y = bottom - occupiedHeight; y >= top; y -= 8) for (let x = left; x + entry.size <= right; x += 8) {
      const rect = { id: entry.id, x, y, width: entry.size, height: entry.size };
      if (occupied.some(b => x < b.x + b.width + 8 && x + entry.size + 8 > b.x && y < b.y + b.height + 8 && y + occupiedHeight + 8 > b.y)) continue;
      const distance = (x - tx) ** 2 + (y - ty) ** 2;
      if (distance < score) { best = rect; score = distance; }
    }
    if (!best && entry.id === 'joystick') {
      if (size > 80) return layoutTouch({ ...config, joystick: { ...config.joystick, size: 80 } }, { width, height, insets, fps, obstacles });
      best = { id: 'joystick', x: Math.max(0, Math.min(tx, width - size)), y: Math.max(0, Math.min(ty, height - size)), width: size, height: size };
    }
    if (best) { result.push(best); occupied.push({ ...best, height: occupiedHeight }); }
  }
  return result;
}

export class TouchState {
  constructor(buttons) { this.buttons = buttons; this.pointers = new Map(); this.queued = new Set(); }
  press(index, pointer) {
    if (!this.buttons[index] || this.pointers.has(pointer)) return false;
    this.pointers.set(pointer, index); this.queued.add(index); return true;
  }
  release(pointer, cancelled = false) {
    const index = this.pointers.get(pointer); this.pointers.delete(pointer);
    if (cancelled && ![...this.pointers.values()].includes(index)) this.queued.delete(index);
  }
  sample(queued = true) {
    const actions = {};
    for (const index of new Set([...(queued ? this.queued : []), ...[...this.pointers.values()].filter(i => this.buttons[i].hold)])) actions[this.buttons[index].action] = 1;
    return actions;
  }
  consume() { this.queued.clear(); }
  reset() { this.pointers.clear(); this.queued.clear(); }
}

export function hapticCue(feedback = {}, events = []) {
  if (feedback.win) return 'win';
  if (events.some(event => event.type === 'damage' && event.to_player)) return 'damage';
  if (feedback.collect) return 'collect';
  if (events.some(event => event.type === 'hit' || (event.type === 'damage' && event.by_player))) return 'hit';
  return null;
}

export function pulseHaptics(cue, { settings, pads = [], gamepadActive = false, touch = false, device = globalThis.navigator } = {}) {
  try {
    if (!cue || settings?.enabled === false) return false;
    const strength = Math.max(0, Math.min(1, settings?.strength ?? 1));
    if (!strength) return false;
    const [duration, strong, weak] = { collect: [60, 0.4, 0.3], damage: [100, 0.8, 1], hit: [35, 0.3, 0.2], win: [180, 0.8, 0.6] }[cue] || [];
    if (!duration) return false;
    if (gamepadActive) {
      for (const pad of Array.from(pads ?? [])) {
        if (!pad || pad.connected === false || typeof pad.vibrationActuator?.playEffect !== 'function') continue;
        try { pad.vibrationActuator.playEffect('dual-rumble', { duration, startDelay: 0, strongMagnitude: strong * strength, weakMagnitude: weak * strength })?.catch?.(() => {}); return true; } catch { /* unsupported actuator */ }
      }
    } else if (touch && typeof device?.vibrate === 'function') { device.vibrate(Math.round(duration * strength)); return true; }
  } catch { /* unsupported device */ }
  return false;
}
