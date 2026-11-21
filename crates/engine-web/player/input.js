export const ACTIONS = Object.freeze(['move_left', 'move_right', 'move_forward', 'move_back', 'action', 'fire', 'reload', 'sprint', 'pause', 'look_x', 'look_y']);
export class InputEdges {
  previous = new Set();
  sample(actions) {
    const held = new Set(Object.keys(actions).filter(action => actions[action] > 0.5));
    const pressed = new Set([...held].filter(action => !this.previous.has(action)));
    this.previous = held;
    return pressed;
  }
}

export function pushedBindings(pads) {
  const bindings = [];
  for (const pad of Array.from(pads ?? [])) {
    if (!pad || pad.connected === false) continue;
    Array.from(pad.buttons ?? []).forEach((b, index) => {
      if (typeof b === 'number' ? b > 0.6 : b?.pressed || b?.value > 0.6) bindings.push({ type: 'button', index });
    });
    Array.from(pad.axes ?? []).forEach((value, index) => {
      if (Number.isFinite(value) && Math.abs(value) > 0.6) bindings.push({ type: 'axis', index, dir: Math.sign(value) });
    });
  }
  return bindings;
}
const key = code => ({ type: 'key', code });
const button = index => ({ type: 'button', index });
const axis = (index, dir) => ({ type: 'axis', index, dir });
const defaults = () => ({
  bindings: {
    move_left: [key('KeyA'), key('ArrowLeft'), axis(0, -1), button(14)],
    move_right: [key('KeyD'), key('ArrowRight'), axis(0, 1), button(15)],
    move_forward: [key('KeyW'), key('ArrowUp'), axis(1, -1), button(12)],
    move_back: [key('KeyS'), key('ArrowDown'), axis(1, 1), button(13)],
    action: [key('Space'), button(0)], fire: [button(7)],
    reload: [key('KeyR'), button(2)], sprint: [key('ShiftLeft'), key('ShiftRight'), button(10)],
    pause: [key('Escape'), button(9)], look_x: [axis(2, 1)], look_y: [axis(3, 1)],
  }, deadzone: 0.15, look_sensitivity: 1, invert_y: false,
});
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const finite = value => typeof value === 'number' && Number.isFinite(value);
const number = value => finite(value) ? value : 0;
function bindingCopy(value) {
  if (!object(value)) return null;
  if (value.type === 'key' && typeof value.code === 'string' && /^[A-Za-z][A-Za-z0-9]*$/.test(value.code)) return key(value.code);
  if (!Number.isInteger(value.index) || value.index < 0 || value.index > 255) return null;
  if (value.type === 'button') return button(value.index);
  if (value.type === 'axis' && (value.dir === 1 || value.dir === -1)) return axis(value.index, value.dir);
  return null;
}
const same = (a, b) => a.type === b.type && a.code === b.code && a.index === b.index && a.dir === b.dir;

export function parseInputConfig(value) {
  const config = defaults(), warnings = [];
  if (value === undefined) return { config, warnings };
  if (typeof value === 'string') {
    try { value = JSON.parse(value); } catch { return { config, warnings: ['ai_input: ungültiges JSON'] }; }
  }
  if (!object(value)) return { config, warnings: ['ai_input: Objekt erwartet'] };
  for (const name of ['deadzone', 'look_sensitivity', 'invert_y']) {
    if (!(name in value)) continue;
    const valid = name === 'invert_y' ? typeof value[name] === 'boolean'
      : finite(value[name]) && (name === 'deadzone' ? value[name] >= 0 && value[name] < 1 : value[name] > 0);
    if (valid) config[name] = value[name]; else warnings.push(`ai_input.${name}: ungültiger Wert`);
  }
  if ('bindings' in value) {
    if (!object(value.bindings)) warnings.push('ai_input.bindings: Objekt erwartet');
    else for (const [action, entries] of Object.entries(value.bindings)) {
      if (!ACTIONS.includes(action) || !Array.isArray(entries)) { warnings.push(`ai_input.bindings.${action}: ungültig`); continue; }
      const bindings = entries.map(bindingCopy);
      if (bindings.some(binding => !binding)) { warnings.push(`ai_input.bindings.${action}: ungültige Bindung`); continue; }
      config.bindings[action] = bindings.filter((binding, i) => bindings.findIndex(other => same(binding, other)) === i);
    }
  }
  return { config, warnings };
}

export function clampVector(x, y) {
  x = number(x); y = number(y);
  const length = Math.max(1, Math.hypot(x, y));
  return { x: x / length, y: y / length };
}

