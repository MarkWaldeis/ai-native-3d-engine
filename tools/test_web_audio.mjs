import test from 'node:test';
import assert from 'node:assert/strict';
import { SpatialAudio, audioConfig, musicWhen, zoneWeight } from '../crates/engine-web/player/audio.js';

class Parameter {
  value = 0;
  setValueAtTime(v) { this.value = v; }
  setTargetAtTime(v, time, constant) { this.value = v; this.target = [v, time, constant]; }
  linearRampToValueAtTime() {}
  exponentialRampToValueAtTime() {}
}
class Node {
  constructor() {
    for (const name of ['gain', 'frequency', ...['position', 'orientation', 'forward', 'up'].flatMap(p => [...'XYZ'].map(axis => p + axis))]) this[name] = new Parameter();
    this.connections = [];
  }
  connect(node) { this.connections.push(node); }
  disconnect() { this.connections = []; }
  start(...args) { this.started = args; }
  stop() { this.stopped = true; }
}
class Context {
  currentTime = 0; sampleRate = 100; state = 'suspended'; listener = new Node(); destination = new Node();
  createGain() { return new Node(); }
  createPanner() { return new Node(); }
  createOscillator() { return new Node(); }
  createBufferSource() { return new Node(); }
  createConvolver() { const n = new Node(); n.convolver = true; return n; }
  createMediaElementSource(media) { const n = new Node(); n.media = media; return n; }
  createBuffer(channels, length, rate) { return { numberOfChannels: channels, length, duration: length / rate, getChannelData: () => new Float32Array(length) }; }
  decodeAudioData() { this.decodes = (this.decodes || 0) + 1; return Promise.resolve(this.createBuffer(2, 100, 100)); }
  resume() { this.state = 'running'; return Promise.resolve(); }
  suspend() { this.state = 'suspended'; return Promise.resolve(); }
}
class Media {
  static duration = 2;
  duration = Media.duration; currentTime = 0;
  set src(v) { this.url = v; queueMicrotask(() => this.onloadedmetadata?.()); }
  removeAttribute() {}
  load() {}
  play() { this.playing = true; return Promise.resolve(); }
  pause() { this.playing = false; }
}
const listen = (x = 0) => ({ position: [x, 0, 0], forward: [1, 0, 0], up: [0, 1, 0] });
const entity = (id, x, sound = { synth: 'hum' }) => ({ id, tags: [], transform: { position: [x, 0, 0], scale: [1, 1, 1], rotation_deg: [0, 90, 0] }, props: { ai_sound: sound } });
const scene = (entities, config = {}) => ({ entities: [{ id: 'player', tags: ['player'], props: { ai_audio: config } }, ...entities] });
const settle = () => new Promise(resolve => setImmediate(resolve));
async function mock(run) {
  const saved = { AudioContext: globalThis.AudioContext, Audio: globalThis.Audio, fetch: globalThis.fetch };
  globalThis.AudioContext = Context; globalThis.Audio = Media;
  globalThis.fetch = async () => ({ ok: true, arrayBuffer: async () => new ArrayBuffer(8) });
  Media.duration = 2;
  try { await run(); } finally { Object.assign(globalThis, saved); }
}

test('audio props accept objects/JSON, malformed and absent props have defaults', () => {
  assert.equal(audioConfig().max_voices, 24); assert.equal(audioConfig().stream_radius, 80);
  assert.equal(audioConfig().hysteresis, 8); assert.equal(audioConfig().memory_budget_mb, 32);
  assert.equal(audioConfig().stream_threshold_s, 20);
  assert.equal(audioConfig({ master: 0.3 }).master, 0.3);
  assert.equal(audioConfig('{"sfx":0.2}').sfx, 0.2);
  assert.deepEqual(audioConfig('{bad'), audioConfig());
  assert.deepEqual(audioConfig('null'), audioConfig());
});

