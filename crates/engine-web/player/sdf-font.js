const INF = 1e20;

export function edt(values, width, height) {
  const result = Float64Array.from(values, value => Number.isFinite(value) ? value : INF), n = Math.max(width, height);
  const f = new Float64Array(n), d = new Float64Array(n), v = new Int32Array(n), z = new Float64Array(n + 1);
  const line = length => {
    let k = 0; v[0] = 0; z[0] = -Infinity; z[1] = Infinity;
    for (let q = 1; q < length; q++) {
      let s = ((f[q] + q * q) - (f[v[k]] + v[k] * v[k])) / (2 * (q - v[k]));
      while (s <= z[k]) { k--; s = ((f[q] + q * q) - (f[v[k]] + v[k] * v[k])) / (2 * (q - v[k])); }
      v[++k] = q; z[k] = s; z[k + 1] = Infinity;
    }
    k = 0;
    for (let q = 0; q < length; q++) { while (z[k + 1] < q) k++; d[q] = (q - v[k]) ** 2 + f[v[k]]; }
  };
  for (let x = 0; x < width; x++) {
    for (let y = 0; y < height; y++) f[y] = result[y * width + x];
    line(height);
    for (let y = 0; y < height; y++) result[y * width + x] = d[y];
  }
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) f[x] = result[y * width + x];
    line(width);
    for (let x = 0; x < width; x++) result[y * width + x] = d[x];
  }
  return result;
}

export function glyphSdf(alphaBytes, width, height, radius = 8) {
  const outer = new Float64Array(width * height), inner = new Float64Array(width * height);
  for (let i = 0; i < outer.length; i++) {
    const a = alphaBytes[i] / 255;
    outer[i] = a === 0 ? INF : Math.max(0, 0.5 - a) ** 2;
    inner[i] = a === 1 ? INF : Math.max(0, a - 0.5) ** 2;
  }
  const outside = edt(outer, width, height), inside = edt(inner, width, height);
  return Uint8Array.from(outside, (value, i) => Math.max(0, Math.min(255, Math.round(128 + (Math.sqrt(inside[i]) - Math.sqrt(value)) * 128 / radius))));
}

export class ShelfPacker {
  constructor(width = 1024, height = 1024) { this.width = width; this.height = height; this.x = 0; this.y = 0; this.row = 0; }
  pack(width, height) {
    if (!(width > 0 && height > 0) || width > this.width || height > this.height) return null;
    const newline = this.x + width > this.width;
    const x = newline ? 0 : this.x, y = newline ? this.y + this.row : this.y;
    if (y + height > this.height) return null;
    this.x = x + width; this.y = y; this.row = newline ? height : Math.max(this.row, height);
    return { x, y, width, height };
  }
}

export function layoutText(text, metricsFn, { size = 0.5, maxWidth = 0, align = 'center', lineHeight = 1.2 } = {}) {
  const lines = [[]]; let advance = 0;
  const add = word => {
    const glyphs = Array.from(word, char => ({ char, ...metricsFn(char) }));
    const width = glyphs.reduce((sum, g) => sum + g.advance * size, 0);
    if (maxWidth > 0 && advance > 0 && advance + width > maxWidth && word.trim()) { lines.push([]); advance = 0; }
    for (const glyph of glyphs) {
      if (!advance && glyph.char === ' ') continue;
      lines.at(-1).push({ ...glyph, pen: advance }); advance += glyph.advance * size;
    }
  };
  for (const token of String(text).split(/(\n| +)/)) {
    if (token === '\n') { lines.push([]); advance = 0; } else if (token) add(token);
  }
  const quads = [];
  lines.forEach((line, index) => {
    const last = line.findLast(g => g.char !== ' ');
    const width = last ? last.pen + last.advance * size : 0;
    const origin = align === 'left' ? 0 : align === 'right' ? -width : -width / 2;
    for (const glyph of line) if (glyph.width > 0 && glyph.height > 0) quads.push({ ...glyph,
      x: origin + glyph.pen + glyph.left * size,
      y: ((lines.length - 1) / 2 - index) * lineHeight * size + (glyph.top - glyph.height - 0.35) * size,
      width: glyph.width * size, height: glyph.height * size,
    });
  });
  return quads;
}

export function parseTextConfig(value) {
  const config = { text: '', size: 0.5, color: '#fff', outline_color: '#000', outline_width: 0.15, offset: null, billboard: true, align: 'center', max_width: 0, depth_test: true, fade_distance: 0 };
  const warnings = [], warn = key => warnings.push(`Text: ungültiges Feld ${key}.`);
  if (value === undefined || value === null) return { config, warnings };
  try {
    if (typeof value === 'string') value = { text: value };
    if (typeof value !== 'object' || Array.isArray(value)) { warn('ai_text'); return { config, warnings }; }
    const number = v => typeof v === 'number' && Number.isFinite(v);
    const color = v => typeof v === 'string' && /^(#[\da-f]{3,8}|[a-z]+|rgba?\([\d.,%\s]+\))$/i.test(v);
    const valid = { text: v => typeof v === 'string', size: v => number(v) && v > 0,
      color, outline_color: color, outline_width: v => number(v) && v >= 0 && v <= 0.4,
      offset: v => Array.isArray(v) && v.length === 3 && v.every(number), billboard: v => typeof v === 'boolean',
      align: v => ['center', 'left', 'right'].includes(v), max_width: v => number(v) && v >= 0,
      depth_test: v => typeof v === 'boolean', fade_distance: v => number(v) && v >= 0 };
    for (const [key, validate] of Object.entries(valid)) if (value[key] !== undefined) {
      if (validate(value[key])) config[key] = Array.isArray(value[key]) ? [...value[key]] : value[key]; else warn(key);
    }
  } catch { warn('ai_text'); }
  return { config, warnings };
}
