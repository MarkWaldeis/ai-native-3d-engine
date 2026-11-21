import { materialSettings } from './graphics-settings.js';

const defaults = {chunk_size:128, stream_radius:512, draw_distance:480, lod_distances:[100,260], hysteresis:16, sim_radius:256, sim_hysteresis:32, instancing:true};

export function worldConfig(entities) {
  const authored = entities.find(e => e.tags?.includes('player'))?.props?.ai_world;
  const value = typeof authored === 'string' ? JSON.parse(authored) : authored;
  const large = entities.length > 1000 || entities.some(e => Math.max(...(e.transform?.position || []).map(Math.abs)) > 1000);
  if (!value && !large) return null;
  const config = {...defaults, ...value};
  if (config.camera_distance !== undefined && (!(config.camera_distance > 0) || !Number.isFinite(config.camera_distance))) throw new RangeError('Invalid ai_world.camera_distance');
  for (const key of ['chunk_size','stream_radius','draw_distance','hysteresis','sim_radius','sim_hysteresis']) if (!(config[key] > 0) || !Number.isFinite(config[key])) throw new RangeError(`Invalid ai_world.${key}`);
  if (!Array.isArray(config.lod_distances) || config.lod_distances.length !== 2 || !(config.lod_distances[0] > 0 && config.lod_distances[1] > config.lod_distances[0])) throw new RangeError('Invalid LOD distances');
  return config;
}

export function batchKey(e) {
  if (e.kind === 'gltf') return JSON.stringify(['gltf',e.path,e.props?.web_lods]);
  return JSON.stringify([e.kind,e.size,e.radius,e.height,e.width,e.door_width,e.door_height,e.color,e.segments,materialSettings(e.props?.ai_material),e.kind==='box'&&!!e.tags?.includes('road')]);
}

export function entityRadius(e) {
  const scale = e.transform?.scale || [1,1,1], bounds = e.props?.web_mesh_bounds;
  if (bounds) return Math.hypot(...bounds[0].map((min,i) => Math.max(Math.abs(min),Math.abs(bounds[1][i])) * Math.abs(scale[i])));
  return Math.max(typeof e.size === 'number' ? e.size : Math.hypot(...e.size || [1,1,1]),e.radius || 0,e.height || 0) * Math.max(...scale.map(Math.abs));
}

export function isStatic(e) {
  return !e.script && !['dynamic','kinematic'].includes(e.rigidbody?.type) && !(e.tags || []).some(t => ['player','trigger','coin','crystal','portal'].includes(t));
}

export function canInstance(e) {
  const scale = e.transform?.scale || [1,1,1];
  return !e.animator && isStatic(e) && scale[0]*scale[1]*scale[2] > 0 && ['box','plane','sphere','cylinder','gltf'].includes(e.kind);
}

export function selectLOD(distance, previous, config) {
  const limits = [...config.lod_distances,config.draw_distance];
  let level = limits.findIndex(limit => distance < limit);
  if (level < 0) level = 3;
  if (previous !== undefined && previous !== level) {
    if (level > previous && distance < limits[previous] + config.hysteresis) return previous;
    if (level < previous && distance > limits[previous - 1] - config.hysteresis) return previous;
  }
  return level;
}

export class ChunkGrid {
  constructor(config = {}) { this.config = {...defaults,...config}; this.items = new Map(); this.chunks = new Map(); this.loaded = new Set(); this.radii = new Map(); this.maxRadius = 0; }
  key(position) { return `${Math.floor(position[0]/this.config.chunk_size)},${Math.floor(position[2]/this.config.chunk_size)}`; }
  register(id, position, radius = 0) {
    this.remove(id);
    const key = this.key(position), item = {id,position:[...position],radius,key};
    this.items.set(id,item);
    if (!this.chunks.has(key)) this.chunks.set(key,new Map());
    this.chunks.get(key).set(id,item);
    this.radii.set(key,Math.max(this.radii.get(key) || 0,radius)); this.maxRadius = Math.max(this.maxRadius,radius);
    return key;
  }
  move(id, position, radius = this.items.get(id)?.radius || 0) { return this.register(id,position,radius); }
  remove(id) {
    const item = this.items.get(id); if (!item) return;
    const chunk = this.chunks.get(item.key); chunk.delete(id);
    if (!chunk.size) { this.chunks.delete(item.key); this.radii.delete(item.key); }
    this.items.delete(id);
  }
  update(position) {
    const load = [], unload = [], size = this.config.chunk_size;
    const reach = Math.ceil((this.config.stream_radius + this.maxRadius) / size) + 1;
    const cx = Math.floor(position[0]/size), cz = Math.floor(position[2]/size);
    for (let x=cx-reach;x<=cx+reach;x++) for (let z=cz-reach;z<=cz+reach;z++) {
      const key = `${x},${z}`;
      if (!this.chunks.has(key) || this.loaded.has(key)) continue;
      if (Math.hypot(Math.max(x*size-position[0],0,position[0]-(x+1)*size),Math.max(z*size-position[2],0,position[2]-(z+1)*size)) <= this.config.stream_radius + (this.radii.get(key) || 0)) { this.loaded.add(key); load.push(key); }
    }
    for (const key of this.loaded) {
      const [x,z] = key.split(',').map(Number);
      if (!this.chunks.has(key) || Math.hypot(Math.max(x*size-position[0],0,position[0]-(x+1)*size),Math.max(z*size-position[2],0,position[2]-(z+1)*size)) > this.config.stream_radius + this.config.hysteresis + (this.radii.get(key) || 0)) { this.loaded.delete(key); unload.push(key); }
    }
    return {load,unload};
  }
}
