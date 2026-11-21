import { parseTouchConfig, layoutTouch, TouchState, TOUCH_LABELS } from './touch.js';

export class TouchControls {
  constructor(authored, { fps = false, enabled, onInteract, onLayout }) {
    const parsed = parseTouchConfig(authored);
    this.config = parsed.config; this.warnings = parsed.warnings; this.authored = authored !== undefined && authored !== null;
    this.fps = fps; this.onLayout = onLayout; this.state = new TouchState(this.config.buttons);
    this.root = document.createElement('div'); this.root.id = 'touch-buttons';
    document.getElementById('controls').append(this.root);
    this.nodes = this.config.buttons.map((button, i) => {
      const node = document.createElement('button'); node.dataset.touchAction = button.action;
      node.setAttribute('aria-label', `${button.label}: ${TOUCH_LABELS[button.action]}`);
      if (button.icon) { const icon = document.createElement('span'); icon.textContent = button.icon; icon.setAttribute('aria-hidden', 'true'); node.append(icon); }
      const label = document.createElement('span'); label.textContent = button.label; node.append(label);
      this.root.append(node);
      node.addEventListener('pointerdown', event => {
        if (!enabled()) return;
        event.preventDefault(); onInteract();
        if (this.state.press(i, event.pointerId)) { try { node.setPointerCapture(event.pointerId); } catch { /* ended pointer */ } }
        this.pressed();
      });
      for (const type of ['pointerup', 'pointercancel', 'lostpointercapture']) node.addEventListener(type, event => { this.state.release(event.pointerId, type !== 'pointerup'); this.pressed(); });
      node.addEventListener('click', event => {
        if (event.detail || !enabled()) return;
        onInteract(); this.state.press(i, `key-${i}`); this.state.release(`key-${i}`); this.pressed();
      });
      return node;
    });
    if (this.authored) document.getElementById('action').hidden = this.config.hide_default_action;
    window.addEventListener('resize', () => this.layout());
    this.layout();
  }
  sample() { return this.state.sample(); }
  consume() { this.state.consume(); this.pressed(); }
  reset() { this.state.reset(); this.pressed(); }
  pressed() {
    const held = new Set(this.state.pointers.values());
    this.nodes.forEach((node, i) => node.classList.toggle('pressed', held.has(i)));
  }
  layout() {
    if (!this.authored) return;
    const style = getComputedStyle(this.root);
    const insets = Object.fromEntries(['top', 'right', 'bottom', 'left'].map(side => [side, parseFloat(style.getPropertyValue(`padding-${side}`)) || 0]));
    const positions = layoutTouch(this.config, { width: innerWidth, height: innerHeight, insets, fps: this.fps });
    for (const position of positions) {
      const { id, x, y, width, height } = position;
      const node = id === 'joystick' ? document.querySelector('.joystick-area') : id === 'action' ? document.getElementById('action') : id === 'fire' ? document.querySelector('.fps-fire') : id === 'reload' ? document.querySelector('.fps-reload-button') : this.nodes[Number(id.slice(6))];
      if (!node) continue;
      Object.assign(node.style, { left: `${x}px`, top: `${y}px`, right: 'auto', bottom: 'auto', width: `${width}px`, height: `${height}px` });
      if (id === 'joystick') {
        const joystick = document.getElementById('joystick'), stick = document.getElementById('stick');
        joystick.style.width = `${width}px`; joystick.style.height = `${height}px`;
        stick.style.left = `${(width - 54) / 2}px`; stick.style.top = `${(height - 54) / 2}px`;
      }
    }
    this.nodes.forEach((node, i) => { node.hidden = !positions.some(p => p.id === `touch-${i}`); });
    if (this.nodes.some(node => node.hidden) && !this.warnings.includes('Touch: zu wenig Platz für alle Tasten.')) this.warnings.push('Touch: zu wenig Platz für alle Tasten.');
    this.onLayout?.();
  }
}
