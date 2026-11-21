import { ACTIONS, describeBinding, parseInputConfig, pushedBindings } from './input.js';

const labels = {
  move_left: 'Nach links', move_right: 'Nach rechts', move_forward: 'Vorwärts', move_back: 'Rückwärts',
  action: 'Aktion / Springen', fire: 'Feuern', reload: 'Nachladen', sprint: 'Sprinten', pause: 'Pause',
  look_x: 'Horizontal zielen', look_y: 'Vertikal zielen',
};
const signature = binding => JSON.stringify(binding);

export class InputSettings {
  constructor(input, authored, title, onOpen, onChange = () => {}) {
    this.input = input; this.authored = authored; this.onOpen = onOpen; this.onChange = onChange;
    this.storageKey = `ai-engine:input:${title}`;
    input.fromJSON(authored); this.warnings = [...input.warnings];
    try {
      const saved = localStorage.getItem(this.storageKey);
      if (saved !== null) {
        const overrides = JSON.parse(saved);
        const parsed = parseInputConfig(overrides);
        this.warnings.push(...parsed.warnings);
        if (overrides && typeof overrides === 'object' && !Array.isArray(overrides)) {
          input.fromJSON({ ...input.toJSON(), ...overrides, bindings: { ...input.config.bindings, ...overrides.bindings } });
        }
      }
    } catch { this.warnings.push('Steuerung: gespeicherte Einstellungen konnten nicht geladen werden.'); }
    this.dialog = document.getElementById('input-panel');
    this.list = document.getElementById('input-bindings');
    this.status = document.getElementById('input-status');
    this.trigger = document.getElementById('input-settings');
    this.trigger.disabled = false;
    this.trigger.addEventListener('click', () => this.show());
    document.getElementById('input-close').addEventListener('click', () => this.dialog.close());
    this.dialog.addEventListener('close', () => { this.cancelCapture(); this.trigger.focus(); });
    this.dialog.addEventListener('cancel', event => { if (this.capturing) { event.preventDefault(); this.cancelCapture(); } });
    this.dialog.addEventListener('keydown', event => {
      if (!this.capturing) return;
      event.preventDefault(); event.stopPropagation();
      if (event.code === 'Escape') this.cancelCapture();
      else if (!event.repeat) this.bind({ type: 'key', code: event.code });
    });
    document.getElementById('input-reset').addEventListener('click', () => {
      this.cancelCapture(); input.fromJSON(this.authored);
      try { localStorage.removeItem(this.storageKey); } catch { this.warnStorage(); }
      this.render(); this.status.textContent = 'Standardsteuerung wiederhergestellt.';
    });
    this.render();
  }
  get open() { return this.dialog.open; }
  show() {
    this.onOpen(); this.cancelCapture(); this.render(); this.dialog.showModal();
    document.getElementById('input-close').focus();
  }
  warnStorage() {
    const warning = 'Steuerung: Speichern ist in diesem Browser nicht verfügbar.';
    if (!this.warnings.includes(warning)) this.warnings.push(warning);
    this.status.textContent = warning;
  }
  cancelCapture() { this.capturing = null; this.status.textContent = 'Wähle eine Aktion zum Belegen. Escape bricht die Aufnahme ab.'; }
  render() {
    this.onChange();
    this.list.replaceChildren();
    for (const action of ACTIONS) {
      const row = document.createElement('li'), text = document.createElement('div');
      const name = document.createElement('strong'), bindings = document.createElement('span'), button = document.createElement('button');
      name.textContent = labels[action]; bindings.textContent = this.input.config.bindings[action].map(describeBinding).join(' · ') || 'Nicht belegt';
      button.textContent = 'Neu belegen'; button.dataset.action = action; button.setAttribute('aria-label', `${labels[action]} neu belegen`);
      button.addEventListener('click', () => {
        this.capturing = action; this.blocked = new Set(pushedBindings(navigator.getGamepads?.() || []).map(signature));
        this.status.textContent = `${labels[action]}: Taste oder Gamepad drücken / Stick bewegen. Escape: Abbrechen.`;
      });
      text.append(name, bindings); row.append(text, button); this.list.append(row);
    }
  }
  bind(binding) {
    const action = this.capturing;
    if (!action) return;
    this.input.config.bindings[action] = [];
    this.input.rebind(action, binding); this.cancelCapture(); this.render();
    this.status.textContent = `${labels[action]}: ${describeBinding(binding)} gespeichert.`;
    try { localStorage.setItem(this.storageKey, JSON.stringify(this.input)); } catch { this.warnStorage(); }
    this.list.querySelector(`[data-action="${action}"]`).focus();
  }
  poll(pads) {
    if (!this.open || !this.capturing) return;
    const pushed = pushedBindings(pads), held = new Set(pushed.map(signature));
    for (const value of this.blocked) if (!held.has(value)) this.blocked.delete(value);
    const next = pushed.find(binding => !this.blocked.has(signature(binding)));
    if (next) this.bind(next);
  }
}
