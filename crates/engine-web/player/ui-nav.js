export function focusStep(index, count, direction) {
  return count > 0 ? ((index < 0 ? (direction > 0 ? -1 : 0) : index) + direction + count) % count : -1;
}

export function menuKey(code, shift = false) {
  if (['ArrowDown', 'ArrowRight', 'Tab'].includes(code)) return code === 'Tab' && shift ? 'previous' : 'next';
  if (['ArrowUp', 'ArrowLeft'].includes(code)) return 'previous';
  if (['Enter', 'Space'].includes(code)) return 'activate';
  if (code === 'Escape') return 'back';
  return null;
}

export function menuPad(pads = []) {
  const state = { next: false, previous: false, activate: false, back: false, pause: false, connected: false, held: false };
  for (const pad of Array.from(pads || [])) {
    if (!pad || pad.connected === false || pad.mapping !== 'standard') continue;
    state.connected = true;
    const down = i => pad.buttons?.[i]?.pressed || pad.buttons?.[i]?.value > 0.5;
    const x = Number(pad.axes?.[0]) || 0, y = Number(pad.axes?.[1]) || 0;
    state.previous ||= !!(down(12) || down(14) || x < -0.55 || y < -0.55);
    state.next ||= !!(down(13) || down(15) || x > 0.55 || y > 0.55);
    state.activate ||= !!down(0); state.back ||= !!down(1); state.pause ||= !!down(9);
    state.held ||= (pad.buttons || []).some(b => b.pressed || b.value > 0.5) || (pad.axes || []).some(v => Math.abs(v) > 0.25);
  }
  return state;
}

export class MenuNavigation {
  constructor() { this.previous = {}; this.direction = ''; this.repeatAt = 0; }
  sample(pads, now, capture = false) {
    const state = menuPad(pads), commands = [];
    this.blocked = !!((capture || this.blocked) && state.held);
    for (const name of ['pause', 'back', 'activate']) if (state[name] && !this.previous[name] && !capture && !this.blocked) commands.push(name);
    const direction = state.next === state.previous ? '' : state.next ? 'next' : 'previous';
    if (!capture && !this.blocked && direction && (direction !== this.direction || now >= this.repeatAt)) {
      commands.push(direction); this.repeatAt = now + (direction === this.direction ? 120 : 400);
    }
    this.direction = capture ? '' : direction; this.previous = state;
    return capture ? [] : commands.slice(0, 1);
  }
}
