export const HUD_ANCHORS = ['top-left', 'top', 'top-right', 'left', 'center', 'right', 'bottom-left', 'bottom', 'bottom-right'];
const types = ['text', 'bar', 'counter', 'panel', 'toast'];
const sizes = ['sm', 'md', 'lg', 'xl'];
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const finite = value => typeof value === 'number' && Number.isFinite(value);
const offset = value => Array.isArray(value) && value.length === 2 && value.every(finite);

export function parseHudConfig(value) {
  const warnings = [], ids = new Set();
  const warn = text => warnings.push(`HUD: ${text}`);
  const empty = { replace_default: false, widgets: [] };
  if (value === undefined || value === null) return { config: empty, warnings };
  try {
    if (typeof value === 'string') value = JSON.parse(value);
    if (!object(value)) throw new Error('Konfiguration muss ein Objekt sein.');
    const parse = (entries, depth = 0) => {
      if (!Array.isArray(entries)) { warn('widgets/children muss eine Liste sein.'); return []; }
      if (depth > 4) { warn('Panels sind zu tief verschachtelt.'); return []; }
      return entries.slice(0, 100).flatMap(raw => {
        if (!object(raw) || typeof raw.id !== 'string' || !raw.id || ids.has(raw.id) || !types.includes(raw.type)) {
          warn('Widget mit ungültiger ID oder Typ ignoriert.'); return [];
        }
        ids.add(raw.id);
        const widget = { id: raw.id, type: raw.type, anchor: 'top-left', offset: [0, 0], width: 220, text: '', size: 'md', style: 'card' };
        const validators = {
          anchor: v => HUD_ANCHORS.includes(v), offset, width: v => finite(v) && v > 0,
          text: v => typeof v === 'string', bind: v => typeof v === 'string',
          max: v => (finite(v) && v > 0) || (typeof v === 'string' && !!v),
          color: v => typeof v === 'string' && /^(#[\da-f]{3,8}|[a-z]+|rgba?\([\d.,%\s]+\))$/i.test(v),
          size: v => sizes.includes(v), style: v => ['card', 'plain'].includes(v),
          visible_if: v => typeof v === 'string' && /^!?[\w.-]+$/.test(v), icon: v => typeof v === 'string',
        };
        for (const [key, valid] of Object.entries(validators)) {
          if (raw[key] !== undefined) {
            if (valid(raw[key])) widget[key] = Array.isArray(raw[key]) ? [...raw[key]] : raw[key];
            else warn(`${raw.id}.${key} ist ungültig.`);
          }
        }
        if (raw.phone !== undefined) {
          widget.phone = {};
          if (!object(raw.phone)) warn(`${raw.id}.phone ist ungültig.`);
          else for (const key of ['anchor', 'offset', 'size', 'hidden']) {
            if (raw.phone[key] === undefined) continue;
            if ((key === 'hidden' ? v => typeof v === 'boolean' : validators[key])(raw.phone[key])) widget.phone[key] = raw.phone[key];
            else warn(`${raw.id}.phone.${key} ist ungültig.`);
          }
        }
        if (raw.type === 'panel') widget.children = parse(raw.children ?? [], depth + 1);
        return [widget];
      });
    };
    if (value.replace_default !== undefined && typeof value.replace_default !== 'boolean') warn('replace_default muss boolesch sein.');
    const config = { replace_default: value.replace_default === true, widgets: parse(value.widgets ?? []) };
    return { config, warnings };
  } catch { warn('Konfiguration konnte nicht gelesen werden.'); return { config: empty, warnings }; }
}

export function resolveHudWidgets(widgets, width) {
  return widgets.map(widget => {
    const resolved = { ...widget, ...(width < 600 ? widget.phone : {}) };
    if (widget.children) resolved.children = resolveHudWidgets(widget.children, width);
    return resolved;
  }).filter(widget => !widget.hidden);
}

export function hudVisible(widget, state) {
  const key = widget.visible_if;
  if (!key) return true;
  const name = key.startsWith('!') ? key.slice(1) : key;
  const visible = Object.hasOwn(state, name) && !!state[name];
  return key.startsWith('!') ? !visible : visible;
}

export function hudTemplate(text, state = {}, totals = {}, left = {}) {
  return String(text).replace(/\{([^{}]+)\}/g, (_, token) => {
    if (token.startsWith('total:') || token.startsWith('left:')) {
      const [kind, tag] = token.split(':');
      const counts = kind === 'total' ? totals : left;
      return Object.hasOwn(counts, tag) ? counts[tag] : 0;
    }
    const [key, format] = token.split('|');
    const value = Object.hasOwn(state, key) ? state[key] : undefined;
    if (value === null || value === undefined || typeof value === 'object') return '–';
    if (format && /^0(?:\.0+)?$/.test(format)) return finite(value) ? value.toFixed(Math.min(20, format.length - 2 > 0 ? format.length - 2 : 0)) : '–';
    return String(value);
  });
}

export function hudBar(widget, state) {
  const value = state[widget.bind], max = typeof widget.max === 'string' ? state[widget.max] : widget.max ?? 100;
  const ratio = finite(value) && finite(max) && max > 0 ? Math.max(0, Math.min(1, value / max)) : 0;
  return { ratio, low: ratio < 0.3 };
}

export function countHudTags(entities) {
  const counts = Object.create(null);
  for (const entity of entities) for (const tag of new Set(entity.tags || [])) counts[tag] = (counts[tag] || 0) + 1;
  return counts;
}

export function layoutHud(widgets, { width, height, insets = {}, touch = false, controlsTop }, heights = {}) {
  const gap = 8, margin = 12;
  const left = (insets.left || 0) + margin, right = width - (insets.right || 0) - margin;
  const top = (insets.top || 0) + margin, bottom = height - (insets.bottom || 0) - margin;
  const positions = [], groups = new Map();
  for (const widget of resolveHudWidgets(widgets, width)) {
    if (!groups.has(widget.anchor)) groups.set(widget.anchor, []);
    groups.get(widget.anchor).push(widget);
  }
  for (const [anchor, group] of groups) {
    const isBottom = anchor.startsWith('bottom'), isTop = anchor.startsWith('top');
    const limit = touch && ['bottom-left', 'bottom-right'].includes(anchor) ? Math.min(bottom, controlsTop ?? height - 230) - gap : bottom;
    const available = Math.max(0, limit - top), rowGap = Math.min(gap, available / group.length);
    const natural = group.map(widget => heights[widget.id] ?? (widget.type === 'bar' ? 60 : 48));
    const sum = natural.reduce((a, b) => a + b, 0);
    const scale = Math.min(1, Math.max(0, available - rowGap * (group.length - 1)) / (sum || 1));
    const rows = natural.map(height => height * scale);
    let cursor = isBottom ? limit : top;
    if (!isTop && !isBottom) cursor = Math.max(top, (top + bottom - rows.reduce((sum, h) => sum + h + rowGap, -rowGap)) / 2);
    for (const [index, widget] of group.entries()) {
      const w = Math.min(widget.width, Math.max(0, right - left));
      const h = rows[index], remaining = rows.slice(index + 1).reduce((sum, h) => sum + h + rowGap, 0);
      let x = anchor.endsWith('left') || anchor === 'left' ? left : anchor.endsWith('right') || anchor === 'right' ? right - w : (left + right - w) / 2;
      x = Math.max(left, Math.min(right - w, x + widget.offset[0]));
      let y = isBottom ? cursor - h + Math.min(0, widget.offset[1]) : cursor + Math.max(0, widget.offset[1]);
      y = isBottom ? Math.max(top + remaining, y) : Math.min(limit - h - remaining, y);
      y = Math.max(top, Math.min(limit - h, y));
      positions.push({ id: widget.id, x, y, width: w, maxHeight: h });
      cursor = isBottom ? y - rowGap : y + h + rowGap;
    }
  }
  return positions;
}
