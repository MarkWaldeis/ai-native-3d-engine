export function qualitySettings(level, pixelRatio = 1) {
  const caps = [0.8, 1, 1.5];
  return { pixelRatio: Math.min(pixelRatio || 1, caps[level]), shadowSize: level === 2 ? 1024 : 512 };
}

export function isCollectible(entity, tag) {
  return entity.tags.some(value => tag ? value === tag : value === 'coin' || value === 'crystal');
}

export function nextObjective(entities, position, collectibleTag) {
  const collectibles = entities.filter(entity => isCollectible(entity, collectibleTag));
  const candidates = collectibles.length ? collectibles : entities.filter(entity => entity.tags.some(tag => tag === 'portal' || tag === 'goal'));
  const distance = entity => entity.position.reduce((sum, value, axis) => sum + (value-position[axis])**2, 0);
  return candidates.reduce((best, entity) => !best || distance(entity) < distance(best) ? entity : best, null);
}

export function objectiveBearing(x, y, behind = false) {
  const sign = behind ? -1 : 1;
  return Math.atan2(x * sign, y * sign);
}

export function feedbackEvents(previous, current) {
  if (!previous) return { collect: false, jump: false, win: false };
  const score = Number(current.score) || 0;
  return {
    collect: score > (Number(previous.score) || 0),
    jump: Number(current['Sprünge']) > (Number(previous['Sprünge']) || 0),
    win: (current.won === true || current.gewonnen === true) && !(previous.won || previous.gewonnen),
  };
}

export class GameAudio {
  constructor() { this.context = null; this.muted = false; }
  get muted() { return this._muted; }
  set muted(value) {
    this._muted = value;
    if (this.master) this.master.gain.setValueAtTime(value ? 0 : 1, this.context.currentTime);
  }
  unlock() {
    try {
      const Context = globalThis.AudioContext || globalThis.webkitAudioContext;
      if (!Context) return;
      if (!this.context) {
        this.context = new Context(); this.master = this.context.createGain();
        this.master.gain.setValueAtTime(this.muted ? 0 : 1, this.context.currentTime);
        this.master.connect(this.context.destination);
      }
      this.context.resume()?.catch(() => {});
    } catch { /* Sound is optional when audio is unavailable. */ }
  }
  suspend() { this.context?.suspend()?.catch(() => {}); }
  play(kind) {
    const context = this.context;
    if (!context || context.state !== 'running' || this.muted) return;
    const notes = kind === 'win' ? [523.25, 659.25, 783.99, 1046.5] : kind === 'collect' ? [880, 1318.5] : [220, 440];
    notes.forEach((frequency, i) => {
      const time = context.currentTime + i * (kind === 'win' ? 0.12 : 0.065);
      const oscillator = context.createOscillator(), gain = context.createGain();
      oscillator.type = 'sine'; oscillator.frequency.setValueAtTime(frequency, time);
      if (kind === 'jump') oscillator.frequency.exponentialRampToValueAtTime(frequency * 1.4, time + 0.1);
      gain.gain.setValueAtTime(0, time); gain.gain.linearRampToValueAtTime(0.055, time + 0.012);
      gain.gain.exponentialRampToValueAtTime(0.001, time + 0.22);
      oscillator.connect(gain); gain.connect(this.master);
      oscillator.start(time); oscillator.stop(time + 0.25);
      oscillator.onended = () => { oscillator.disconnect(); gain.disconnect(); };
    });
  }
}
