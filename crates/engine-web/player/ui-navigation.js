import { MenuNavigation, menuKey, menuPad, focusStep } from './ui-nav.js';

const focusable = 'button,input,select,textarea,a[href],[tabindex]';
const visible = node => node.getClientRects().length && getComputedStyle(node).visibility !== 'hidden';

export class OverlayNavigation {
  constructor({ capture, pause, back, reset }) {
    this.capture = capture; this.pause = pause; this.back = back; this.reset = reset;
    this.navigation = new MenuNavigation(); this.stack = []; this.focus = new WeakMap(); this.keys = new Set();
    document.addEventListener('keydown', event => this.keydown(event), true);
    document.addEventListener('keyup', event => this.keys.delete(event.code), true);
    window.addEventListener('blur', () => this.keys.clear());
    document.addEventListener('focusin', event => {
      this.sync();
      if (this.scope?.contains(event.target)) this.focus.set(this.scope, event.target);
      else if (this.scope) this.primary()?.focus();
    });
    new MutationObserver(() => this.sync()).observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ['hidden', 'open', 'disabled', 'data-screen'] });
    this.sync();
  }
  controls() { return this.scope ? [...this.scope.querySelectorAll(focusable)].filter(node => !node.matches(':disabled') && !node.closest('[inert]') && node.tabIndex >= 0 && visible(node)) : []; }
  primary() { const controls = this.controls(); return controls.find(node => node.matches('[data-primary],#start')) || controls[0]; }
  sync() {
    const scopes = [...document.querySelectorAll('dialog[open],[aria-modal="true"]')].filter(visible);
    this.stack = [...this.stack.filter(node => scopes.includes(node)), ...scopes.filter(node => !this.stack.includes(node))];
    const modal = this.stack.findLast(node => node.matches(':modal'));
    const scope = (modal ? this.stack.findLast(node => node === modal || modal.contains(node)) : this.stack.at(-1)) || null;
    document.body.classList.toggle('ui-overlay', !!scope);
    const screen = scope?.dataset.screen, changed = scope !== this.scope;
    if (changed || screen !== this.screen) {
      const previous = this.scope;
      this.scope = scope; this.screen = screen; this.quarantine = true; this.reset();
      if (scope) {
        if (!scope.querySelector('.ui-nav-hint')) {
          const hint = document.createElement('p'); hint.className = 'ui-nav-hint'; hint.hidden = !this.connected;
          hint.textContent = 'Ⓐ Auswählen · Ⓑ Zurück';
          if (scope.tagName === 'DIALOG') scope.prepend(hint); else (scope.querySelector('.modal-card') || scope).append(hint);
        }
        const saved = changed && previous ? this.focus.get(scope) : null;
        (this.controls().includes(saved) ? saved : this.primary())?.focus();
      } else if (previous) document.getElementById('game')?.focus({ preventScroll: true });
    }
    if (scope && !scope.contains(document.activeElement)) this.primary()?.focus();
  }
  command(command) {
    this.sync();
    if (command === 'pause') { this.pause(); this.sync(); return; }
    if (!this.scope) return;
    if (command === 'back') {
      if (this.scope.tagName === 'DIALOG') {
        if (this.scope.dispatchEvent(new Event('cancel', { cancelable: true }))) this.scope.close();
      } else this.back();
    } else if (command === 'activate') document.activeElement?.click();
    else {
      const controls = this.controls(), index = focusStep(controls.indexOf(document.activeElement), controls.length, command === 'next' ? 1 : -1);
      controls[index]?.focus(); controls[index]?.scrollIntoView({ block: 'nearest' });
    }
    this.sync();
  }
  keydown(event) {
    this.sync();
    const held = this.keys.has(event.code);
    if (this.scope) this.keys.add(event.code);
    if (this.capture()) return;
    if (!this.scope) {
      if (held) { event.preventDefault(); event.stopImmediatePropagation(); }
      return;
    }
    const command = menuKey(event.code, event.shiftKey);
    if (!command) return;
    event.preventDefault(); event.stopImmediatePropagation();
    if (!event.repeat || command === 'next' || command === 'previous') this.command(command);
  }
  poll(pads, now) {
    this.sync();
    const state = menuPad(pads); this.connected = state.connected;
    document.body.classList.toggle('gamepad-ui', state.connected);
    for (const hint of document.querySelectorAll('.ui-nav-hint')) if (hint.hidden === state.connected) hint.hidden = !state.connected;
    if (!state.held && !this.scope) this.quarantine = false;
    for (const command of this.navigation.sample(pads, now, this.capture())) this.command(command);
    return !!this.scope || this.quarantine || this.keys.size > 0;
  }
}
