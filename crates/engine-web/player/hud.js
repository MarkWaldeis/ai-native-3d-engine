import { heading, minimapPoint } from './fps_core.js';

// DOM shooter HUD; reads only snapshot state/events, never game rules.
const el = (tag, className, parent, text) => { const node = document.createElement(tag); if (className) node.className = className; if (text) node.textContent = text; parent?.append(node); return node; };

export class ShooterHud {
  constructor(ui = {}) {
    this.root = el('div', 'fps-hud'); this.root.id = 'fps-hud';
    const canvas = document.getElementById('game'); if (canvas) canvas.after(this.root); else document.body.append(this.root);
    this.vignette = el('div', 'fps-vignette', this.root);
    this.crosshair = el('div', 'fps-crosshair', this.root);
    for (const side of ['t', 'b', 'l', 'r']) el('i', `fps-ch-${side}`, this.crosshair);
    this.hitmarker = el('div', 'fps-hitmarker', this.root);
    const top = el('div', 'fps-top', this.root);
    this.compass = el('div', 'fps-compass', top); this.strip = el('div', 'fps-compass-strip', this.compass);
    el('b', 'fps-compass-needle', this.compass);
    for (let deg = -360; deg <= 720; deg += 15) {
      const mark = el('span', deg % 90 === 0 ? 'major' : '', this.strip, { 0: 'N', 90: 'O', 180: 'S', 270: 'W' }[((deg % 360) + 360) % 360] || (deg % 45 === 0 ? String(((deg % 360) + 360) % 360) : '·'));
      mark.style.left = `${(deg + 360) * 4}px`;
    }
    this.objective = el('div', 'fps-objective', top);
    this.feed = el('div', 'fps-killfeed', this.root);
    this.minimap = el('canvas', 'fps-minimap', this.root); this.minimap.width = this.minimap.height = 168;
    const vitals = el('div', 'fps-vitals', this.root);
    this.healthLabel = el('span', 'fps-health-label', vitals, '100');
    const bar = el('div', 'fps-health', vitals); this.healthFill = el('i', '', bar);
    this.ammo = el('div', 'fps-ammo', this.root);
    this.ammoMag = el('b', '', this.ammo, '–'); this.ammoReserve = el('span', '', this.ammo, '');
    this.reloadHint = el('div', 'fps-reload', this.root, ui.reload_label || 'NACHLADEN [R]');
    this.dead = el('div', 'fps-dead', this.root, ui.death_text || 'GEFALLEN');
    this.hitTime = 1; this.damage = 0; this.spread = 0; this.maxHealth = null; this.entries = []; this.range = Number(ui.minimap_range) || 60;
    this.visible(false);
  }
  visible(on) { this.root.hidden = !on; }
  state(state) {
    const health = Number(state.health);
    if (Number.isFinite(health)) {
      if (this.maxHealth === null || health > this.maxHealth) this.maxHealth = Math.max(health, 1);
      this.healthFill.style.width = `${Math.max(0, Math.min(1, health / this.maxHealth)) * 100}%`;
      this.healthFill.classList.toggle('low', health / this.maxHealth < 0.3);
      this.healthLabel.textContent = Math.ceil(Math.max(0, health));
      this.dead.classList.toggle('show', health <= 0);
    }
    if (state.ammo !== undefined) {
      this.ammoMag.textContent = state.ammo; this.ammoReserve.textContent = state.ammo_reserve !== undefined ? ` / ${state.ammo_reserve}` : '';
      this.reloadHint.classList.toggle('show', Number(state.ammo) === 0 || Number(state._reload) > 0);
      this.reloadHint.textContent = Number(state._reload) > 0 ? 'LÄDT NACH …' : 'NACHLADEN [R]';
    }
    const objective = state.objective ?? '';
    const text = `${objective}${state.kills !== undefined ? ` · Abschüsse ${state.kills}` : ''}`;
    if (this.objective.textContent !== text) this.objective.textContent = text;
  }
  cues(cues) {
    if (cues.hits) { this.hitTime = 0; this.hitmarker.classList.toggle('kill', cues.killedTarget); }
    if (cues.damageTaken) this.damage = Math.min(1, this.damage + cues.damageTaken / 40);
    if (cues.shots) this.spread = Math.min(1, this.spread + 0.35 * cues.shots);
    for (const text of cues.kills) {
      const row = el('div', 'fps-kill', null, text); this.feed.prepend(row); this.entries.push({ row, age: 0 });
      while (this.entries.length > 5) this.entries.shift().row.remove();
    }
  }
  update(dt, look, moving) {
    this.hitTime += dt; this.damage = Math.max(0, this.damage - dt * 0.6); this.spread = Math.max(0, this.spread - dt * 3);
    this.hitmarker.style.opacity = this.hitTime < 0.18 ? String(1 - this.hitTime / 0.18) : '0';
    this.vignette.style.opacity = this.damage.toFixed(3);
    this.crosshair.style.setProperty('--gap', `${7 + this.spread * 12 + moving * 6}px`);
    this.strip.style.transform = `translateX(${-(heading(look.yaw) + 360) * 4}px)`;
    for (const entry of [...this.entries]) { entry.age += dt; if (entry.age > 6) { entry.row.remove(); this.entries.splice(this.entries.indexOf(entry), 1); } else entry.row.style.opacity = entry.age > 5 ? String(6 - entry.age) : '1'; }
  }
  drawMinimap(player, yaw, markers) {
    const ctx = this.minimap.getContext('2d'), size = this.minimap.width;
    ctx.clearRect(0, 0, size, size);
    ctx.fillStyle = 'rgba(10,20,16,0.55)'; ctx.beginPath(); ctx.arc(size / 2, size / 2, size / 2 - 2, 0, Math.PI * 2); ctx.fill();
    ctx.strokeStyle = 'rgba(255,255,255,0.35)'; ctx.lineWidth = 2; ctx.stroke();
    ctx.strokeStyle = 'rgba(255,255,255,0.12)'; ctx.beginPath(); ctx.arc(size / 2, size / 2, size / 4, 0, Math.PI * 2); ctx.stroke();
    if (!player) return;
    for (const marker of markers) {
      const point = minimapPoint(player, yaw, marker.position, this.range, size - 8); if (!point) continue;
      ctx.fillStyle = marker.color; ctx.beginPath(); ctx.arc(point.x + 4, point.y + 4, marker.radius || 3.5, 0, Math.PI * 2); ctx.fill();
    }
    ctx.fillStyle = '#fff'; ctx.beginPath(); ctx.moveTo(size / 2, size / 2 - 8); ctx.lineTo(size / 2 + 5, size / 2 + 6); ctx.lineTo(size / 2 - 5, size / 2 + 6); ctx.closePath(); ctx.fill();
  }
  reset() { this.entries.forEach(e => e.row.remove()); this.entries = []; this.damage = 0; this.hitTime = 1; this.maxHealth = null; }
}