test('gesture-lazy HRTF emitters, modern listener, transforms/spawns/removal and mute/SFX bus', () => mock(async () => {
  const audio = new SpatialAudio(); audio.configure(scene([entity(1, 2, '{"synth":"wind","cone":{"inner":90,"outer":180,"outer_gain":0.2}}')]));
  audio.update(listen()); assert.equal(audio.context, undefined);
  audio.unlock(); audio.update(listen());
  const v = audio.voices.get(1);
  assert.equal(v.panner.panningModel, 'HRTF'); assert.equal(v.panner.distanceModel, 'inverse');
  assert.equal(v.panner.refDistance, 2); assert.equal(v.panner.coneInnerAngle, 90);
  assert.equal(v.panner.orientationX.value, 1); assert.equal(audio.context.listener.forwardX.value, 1);
  assert.equal(v.panner.positionX.value, 2); assert.ok(audio.feedback.master.connections.includes(audio.sfx));
  audio.apply({ transforms: [{ id: 1, transform: { position: [4, 0, 0] } }], spawned: [entity(2, 1)] }); audio.update(listen(1));
  assert.equal(v.panner.positionX.value, 4); assert.equal(audio.context.listener.positionX.value, 1);
  assert.ok(audio.voices.has(2)); audio.apply({ removed: [1] }); assert.ok(v.source.stopped); assert.ok(!audio.voices.has(1));
  audio.muted = true; assert.equal(audio.master.gain.value, 0); audio.muted = false; assert.equal(audio.master.gain.value, 1);
  audio.clear(); assert.equal(audio.voices.size, 0);
}));

test('Safari listener and panner position/orientation fallbacks', () => mock(async () => {
  const audio = new SpatialAudio(); audio.unlock(); const listener = audio.context.listener;
  for (const key of Object.keys(listener)) if (/^(position|forward|up)/.test(key)) delete listener[key];
  listener.setPosition = (...p) => { listener.p = p; }; listener.setOrientation = (...v) => { listener.o = v; };
  audio.update(listen(4)); assert.deepEqual(listener.p, [4, 0, 0]); assert.deepEqual(listener.o, [1, 0, 0, 0, 1, 0]);
}));

test('radius hysteresis, nearest voice cap and virtual playback phase', () => mock(async () => {
  const audio = new SpatialAudio(); audio.configure(scene([entity(1, 9), entity(2, 3), entity(3, 30)], { max_voices: 1, stream_radius: 10, hysteresis: 2 }));
  audio.unlock(); audio.update(listen()); assert.deepEqual([...audio.voices.keys()], [2]); assert.equal(audio.stats().audioVirtualVoices, 2);
  audio.apply({ removed: [2] }); audio.update(listen()); assert.ok(audio.voices.has(1));
  audio.update(listen(-2)); assert.ok(audio.voices.has(1)); audio.update(listen(-4)); assert.equal(audio.voices.size, 0);
  audio.context.currentTime = 7; audio.update(listen()); assert.equal(audio.voices.get(1).source.started[0], 7);
  audio.clear();
}));

test('decoded cache deduplicates fetches, touches LRU and evicts unused bytes under budget', () => mock(async () => {
  const audio = new SpatialAudio({ memory_budget_mb: 1600 / 1048576 }); audio.unlock();
  const [a, same] = await Promise.all([audio.resource({ src: 'a' }), audio.resource({ src: 'a' })]);
  assert.equal(a, same); assert.equal(audio.context.decodes, 1);
  await audio.resource({ src: 'b' }); await audio.resource({ src: 'a' }); await audio.resource({ src: 'c' });
  assert.deepEqual([...audio.buffers.keys()], ['a', 'c']); assert.equal(audio.stats().audioBufferBytes, 1600);
  audio.config.memory_budget_mb = 0; audio.evict(); assert.equal(audio.bufferBytes(), 0);
}));

test('failed nearest emitters cannot starve playable voices or keep a slot', () => mock(async () => {
  const audio = new SpatialAudio();
  audio.configure(scene([entity(1, 1, { src: 'bad', stream: true }), entity(2, 3)], { max_voices: 1 }));
  audio.unlock(); audio.update(listen()); await settle();
  const failed = audio.voices.get(1); assert.ok(failed.media);
  audio.failures.add('bad'); audio.update(listen());
  assert.equal(audio.voices.has(1), false); assert.equal(failed.media.playing, false);
  assert.equal(audio.voices.get(2).source.frequency.value, 60);
  assert.equal(audio.stats().audioVirtualVoices, 0);
  audio.clear();
}));