export function radialDeadzone(x, y, deadzone = 0.15) {
  x = number(x); y = number(y);
  if (!finite(deadzone) || deadzone < 0 || deadzone >= 1) deadzone = 0.15;
  const length = Math.hypot(x, y);
  if (length <= deadzone) return { x: 0, y: 0 };
  const scale = Math.min(1, (length - deadzone) / (1 - deadzone)) / length;
  return { x: x * scale, y: y * scale };
}

const emptyActions = () => Object.fromEntries(ACTIONS.map(action => [action, 0]));
function axes(actions) {
  const move = clampVector(actions.move_right - actions.move_left, actions.move_back - actions.move_forward);
  const look = clampVector(actions.look_x, actions.look_y);
  return { actions, move_x: move.x, move_z: move.y, look_x: look.x, look_y: look.y };
}

export function sampleGamepads(pads = [], config = defaults()) {
  const actions = emptyActions();
  for (const pad of Array.from(pads ?? [])) {
    if (!pad || pad.connected === false) continue;
    const raw = pad.axes ?? [], filtered = [];
    for (let i = 0; i < raw.length; i += 2) {
      const stick = radialDeadzone(raw[i], raw[i + 1], config.deadzone);
      filtered[i] = stick.x; filtered[i + 1] = stick.y;
    }
    for (const action of ACTIONS) for (const binding of config.bindings[action]) {
      let value = 0;
      if (binding.type === 'button') {
        const b = pad.buttons?.[binding.index];
        value = (typeof b === 'number' ? b > 0.5 : b?.pressed || number(b?.value) > 0.5) ? 1 : 0;
      } else if (binding.type === 'axis') {
        value = number(filtered[binding.index]) * binding.dir;
        if (action !== 'look_x' && action !== 'look_y') value = Math.max(0, value);
      }
      if (Math.abs(value) > Math.abs(actions[action])) actions[action] = value;
    }
  }
  const result = axes(actions);
  result.look_x *= config.look_sensitivity;
  result.look_y *= config.look_sensitivity * (config.invert_y ? -1 : 1);
  return result;
}

export class InputSystem {
  constructor(value) { this.fromJSON(value); }
  fromJSON(value) {
    const parsed = parseInputConfig(value);
    this.config = parsed.config; this.warnings = parsed.warnings;
    return this;
  }
  toJSON() { return JSON.parse(JSON.stringify(this.config)); }
  reset() { return this.fromJSON(undefined); }
  rebind(action, value) {
    const binding = bindingCopy(value);
    if (!ACTIONS.includes(action) || !binding) return false;
    for (const name of ACTIONS) this.config.bindings[name] = this.config.bindings[name].filter(other => !same(binding, other));
    this.config.bindings[action].push(binding);
    return true;
  }
  sampleGamepads(pads) { return sampleGamepads(pads, this.config); }
  sample({ keys = new Set(), pads = [], touch = {} } = {}) {
    const result = this.sampleGamepads(pads), actions = { ...result.actions };
    for (const action of ACTIONS) {
      if (this.config.bindings[action].some(binding => binding.type === 'key' && keys.has(binding.code))) actions[action] = 1;
    }
    const keyboard = emptyActions();
    for (const action of ACTIONS) keyboard[action] = this.config.bindings[action].some(binding => binding.type === 'key' && keys.has(binding.code)) ? 1 : 0;
    const move = clampVector(result.move_x + keyboard.move_right - keyboard.move_left + number(touch.x), result.move_z + keyboard.move_back - keyboard.move_forward + number(touch.z));
    return { ...result, actions, move_x: move.x, move_z: move.y,
      look_x: result.look_x + keyboard.look_x * this.config.look_sensitivity,
      look_y: result.look_y + keyboard.look_y * this.config.look_sensitivity * (this.config.invert_y ? -1 : 1) };
  }
}

export function describeBinding(value) {
  const binding = bindingCopy(value);
  if (!binding) return 'Unbekannte Bindung';
  if (binding.type === 'key') return `Taste ${{ Space: 'Leertaste', ArrowUp: '↑', ArrowDown: '↓', ArrowLeft: '←', ArrowRight: '→', Escape: 'Esc' }[binding.code] ?? binding.code.replace(/^Key|^Digit/, '')}`;
  if (binding.type === 'button') return `Gamepad ${['A', 'B', 'X', 'Y', 'LB', 'RB', 'LT', 'RT', 'Zurück', 'Start', 'L3', 'R3', 'Steuerkreuz ↑', 'Steuerkreuz ↓', 'Steuerkreuz ←', 'Steuerkreuz →'][binding.index] ?? `Taste ${binding.index}`}`;
  const arrow = binding.index % 2 ? (binding.dir > 0 ? '↓' : '↑') : (binding.dir > 0 ? '→' : '←');
  return `${binding.index < 2 ? 'Linker Stick' : binding.index < 4 ? 'Rechter Stick' : `Achse ${binding.index}`} ${arrow}`;
}
