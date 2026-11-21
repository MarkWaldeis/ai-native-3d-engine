import { GameAudio } from './presentation.js';

export function audioObject(value) {
  try { const parsed = typeof value === 'string' ? JSON.parse(value) : value; return parsed && typeof parsed === 'object' ? parsed : {}; } catch { return {}; }
}
export function audioConfig(value) {
  return { master: 1, sfx: 1, music_volume: 1, max_voices: 24, stream_radius: 80, hysteresis: 8, memory_budget_mb: 32, stream_threshold_s: 20, ...audioObject(value) };
}
export function musicWhen(when, state) {
  if (!when || when === 'always') return true;
  const a = state[when.key], b = when.value;
  switch (when.op) {
    case '>=': return a >= b; case '>': return a > b; case '<=': return a <= b; case '<': return a < b;
    case '!=': return a !== b; case '==': case '===': return a === b; default: return false;
  }
}
const presets = { room: [0.7, 4, 0.5], hall: [2.8, 3, 0.7], cave: [4, 2, 0.8], outdoor: [0.25, 5, 0.3], underwater: [1.6, 4, 0.95] };
const position = entity => entity.transform?.position || entity.position || [0, 0, 0];
export function zoneWeight(zone, point) {
  const p = position(zone), scale = zone.transform?.scale || [1, 1, 1];
  const delta = point.map((v, i) => Math.abs(v - p[i]));
  const border = zone.shape === 'sphere'
    ? (zone.radius ?? 1) * Math.max(...scale.map(Math.abs)) - Math.hypot(...delta)
    : Math.min(...delta.map((v, i) => Math.abs((zone.size || [1, 1, 1])[i] * scale[i]) / 2 - v));
  return Math.max(0, Math.min(1, border / Math.max(0.001, zone.fade ?? 4))) * Math.max(0, Math.min(1, zone.wet ?? 0.5));
}
function param(node, key, value, time) { node[key]?.setValueAtTime(value, time); }
function locate(node, p, time) {
  if (node.positionX) p.forEach((v, i) => param(node, `position${'XYZ'[i]}`, v, time));
  else node.setPosition?.(...p);
}