test('zones fade at borders, scale entity volumes and choose highest-weight preset lazily', () => mock(async () => {
  const zone = { shape: 'box', position: [0, 0, 0], size: [20, 20, 20], wet: 1, fade: 4 };
  assert.equal(zoneWeight(zone, [10, 0, 0]), 0); assert.equal(zoneWeight(zone, [8, 0, 0]), 0.5);
  assert.equal(zoneWeight({ shape: 'sphere', radius: 5, wet: 1, fade: 2 }, [4, 0, 0]), 0.5);
  assert.equal(zoneWeight({ ...zone, transform: { position: [0, 0, 0], scale: [2, 2, 2] } }, [10, 0, 0]), 1);
  const audio = new SpatialAudio(); audio.configure(scene([], { reverb_zones: [{ ...zone, preset: 'cave', name: 'Cave' }, { ...zone, preset: 'room', wet: 0.1 }] })); audio.unlock();
  assert.equal(audio.reverbs.size, 0); audio.update(listen());
  assert.equal(audio.stats().audioReverbZone, 'Cave'); assert.equal(audio.reverbs.get('cave').input.buffer.duration, 4);
  assert.equal(audio.reverbs.get('cave').gain.gain.target[0], 1);
  audio.update(listen(20)); assert.equal(audio.stats().audioReverbZone, null); assert.equal(audio.reverbs.get('cave').gain.gain.target[0], 0);
}));

test('decoded virtual voices retain phase, active buffers are pinned, one-shots end once', () => mock(async () => {
  const audio = new SpatialAudio(); audio.configure(scene([entity(1, 20, { src: 'loop' }), entity(2, 1, { src: 'one', loop: false })], { stream_radius: 10, memory_budget_mb: 0 }));
  audio.unlock(); audio.update(listen()); await settle(); audio.update(listen());
  assert.equal(audio.bufferBytes(), 800);
  const shot = audio.voices.get(2); shot.source.onended(); audio.update(listen());
  assert.equal(audio.voices.has(2), false); assert.equal(audio.bufferBytes(), 0);
  audio.context.currentTime = 7.5; audio.update(listen(20)); await settle(); audio.update(listen(20));
  assert.deepEqual(audio.voices.get(1).source.started, [7.5, 0.5]);
  audio.update(listen(40)); assert.equal(audio.bufferBytes(), 0); audio.clear();
}));

test('entity reverb JSON follows spawned, moved and removed transforms', () => mock(async () => {
  const audio = new SpatialAudio(); audio.configure(scene([])); audio.unlock();
  audio.apply({ spawned: [{ id: 4, name: 'Hall', props: { ai_reverb_zone: '{"shape":"box","size":[20,20,20],"preset":"hall"}' }, transform: { position: [0, 0, 0], scale: [1, 1, 1] } }] });
  audio.update(listen()); assert.equal(audio.stats().audioReverbZone, 'Hall');
  audio.apply({ transforms: [{ id: 4, transform: { position: [40, 0, 0], scale: [1, 1, 1] } }] }); audio.update(listen()); assert.equal(audio.stats().audioReverbZone, null);
  audio.apply({ removed: [4] }); audio.update(listen(40)); assert.equal(audio.stats().audioReverbZone, null);
}));

test('initial full spawn listing removes on-start ghost emitters/zones but later deltas do not', () => mock(async () => {
  const kept = entity(1, 0), inactive = entity(2, 4);
  const ghost = { ...entity(3, 0), name: 'ghost', props: { ai_sound: { synth: 'hum' }, ai_reverb_zone: { size: [20, 20, 20], preset: 'cave' } } };
  const audio = new SpatialAudio(); audio.configure(scene([kept, inactive, ghost]));
  audio.apply({ spawned: [kept, inactive], transforms: [kept], removed: [] });
  assert.equal(audio.entities.has(3), false); assert.equal(audio.entities.has(2), true);
  audio.unlock(); audio.update(listen());
  assert.equal(audio.voices.has(3), false); assert.equal(audio.stats().audioReverbZone, null);
  audio.apply({ spawned: [], transforms: [], removed: [] }); audio.update(listen());
  assert.equal(audio.entities.has(1), true); assert.equal(audio.entities.has(2), true);
  audio.configure(scene([ghost])); audio.apply({ spawned: [], transforms: [], removed: [] }); audio.update(listen());
  assert.equal(audio.entities.size, 0); assert.equal(audio.voices.size, 0); assert.equal(audio.stats().audioReverbZone, null);
}));

