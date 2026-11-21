import { parseHudConfig, resolveHudWidgets, hudVisible, hudTemplate, hudBar, countHudTags, layoutHud } from './hud-layout.js';

const setText = (node, text) => { if (node.textContent !== text) node.textContent = text; };

export class HudLayout {
  constructor(authored, entities) {
    const parsed = parseHudConfig(authored);
    this.config = parsed.config; this.warnings = parsed.warnings; this.nodes = new Map();
    this.root = document.createElement('div'); this.root.id = 'hud-layout';
    this.root.style.cssText = 'position:fixed;inset:0;pointer-events:none;padding:env(safe-area-inset-top) env(safe-area-inset-right) env(safe-area-inset-bottom) env(safe-area-inset-left)';
    document.getElementById('game').after(this.root);
    this.make(this.config.widgets, this.root);
    if (this.config.replace_default) document.getElementById('state').hidden = true;
    this.resize = () => { this.updateWidgets(); this.layout(); };
    window.addEventListener('resize', this.resize);
    this.reset(entities);
  }
  make(widgets, parent) {
    for (const widget of widgets) {
      const node = document.createElement('div'); node.dataset.hudId = widget.id;
      const label = document.createElement('span'); label.className = 'hud-widget-label';
      node.append(label); parent.append(node);
      const record = { node, label, widget, initialized: false, until: 0 };
      if (['text', 'counter', 'toast'].includes(widget.type)) node.setAttribute('aria-live', 'polite');
      if (widget.type === 'bar') {
        const track = document.createElement('div'), fill = document.createElement('i');
        track.className = 'hud-widget-track'; fill.className = 'hud-widget-fill'; track.append(fill); node.append(track);
        track.setAttribute('role', 'progressbar'); track.setAttribute('aria-label', widget.text || widget.bind || widget.id);
        track.setAttribute('aria-valuemin', '0'); track.setAttribute('aria-valuemax', '100');
        record.fill = fill; record.track = track;
      }
      this.nodes.set(widget.id, record);
      if (widget.children) this.make(widget.children, node);
    }
  }
  reset(entities) {
    this.entities = new Map();
    this.totals = countHudTags(entities); this.state = {}; this.left = { ...this.totals };
    for (const record of this.nodes.values()) { record.initialized = false; record.until = 0; if (record.widget.type === 'toast') record.node.hidden = true; }
  }
  update(snapshot, now = performance.now()) {
    for (const id of snapshot.removed || []) this.entities.delete(id);
    for (const entity of snapshot.spawned || []) this.entities.set(entity.id, entity);
    this.left = countHudTags(this.entities.values()); this.state = snapshot.state;
    if (!this.config.widgets.length) return;
    this.updateWidgets(now); this.layout();
  }
  updateWidgets(now = performance.now()) {
    const visit = widgets => {
      for (const widget of widgets) {
        const record = this.nodes.get(widget.id), { node, label } = record;
        const value = this.state[widget.bind];
        if (widget.type === 'toast') {
          const key = value !== null && typeof value === 'object' ? JSON.stringify(value) : value;
          if (record.initialized && !Object.is(record.value, key)) record.until = now + 2000;
          record.value = key; record.initialized = true;
        }
        const hidden = !!widget.hidden || !hudVisible(widget, this.state) || (widget.type === 'toast' && now >= record.until);
        if (node.hidden !== hidden) node.hidden = hidden;
        const className = `hud-widget hud-${widget.type} hud-size-${widget.size} hud-style-${widget.style}`;
        if (node.className !== className) node.className = className;
        if (widget.color && node.style.getPropertyValue('--hud-color') !== widget.color) node.style.setProperty('--hud-color', widget.color);
        const template = widget.text || (widget.bind ? `{${widget.bind}}` : '');
        setText(label, `${widget.icon ? `${widget.icon} ` : ''}${hudTemplate(template, this.state, this.totals, this.left)}`);
        if (record.fill) {
          const { ratio, low } = hudBar(widget, this.state);
          const width = `${ratio * 100}%`;
          if (record.ratio !== ratio) { record.fill.style.width = width; record.ratio = ratio; }
          if (record.track.getAttribute('aria-valuenow') !== String(Math.round(ratio * 100))) record.track.setAttribute('aria-valuenow', String(Math.round(ratio * 100)));
          if (record.fill.classList.contains('low') !== low) record.fill.classList.toggle('low', low);
        }
        if (widget.children) visit(widget.children);
      }
    };
    visit(resolveHudWidgets(this.config.widgets, innerWidth));
    // Hidden phone widgets still have persistent nodes.
    const hidePhone = widgets => { for (const widget of widgets) { const node = this.nodes.get(widget.id).node; if (innerWidth < 600 && widget.phone?.hidden && !node.hidden) node.hidden = true; if (widget.children) hidePhone(widget.children); } };
    hidePhone(this.config.widgets);
  }
  frame(now) {
    for (const record of this.nodes.values()) if (record.widget.type === 'toast' && record.until && now >= record.until) { record.node.hidden = true; record.until = 0; }
  }
  layout() {
    const style = getComputedStyle(this.root);
    const insets = Object.fromEntries(['top', 'right', 'bottom', 'left'].map(side => [side, parseFloat(style.getPropertyValue(`padding-${side}`)) || 0]));
    const touch = matchMedia('(pointer: coarse)').matches || document.body.classList.contains('touch-input');
    const controls = [...document.querySelectorAll('#joystick,#action,.fps-fire,.fps-reload-button,[data-touch-action]')].filter(node => node.getClientRects().length).map(node => node.getBoundingClientRect().top);
    const widgets = resolveHudWidgets(this.config.widgets, innerWidth).filter(w => !this.nodes.get(w.id).node.hidden);
    const viewport = { width: innerWidth, height: innerHeight, insets, touch, controlsTop: controls.length ? Math.min(...controls) : undefined };
    let heights = Object.fromEntries(widgets.filter(w => this.nodes.get(w.id).node.style.width).map(w => [w.id, this.nodes.get(w.id).node.scrollHeight + 2]));
    for (let pass = 0; pass < 2; pass++) {
      for (const position of layoutHud(widgets, viewport, heights)) {
        const record = this.nodes.get(position.id), node = record.node;
        const css = { position: 'absolute', left: `${position.x}px`, top: `${position.y}px`, width: `${position.width}px`, maxHeight: `${position.maxHeight}px` };
        for (const [key, value] of Object.entries(css)) if (record.layoutCSS?.[key] !== value) node.style[key] = value;
        record.layoutCSS = css;
      }
      heights = Object.fromEntries(widgets.map(w => [w.id, this.nodes.get(w.id).node.scrollHeight + 2]));
    }
  }
}