export class SpatialAudio {
  constructor(config) {
    this.config = audioConfig(config); this.feedback = new GameAudio(); this.entities = new Map(); this.voices = new Map();
    this.buffers = new Map(); this.pending = new Map(); this.failures = new Set(); this.reverbs = new Map(); this.layers = [];
    this.state = {}; this.zone = null; this.clock = 0; this._muted = false; this.streams = new Set();
    this.mediaPool = []; this.mediaEntries = new Set();
  }
  get muted() { return this._muted; }
  set muted(value) { this._muted = value; if (this.master) param(this.master, 'gain', value ? 0 : this.config.master, this.context.currentTime); }
  configure(scene) {
    this.clear();
    this.config = audioConfig(scene.entities?.find(e => e.tags?.includes('player'))?.props?.ai_audio);
    for (const entity of scene.entities || []) this.entities.set(entity.id, { ...entity, since: this.clock });
    this.layers = (this.config.music?.layers || []).map((layer, i) => ({ ...layer, id: `music:${i}`, layerId: layer.id, since: this.clock }));
    if (this.master) { param(this.master, 'gain', this.muted ? 0 : this.config.master, this.clock); param(this.sfx, 'gain', this.config.sfx, this.clock); }
  }
  unlock() {
    try {
      if (!this.context) {
        this.feedback.unlock(); this.context = this.feedback.context;
        if (!this.context) return;
        const c = this.context;
        this.master = c.createGain(); this.master.connect(c.destination);
        this.sfx = c.createGain(); this.sfx.connect(this.master);
        this.dry = c.createGain(); this.dry.connect(this.sfx);
        this.feedback.master.disconnect(); this.feedback.master.connect(this.sfx);
        param(this.master, 'gain', this.muted ? 0 : this.config.master, c.currentTime);
        param(this.sfx, 'gain', this.config.sfx, c.currentTime);
        this.musicStart = c.currentTime + 0.1;
      }
      this.context.resume()?.catch(() => {});
      const poolSize = Math.min(8, this.layers.filter(layer => layer.src).length + Math.min(4, this.config.max_voices));
      while (this.mediaEntries.size < poolSize && globalThis.Audio) {
        const entry = { media: new Audio() }; entry.media.playsInline = true;
        entry.media.src = 'data:audio/wav;base64,UklGRiYAAABXQVZFZm10IBAAAAABAAEARKwAAIhYAQACABAAZGF0YQIAAAAAAA==';
        this.mediaEntries.add(entry); this.mediaPool.push(entry);
      }
      for (const entry of this.mediaPool) { entry.media.play()?.catch(() => {}); entry.media.pause(); }
      for (const voice of this.voices.values()) voice.media?.play()?.catch(() => {});
    } catch { /* Audio is optional. */ }
  }
  suspend() { for (const voice of this.voices.values()) { clearTimeout(voice.timer); voice.media?.pause(); } this.context?.suspend()?.catch(() => {}); }
  play(kind) { this.feedback.play(kind); }
  apply(result) {
    this.state = result.state || {};
    for (const id of result.removed || []) { this.entities.delete(id); this.stop(id); }
    for (const e of result.spawned || []) this.entities.set(e.id, { ...e, since: this.clock });
    for (const e of result.transforms || []) { const old = this.entities.get(e.id); if (old) old.transform = e.transform; }
  }
  clear() { for (const id of this.voices.keys()) this.stop(id); this.entities.clear(); this.layers = []; this.state = {}; this.zone = null; this.virtual = 0; this.activeLayers = []; this.musicReady = false; this.musicLoading = false; this.generation = (this.generation || 0) + 1; this.musicStart = this.context ? this.context.currentTime + 0.1 : 0; }
  warn(src, error) { if (!this.failures.has(src)) { this.failures.add(src); console.warn(`Audio ${src}: ${error.message || error}`); } }
  evict() {
    const budget = Math.max(0, this.config.memory_budget_mb) * 1024 * 1024;
    for (const [key, entry] of this.buffers) {
      if (this.bufferBytes() <= budget) break;
      if (![...this.voices.values()].some(v => v.buffer === entry.buffer)) this.buffers.delete(key);
    }
  }
  bufferBytes() { return [...this.buffers.values()].reduce((n, e) => n + e.bytes, 0); }
  async resource(sound) {
    if (sound.stream || this.streams.has(sound.src)) return { stream: true };
    if (this.buffers.has(sound.src)) { const e = this.buffers.get(sound.src); this.buffers.delete(sound.src); this.buffers.set(sound.src, e); return e; }
    if (!this.pending.has(sound.src)) {
      const task = (async () => {
        const media = new Audio(); media.preload = 'metadata';
        const duration = await new Promise((resolve, reject) => {
          media.onloadedmetadata = () => resolve(media.duration); media.onerror = () => reject(new Error('metadata load failed')); media.src = sound.src;
        }).finally(() => { media.removeAttribute('src'); media.load(); });
        if (!Number.isFinite(duration) || duration > this.config.stream_threshold_s) { this.streams.add(sound.src); return { stream: true }; }
        const response = await fetch(sound.src); if (!response.ok) throw new Error(`HTTP ${response.status}`);
        const buffer = await this.context.decodeAudioData(await response.arrayBuffer());
        const entry = { buffer, bytes: buffer.length * buffer.numberOfChannels * 4 };
        this.buffers.set(sound.src, entry); this.evict(); return entry;
      })().catch(error => { this.warn(sound.src, error); return null; }).finally(() => this.pending.delete(sound.src));
      this.pending.set(sound.src, task);
    }
    return this.pending.get(sound.src);
  }
  synth(kind) {
    const c = this.context;
    if (kind === 'hum' || kind === 'pulse') {
      const oscillator = c.createOscillator(); oscillator.type = kind === 'pulse' ? 'triangle' : 'sine'; oscillator.frequency.value = kind === 'pulse' ? 110 : 60; return oscillator;
    }
    const buffer = c.createBuffer(1, c.sampleRate * 2, c.sampleRate), data = buffer.getChannelData(0);
    let smooth = 0;
    for (let i = 0; i < data.length; i++) { smooth = smooth * 0.97 + (Math.random() * 2 - 1) * 0.03; data[i] = kind === 'fire' ? (Math.random() < 0.002 ? Math.random() : smooth) : smooth * (kind === 'water' ? 2 : 1); }
    const node = c.createBufferSource(); node.buffer = buffer; node.loop = true; return node;
  }
  stop(id) {
    const v = this.voices.get(id); if (!v) return;
    this.voices.delete(id); v.cancelled = true;
    clearTimeout(v.timer);
    if (v.media) {
      v.media.onloadedmetadata = v.media.onerror = v.media.onended = null;
      v.media.pause(); v.media.removeAttribute('src'); v.media.load();
      if (this.mediaEntries.has(v.mediaEntry)) this.mediaPool.push(v.mediaEntry);
    }
    try { v.source?.stop(); } catch { /* Already stopped. */ }
    for (const node of [v.source, v.gain, v.panner]) node?.disconnect();
    this.evict();
  }
  async start(entity, sound, music) {
    const v = { cancelled: false }; this.voices.set(entity.id, v);
    try {
      const resource = sound.synth ? {} : music && entity.resource ? entity.resource : await this.resource(sound);
      if (v.cancelled || !resource) { if (!v.cancelled) this.stop(entity.id); return; }
      const c = this.context; v.gain = c.createGain(); v.gain.gain.value = 0;
      if (!music) {
        v.panner = c.createPanner(); Object.assign(v.panner, { panningModel: 'HRTF', distanceModel: 'inverse', refDistance: sound.ref_distance ?? 2, maxDistance: sound.max_distance ?? 60, rolloffFactor: sound.rolloff ?? 1,
          coneInnerAngle: sound.cone?.inner ?? 360, coneOuterAngle: sound.cone?.outer ?? 360, coneOuterGain: sound.cone?.outer_gain ?? 0 });
        v.gain.connect(v.panner); v.panner.connect(this.dry);
        for (const reverb of this.reverbs.values()) v.panner.connect(reverb.input);
      } else v.gain.connect(this.master);
      if (sound.synth) v.source = this.synth(sound.synth);
      else if (resource.stream) {
        v.mediaEntry = this.mediaPool.pop() || { media: new Audio() };
        v.media = v.mediaEntry.media; v.media.loop = sound.loop !== false;
        v.source = v.mediaEntry.source ||= c.createMediaElementSource(v.media);
      } else {
        v.source = c.createBufferSource(); v.buffer = resource.buffer; v.source.buffer = v.buffer; v.source.loop = sound.loop !== false;
        if (!this.buffers.has(sound.src)) this.buffers.set(sound.src, resource);
        this.evict();
      }
      if (music) entity.resource = null;
      v.source.connect(v.gain);
      if (sound.loop === false) {
        const ended = () => { if (!v.cancelled) { entity.finished = true; this.stop(entity.id); } };
        if (v.media) v.media.onended = ended; else v.source.onended = ended;
      }
      const elapsed = Math.max(0, c.currentTime - (music ? this.musicStart : entity.since));
      if (v.media) {
        const seek = () => { const offset = Math.max(0, c.currentTime - (music ? this.musicStart : entity.since)); if (!v.cancelled && Number.isFinite(v.media.duration)) v.media.currentTime = v.media.loop ? offset % v.media.duration : Math.min(offset, v.media.duration); };
        v.media.onloadedmetadata = seek;
        v.media.onerror = () => { if (!v.cancelled) { this.warn(sound.src, new Error('stream load failed')); this.stop(entity.id); } };
        v.media.src = sound.src;
        const play = () => { if (!v.cancelled) { seek(); v.media.play()?.catch(error => this.warn(sound.src, error)); } };
        if (music && this.musicStart > c.currentTime) v.timer = setTimeout(() => { if (c.state === 'running') play(); }, (this.musicStart - c.currentTime) * 1000); else if (c.state === 'running') play();
      } else {
        const duration = v.buffer?.duration;
        if (duration && sound.loop === false && elapsed >= duration) { entity.finished = true; this.stop(entity.id); return; }
        v.source.start(music ? Math.max(c.currentTime, this.musicStart) : c.currentTime, duration ? elapsed % duration : 0);
      }
    } catch (error) { this.warn(sound.src || sound.synth, error); this.stop(entity.id); }
  }
  reverb(preset) {
    preset = presets[preset] ? preset : 'room';
    if (this.reverbs.has(preset)) return this.reverbs.get(preset);
    const c = this.context, [seconds, decay, damping] = presets[preset];
    const buffer = c.createBuffer(2, Math.ceil(c.sampleRate * seconds), c.sampleRate);
    for (let channel = 0; channel < 2; channel++) {
      const data = buffer.getChannelData(channel); let smooth = 0;
      for (let i = 0; i < data.length; i++) { smooth = smooth * damping + (Math.random() * 2 - 1) * (1 - damping); data[i] = smooth * (1 - i / data.length) ** decay; }
    }
    const input = c.createConvolver(), gain = c.createGain(); input.buffer = buffer; gain.gain.value = 0; input.connect(gain); gain.connect(this.sfx);
    const entry = { input, gain }; this.reverbs.set(preset, entry);
    for (const voice of this.voices.values()) voice.panner?.connect(input);
    return entry;
  }
  update(listener) {
    const c = this.context; if (!c || c.state !== 'running') return;
    this.clock = c.currentTime;
    const p = listener.position, f = listener.forward || [0, 0, -1], up = listener.up || [0, 1, 0];
    locate(c.listener, p, this.clock);
    if (c.listener.forwardX) { f.forEach((v, i) => param(c.listener, `forward${'XYZ'[i]}`, v, this.clock)); up.forEach((v, i) => param(c.listener, `up${'XYZ'[i]}`, v, this.clock)); }
    else c.listener.setOrientation?.(...f, ...up);
    const zones = [...this.entities.values()].filter(e => e.props?.ai_reverb_zone).map(e => ({ ...audioObject(e.props.ai_reverb_zone), transform: e.transform, name: e.name }));
    zones.push(...(this.config.reverb_zones || []).map(audioObject));
    const dominant = zones.map(z => ({ ...z, weight: zoneWeight(z, p) })).sort((a, b) => b.weight - a.weight)[0];
    this.zone = dominant?.weight > 0 ? dominant : null;
    if (this.zone) this.reverb(this.zone.preset);
    for (const [preset, r] of this.reverbs) r.gain.gain.setTargetAtTime(this.zone && (presets[this.zone.preset] ? this.zone.preset : 'room') === preset ? this.zone.weight : 0, this.clock, 0.15);
    this.dry.gain.setTargetAtTime(1 - (this.zone?.weight || 0) * 0.5, this.clock, 0.15);
    const candidates = [...this.entities.values()].filter(e => e.props?.ai_sound && !e.finished).map(e => ({ entity: e, sound: { loop: true, autoplay: true, ...audioObject(e.props.ai_sound) }, distance: Math.hypot(...position(e).map((v, i) => v - p[i])) }));
    this.virtual = candidates.filter(e => e.sound.autoplay && (e.sound.src || e.sound.synth)).length;
    const selected = candidates.filter(e => e.sound.autoplay && (e.sound.src || e.sound.synth) && e.distance <= this.config.stream_radius + (this.voices.has(e.entity.id) ? this.config.hysteresis : 0)).sort((a, b) => a.distance - b.distance).slice(0, Math.max(0, this.config.max_voices));
    const wanted = new Set([...selected.map(e => e.entity.id), ...this.layers.map(e => e.id)]);
    for (const id of this.voices.keys()) if (!wanted.has(id)) this.stop(id);
    for (const { entity, sound } of selected) {
      if (!this.voices.has(entity.id) && !this.failures.has(sound.src || sound.synth)) this.start(entity, sound, false);
      const v = this.voices.get(entity.id);
      if (v?.panner) {
        locate(v.panner, position(entity), this.clock);
        const rotation = entity.transform?.rotation_deg || [0, 0, 0], yaw = rotation[1] * Math.PI / 180, pitch = rotation[0] * Math.PI / 180;
        const direction = [Math.sin(yaw) * Math.cos(pitch), -Math.sin(pitch), Math.cos(yaw) * Math.cos(pitch)];
        if (v.panner.orientationX) direction.forEach((n, i) => param(v.panner, `orientation${'XYZ'[i]}`, n, this.clock)); else v.panner.setOrientation?.(...direction);
        v.gain.gain.setTargetAtTime(sound.volume ?? 1, this.clock, 0.05);
      }
    }
    this.activeLayers = [];
    if (this.layers.length && !this.musicReady && !this.musicLoading) {
      this.musicLoading = true; const generation = this.generation;
      Promise.all(this.layers.map(layer => layer.src ? this.resource(layer) : null)).then(resources => {
        if (generation !== this.generation) return;
        this.layers.forEach((layer, i) => { layer.resource = resources[i]; });
        this.musicStart = c.currentTime + 0.1; this.musicReady = true; this.musicLoading = false;
      });
    }
    for (const layer of this.layers) {
      if (this.musicReady && !this.voices.has(layer.id) && (layer.src || layer.synth) && !this.failures.has(layer.src || layer.synth)) this.start(layer, { ...layer, loop: true }, true);
      const active = musicWhen(layer.when, this.state); if (active) this.activeLayers.push(layer.layerId);
      this.voices.get(layer.id)?.gain?.gain.setTargetAtTime(active ? (layer.volume ?? 1) * this.config.music_volume : 0, this.clock, Math.max(0.001, (this.config.music?.crossfade_s ?? 2) / 3));
    }
  }
  stats() {
    const voices = [...this.voices.values()].filter(v => v.source);
    const spatial = voices.filter(v => v.panner).length;
    return { audioVoices: voices.length, audioVirtualVoices: Math.max(0, (this.virtual || 0) - spatial), audioBufferBytes: this.bufferBytes(), audioStreams: voices.filter(v => v.media).length, audioReverbZone: this.zone?.name || this.zone?.preset || null, musicLayers: this.activeLayers || [] };
  }
}