test('declarative music conditions crossfade and layers share the same start time', () => mock(async () => {
  assert.equal(musicWhen('always', {}), true); assert.equal(musicWhen({ key: 'combat', op: '>=', value: 1 }, {}), false);
  assert.equal(musicWhen({ key: 'combat', op: 'eval', value: 1 }, { combat: 1 }), false);
  const audio = new SpatialAudio(); audio.configure(scene([], { music_volume: 0.5, music: { crossfade_s: 3, layers: [{ id: 'base', synth: 'pulse' }, { id: 'battle', synth: 'hum', volume: 0.8, when: { key: 'combat', op: '>=', value: 1 } }] } }));
  audio.unlock(); audio.update(listen()); await settle(); audio.update(listen());
  const base = audio.voices.get('music:0'), battle = audio.voices.get('music:1');
  assert.equal(base.source.started[0], battle.source.started[0]); assert.deepEqual(battle.gain.gain.target, [0, 0, 1]);
  audio.apply({ state: { combat: 1 } }); audio.update(listen()); assert.equal(battle.gain.gain.target[0], 0.4);
  assert.deepEqual(audio.stats().musicLayers, ['base', 'battle']); audio.clear();
}));

test('long and explicit streams bypass decode; media lifecycle suspends and destroys', () => mock(async () => {
  const audio = new SpatialAudio(); audio.unlock(); Media.duration = 30;
  assert.deepEqual(await audio.resource({ src: 'long' }), { stream: true }); assert.equal(audio.context.decodes, undefined);
  Media.duration = 2; assert.ok((await audio.resource({ src: 'short' })).buffer);
  audio.configure(scene([entity(1, 0, { src: 'forced', stream: true })])); audio.update(listen()); await settle(); audio.update(listen());
  const media = audio.voices.get(1).media; assert.equal(audio.stats().audioStreams, 1);
  audio.suspend(); assert.equal(media.playing, false); audio.unlock(); assert.equal(media.playing, true);
  audio.apply({ removed: [1] }); assert.equal(media.playing, false); assert.equal(audio.stats().audioStreams, 0);
}));

test('stream voices reuse gesture-primed elements and their single media source', () => mock(async () => {
  let gesture = true, sources = 0;
  globalThis.Audio = class extends Media {
    play() { if (gesture) this.primed = true; if (!this.primed) return Promise.reject(new Error('gesture required')); return super.play(); }
  };
  const audio = new SpatialAudio(); audio.configure(scene([entity(1, 0, { src: 'stream', stream: true })]));
  audio.unlock(); assert.equal(audio.mediaPool.length, 4);
  assert.ok(audio.mediaPool.every(entry => entry.media.primed && !entry.media.playing));
  const original = audio.context.createMediaElementSource.bind(audio.context);
  audio.context.createMediaElementSource = media => { sources++; return original(media); };
  gesture = false; audio.update(listen()); await settle();
  const first = audio.voices.get(1); assert.equal(first.media.playing, true);
  audio.stop(1); assert.equal(audio.mediaPool.length, 4);
  assert.equal(first.media.onerror, null);
  audio.update(listen()); await settle();
  assert.equal(audio.voices.get(1).media, first.media); assert.equal(audio.voices.get(1).source, first.source);
  assert.equal(sources, 1); assert.equal(audio.voices.get(1).media.playing, true);
  audio.clear();
}));

test('load failures log once and do not crash; no AudioContext is safe', () => mock(async () => {
  const audio = new SpatialAudio(); audio.unlock(); globalThis.fetch = async () => { throw new Error('offline'); };
  const saved = console.warn; const warnings = []; console.warn = message => warnings.push(message);
  try { assert.equal(await audio.resource({ src: 'bad' }), null); assert.equal(await audio.resource({ src: 'bad' }), null); assert.equal(warnings.length, 1); } finally { console.warn = saved; }
  delete globalThis.AudioContext;
  const noAudio = new SpatialAudio(); assert.doesNotThrow(() => { noAudio.unlock(); noAudio.configure(scene([])); noAudio.apply({}); noAudio.update(listen()); noAudio.play('win'); noAudio.suspend(); noAudio.muted = true; });
  assert.equal(noAudio.stats().audioVoices, 0);
}));
