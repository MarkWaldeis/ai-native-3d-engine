import { terrainSurface } from './surfaces.js';

// Transport, residency and rendering only. Simulation remains in WebGame.
export function decodeHeights(bytes, magic, dims, range) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (bytes.length !== 12 + dims[0] * dims[1] * 2 ||
      String.fromCharCode(...bytes.subarray(0, 4)) !== magic ||
      view.getUint32(4, true) !== dims[0] || view.getUint32(8, true) !== dims[1]) throw new Error('Invalid terrain heightfield');
  const heights = new Float32Array(dims[0] * dims[1]);
  for (let i = 0; i < heights.length; i++) heights[i] = range[0] + view.getUint16(12 + i * 2, true) / 65535 * (range[1] - range[0]);
  return heights;
}

export function sampleTile(heights, config, tx, tz, x, z) {
  const n = config.samples, spacing = config.tile_m / (n - 1);
  const u = Math.max(0, Math.min(n - 1, (x - tx * config.tile_m) / spacing));
  const v = Math.max(0, Math.min(n - 1, (z - tz * config.tile_m) / spacing));
  const i = Math.min(n - 2, Math.floor(u)), j = Math.min(n - 2, Math.floor(v)), a = u - i, b = v - j;
  const h = (dx, dz) => heights[(j + dz + 1) * (n + 2) + i + dx + 1];
  return a + b <= 1 ? h(0, 0) * (1 - a - b) + h(1, 0) * a + h(0, 1) * b : h(1, 0) * (1 - b) + h(0, 1) * (1 - a) + h(1, 1) * (a + b - 1);
}

export function terrainLOD(distance, previous = null) {
  const limits = [180, 420, 800];
  if (previous !== null) {
    const level = Math.log2(previous);
    if (distance >= (limits[level - 1] || 0) - 35 && distance <= (limits[level] || Infinity) + 35) return previous;
  }
  return 2 ** limits.filter(limit => distance > limit).length;
}

export function terrainReach(far, quality = 2) {
  return Math.min(far, [320,480,900][quality] ?? 900);
}

export function drainTerrainWork(jobs, work, limit = 2, milliseconds = 3, clock = () => performance.now()) {
  jobs.sort((a,b)=>a.distance-b.distance);
  const start=clock();let count=0;
  while(jobs.length && count<limit && (count===0 || clock()-start<milliseconds)) {
    work(jobs.shift());count++;
  }
  return count;
}

export class TileQueue {
  constructor(fetchBytes, limit = 6, capacity = 128) {
    this.fetchBytes = fetchBytes; this.limit = limit; this.capacity = capacity;
    this.active = 0; this.queue = []; this.pending = new Map(); this.cache = new Map(); this.bytes = 0;
  }
  get(url) {
    if (this.cache.has(url)) {
      const bytes = this.cache.get(url); this.cache.delete(url); this.cache.set(url, bytes);
      return Promise.resolve(bytes);
    }
    if (this.pending.has(url)) return this.pending.get(url);
    const promise = new Promise((resolve, reject) => this.queue.push({url, resolve, reject}));
    this.pending.set(url, promise); this.pump(); return promise;
  }
  pump() {
    while (this.active < this.limit && this.queue.length) {
      const {url, resolve, reject} = this.queue.shift(); this.active++;
      Promise.resolve().then(() => this.fetchBytes(url)).then(bytes => {
        this.bytes += bytes.byteLength; this.cache.set(url, bytes);
        while (this.cache.size > this.capacity) this.cache.delete(this.cache.keys().next().value);
        resolve(bytes);
      }, reject).finally(() => { this.active--; this.pending.delete(url); this.pump(); });
    }
  }
}

async function fetchBytes(url) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  return new Uint8Array(await response.arrayBuffer());
}

export function terrainDetailTexture(THREE) {
  const textureBytes = new Uint8Array(64 * 64 * 4);
  let seed = 7331;
  for (let i = 0; i < textureBytes.length; i += 4) {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    textureBytes.fill(100 + (seed >>> 25), i, i + 3); textureBytes[i + 3] = 255;
  }
  const detail = new THREE.DataTexture(textureBytes, 64, 64);
  detail.wrapS = detail.wrapT = THREE.RepeatWrapping; detail.magFilter = THREE.LinearFilter;
  detail.minFilter = THREE.LinearMipmapLinearFilter; detail.generateMipmaps = true; detail.needsUpdate = true;
  return detail;
}


export async function createTerrain(THREE, entities, game, world, engineStats, onError) {
  const terrainEntities = entities.filter(e => typeof e.props?.ai_terrain === 'string');
  if (!terrainEntities.length) return null;
  const queue = new TileQueue(fetchBytes), maps = new Map(), chunks = [], decoded = new Map(), indices = new Map(), layers = [];
  const frustum = new THREE.Frustum(), viewProjection = new THREE.Matrix4();
  let occluder, occludedTerrainTiles=0;
  let lastPoll = -Infinity, terrainBuilds=0, terrainActivations=0;
  const fog=world.fog, authoredFog=fog && {density:fog.density,near:fog.near,far:fog.far};
  let visualsTarget, visualsSetter;
  function connectVisuals() {
    const visuals = window.__aiEngineVisuals;
    if (typeof visuals?.setHeightSampler === 'function' && (visuals !== visualsTarget || visuals.setHeightSampler !== visualsSetter)) {
      visuals.setHeightSampler(heightAt);
      visualsTarget = visuals; visualsSetter = visuals.setHeightSampler;
    }
  }
  const graphics = entities.find(e => e.tags?.includes('player'))?.props?.ai_graphics;
  let authored = graphics;
  if (typeof authored === 'string') {
    try { authored = JSON.parse(authored); } catch { authored = null; }
  }
  const water = Number.isFinite(authored?.water?.level) ? authored.water.level : 0.6;
  const detail = terrainEntities.some(e => Number.isFinite(e.props.ai_terrain_detail) && e.props.ai_terrain_detail > 0)
    ? terrainDetailTexture(THREE) : null;
  for (const entity of terrainEntities) {
    const path = entity.props.ai_terrain;
    if (!maps.has(path)) {
      const manifestBytes = await queue.get(new URL(path, location.href).href);
      const manifest = JSON.parse(new TextDecoder().decode(manifestBytes));
      const base = new URL(path, location.href);
      const coarseBytes = await queue.get(new URL(manifest.coarse.file, base).href);
      game.set_terrain(path, JSON.stringify(manifest), coarseBytes);
      maps.set(path, {path, manifest, base, coarse: decodeHeights(coarseBytes, 'AITC', manifest.coarse.dims, manifest.height_range)});
    }
    const map = maps.get(path), c = map.manifest.config;
    const offset = entity.transform?.position || [0, 0, 0];
    const layer = {map, offset, visible: entity.visible !== false, chunks: []};
    const material = new THREE.MeshStandardMaterial({roughness: 0.96});
    terrainSurface(THREE, material, water, detail, entity.props.ai_terrain_detail);
    for (let z = 0; z < map.manifest.grid[1]; z++) for (let x = 0; x < map.manifest.grid[0]; x++) {
      const chunk = {map, x, z, offset, material, visible: layer.visible, mesh: null, stride: null};
      chunks.push(chunk); layer.chunks.push(chunk);
    }
    if (layer.visible) layers.push(layer);
  }

  function coarseHeight(map, x, z) {
    const {config: c, coarse: info} = map.manifest;
    x = Math.max(0, Math.min(c.size_m[0], x)); z = Math.max(0, Math.min(c.size_m[1], z));
    const i = Math.min(info.dims[0] - 2, Math.floor(x / info.step)), j = Math.min(info.dims[1] - 2, Math.floor(z / info.step));
    const a = (x - i * info.step) / (Math.min((i + 1) * info.step, c.size_m[0]) - i * info.step);
    const b = (z - j * info.step) / (Math.min((j + 1) * info.step, c.size_m[1]) - j * info.step);
    const h = (dx, dz) => map.coarse[(j + dz) * info.dims[0] + i + dx];
    return (h(0,0)*(1-a)+h(1,0)*a)*(1-b)+(h(0,1)*(1-a)+h(1,1)*a)*b;
  }
  function key(map, x, z) { return `${map.path}:${x}:${z}`; }
  function tileURL(map, x, z) { return new URL(map.manifest.tiles.replace('{x}', x).replace('{z}', z), map.base).href; }
  function tileData(map, x, z, bytes) {
    const id = key(map,x,z);
    let data = decoded.get(id);
    if (!data) { const n = map.manifest.config.samples + 2; data = decodeHeights(bytes,'AITT',[n,n],map.manifest.height_range); }
    decoded.delete(id); decoded.set(id,data);
    while (decoded.size > 96) decoded.delete(decoded.keys().next().value);
    return data;
  }
  function buildMesh(chunk, heights, stride) {
    const {map,x,z,offset,material} = chunk, c = map.manifest.config;
    const width = c.tile_m, depth = c.tile_m;
    const step = heights ? c.tile_m/(c.samples-1)*stride : Math.max(map.manifest.coarse.step,c.tile_m/2);
    function axisPositions(tile, size) {
      if(heights) {
        const values=[];
        for(let i=0;i<c.samples-1;i+=stride) values.push(i*c.tile_m/(c.samples-1));
        values.push(c.tile_m);
        return values;
      }
      const start=tile*c.tile_m, values=[0], end=Math.min(start+c.tile_m,size);
      for(let p=(Math.floor(start/step)+1)*step;p<end;p+=step) values.push(p-start);
      if(end<start+c.tile_m) values.push(end-start);
      values.push(c.tile_m);
      return values;
    }
    const xs=axisPositions(x,c.size_m[0]), zs=axisPositions(z,c.size_m[1]);
    const nx = xs.length, nz = zs.length;
    const positions = [], normals = [];
    const height = (lx,lz) => heights ? sampleTile(heights,c,x,z,lx+x*c.tile_m,lz+z*c.tile_m) : coarseHeight(map,lx+x*c.tile_m,lz+z*c.tile_m);
    for (let j=0;j<nz;j++) for (let i=0;i<nx;i++) {
      const lx=xs[i], lz=zs[j], h=height(lx,lz);
      let dx, dz;
      if (heights) {
        const n=c.samples+2, ix=Math.round(lx/(c.tile_m/(c.samples-1)))+1, iz=Math.round(lz/(c.tile_m/(c.samples-1)))+1;
        const spacing=c.tile_m/(c.samples-1);
        dx=(heights[iz*n+ix+1]-heights[iz*n+ix-1])/(2*spacing);
        dz=(heights[(iz+1)*n+ix]-heights[(iz-1)*n+ix])/(2*spacing);
      } else {dx=(height(lx+step,lz)-height(lx-step,lz))/(2*step); dz=(height(lx,lz+step)-height(lx,lz-step))/(2*step);}
      const length=Math.hypot(dx,1,dz);
      positions.push(lx,h,lz); normals.push(-dx/length,1/length,-dz/length);
    }
    const edge=[];
    for(let i=0;i<nx;i++) edge.push(i);
    for(let j=1;j<nz;j++) edge.push(j*nx+nx-1);
    for(let i=nx-2;i>=0;i--) edge.push((nz-1)*nx+i);
    for(let j=nz-2;j>0;j--) edge.push(j*nx);
    let error=0;
    if(heights && stride>1) {
      for(let j=0;j<c.samples;j++) for(let i=0;i<c.samples;i++) {
        const lx=i*c.tile_m/(c.samples-1),lz=j*c.tile_m/(c.samples-1);
        const ix=Math.min(nx-2,Math.floor(i/stride)),iz=Math.min(nz-2,Math.floor(j/stride));
        const dx=xs[ix+1]-xs[ix],dz=zs[iz+1]-zs[iz];
        const a=dx>0?(lx-xs[ix])/dx:0,b=dz>0?(lz-zs[iz])/dz:0;
        const h=(dx,dz)=>positions[((iz+dz)*nx+ix+dx)*3+1];
        const simplified=a+b<=1?h(0,0)*(1-a-b)+h(1,0)*a+h(0,1)*b:h(1,0)*(1-b)+h(0,1)*(1-a)+h(1,1)*(a+b-1);
        error=Math.max(error,Math.abs(simplified-heights[(j+1)*(c.samples+2)+i+1]));
      }
    }
    const skirt=heights ? error+5 : Math.max(8,(map.manifest.height_range[1]-map.manifest.height_range[0])*0.06,step*0.7);
    for(const index of edge) {positions.push(positions[index*3],positions[index*3+1]-skirt,positions[index*3+2]); normals.push(...normals.slice(index*3,index*3+3));}
    const indexKey=`${nx}:${nz}`;
    if(!indices.has(indexKey)) {
      const list=[];
      for(let j=0;j<nz-1;j++) for(let i=0;i<nx-1;i++) {const a=j*nx+i,b=a+nx; list.push(a,b,a+1,a+1,b,b+1);}
      for(let i=0;i<edge.length;i++) {const next=(i+1)%edge.length,a=edge[i],b=edge[next],u=nx*nz+i,v=nx*nz+next; list.push(a,u,b,b,u,v);}
      indices.set(indexKey,new THREE.BufferAttribute(new Uint32Array(list),1));
    }
    const geometry=new THREE.BufferGeometry();
    geometry.setAttribute('position',new THREE.Float32BufferAttribute(positions,3)); geometry.setAttribute('normal',new THREE.Float32BufferAttribute(normals,3)); geometry.setIndex(indices.get(indexKey)); geometry.computeBoundingSphere(); geometry.computeBoundingBox();
    const mesh=new THREE.Mesh(geometry,material); mesh.position.set(offset[0]+x*c.tile_m,offset[1],offset[2]+z*c.tile_m); mesh.receiveShadow=true;
    mesh.visible=false;mesh.onBeforeRender=()=>{mesh.userData.terrainUploaded=true;};
    return mesh;
  }
  async function physicsTiles() {
    await Promise.all(JSON.parse(game.terrain_tiles_wanted(128)).map(async ({path,x,z}) => {
      const map=maps.get(path); if(!map) throw new Error(`Unknown terrain ${path}`);
      const bytes=await queue.get(tileURL(map,x,z)); game.add_terrain_tile(path,x,z,bytes);
    }));
  }
  async function initialTiles() {
    const centers=entities.filter(e=>e.tags?.includes('player') || e.rigidbody?.type==='dynamic').map(e=>e.transform?.position || [0,0,0]);
    const wanted=new Map();
    for(const entity of terrainEntities) {
      const map=maps.get(entity.props.ai_terrain), c=map.manifest.config, offset=entity.transform?.position || [0,0,0];
      const radius=(typeof entity.props.ai_terrain_radius==='number' ? entity.props.ai_terrain_radius : 512)+128;
      for(const center of centers) {
        const local=[center[0]-offset[0],center[2]-offset[2]];
        const lower=local.map((v,i)=>Math.max(0,Math.min(map.manifest.grid[i],Math.ceil((v-radius)/c.tile_m)-1)));
        const upper=local.map((v,i)=>Math.max(-1,Math.min(map.manifest.grid[i]-1,Math.floor((v+radius)/c.tile_m))));
        for(let z=lower[1];z<=upper[1];z++) for(let x=lower[0];x<=upper[0];x++) {
          const dx=local[0]-Math.max(x*c.tile_m,Math.min((x+1)*c.tile_m,local[0]));
          const dz=local[1]-Math.max(z*c.tile_m,Math.min((z+1)*c.tile_m,local[1]));
          if(dx*dx+dz*dz<=radius*radius) wanted.set(key(map,x,z),{map,x,z});
        }
      }
    }
    await Promise.all([...wanted.values()].map(async ({map,x,z})=>{
      const bytes=await queue.get(tileURL(map,x,z));
      game.add_terrain_tile(map.path,x,z,bytes); tileData(map,x,z,bytes);
    }));
  }
  function disposeTile(mesh) {
    mesh.removeFromParent();
    mesh.geometry.setIndex(null);
    mesh.geometry.dispose();
  }
  function update(camera, now = performance.now()) {
    if(now-lastPoll>=500) {lastPoll=now; connectVisuals(); physicsTiles().catch(onError);}
    const quality=Number(world.userData.terrainQuality?.() ?? 2),reach=terrainReach(camera.far,quality);
    if(chunks.some(chunk=>chunk.visible) && fog) {
      if(fog.isFogExp2) fog.density=Math.max(authoredFog.density,Math.sqrt(-Math.log(0.01))/(reach*0.9));
      else if(fog.isFog) {fog.near=Math.min(authoredFog.near,reach*0.55);fog.far=Math.min(authoredFog.far,reach*0.9);}
    }
    const jobs=[];let fetches=0;
    const nearest=chunks.filter(chunk=>chunk.visible).map(chunk=>{
      const c=chunk.map.manifest.config,x=chunk.offset[0]+chunk.x*c.tile_m,z=chunk.offset[2]+chunk.z*c.tile_m;
      return {chunk,distance:Math.hypot(Math.max(x-camera.position.x,0,camera.position.x-x-c.tile_m),Math.max(z-camera.position.z,0,camera.position.z-z-c.tile_m))};
    }).sort((a,b)=>a.distance-b.distance);
    for(const job of nearest) {
      const {chunk,distance:d}=job;
      if(chunk.mesh)chunk.mesh.visible=false;
      if(d>reach) {
        if(d>reach+chunk.map.manifest.config.tile_m && chunk.mesh) {disposeTile(chunk.mesh);chunk.mesh=null;chunk.stride=null;}
        continue;
      }
      const c=chunk.map.manifest.config;
      const centerDistance=Math.hypot(camera.position.x-chunk.offset[0]-(chunk.x+0.5)*c.tile_m,camera.position.z-chunk.offset[2]-(chunk.z+0.5)*c.tile_m);
      const stride=terrainLOD(centerDistance,chunk.stride),id=key(chunk.map,chunk.x,chunk.z),data=decoded.get(id);
      if(!chunk.mesh || (data && chunk.stride!==stride)) jobs.push({...job,data,stride});
      if(!data && !chunk.fetching && fetches<2) {
        fetches++;chunk.fetching=true;
        queue.get(tileURL(chunk.map,chunk.x,chunk.z)).then(bytes=>tileData(chunk.map,chunk.x,chunk.z,bytes)).catch(onError).finally(()=>{chunk.fetching=false;});
      }
    }
    terrainBuilds=drainTerrainWork(jobs,({chunk,data,stride})=>{
      const mesh=buildMesh(chunk,data,data?stride:1);
      if(chunk.mesh)disposeTile(chunk.mesh);
      chunk.mesh=mesh;chunk.stride=data?stride:null;world.add(mesh);
    });
    camera.updateMatrixWorld();
    frustum.setFromProjectionMatrix(viewProjection.multiplyMatrices(camera.projectionMatrix,camera.matrixWorldInverse));
    const activations=[];
    occludedTerrainTiles=0;
    for(const {chunk,distance} of nearest) {
      chunk.inView=false;
      const mesh=chunk.mesh;
      if(!mesh || distance>reach)continue;
      mesh.updateMatrixWorld(); chunk.inView=frustum.intersectsObject(mesh);
      const box=mesh.geometry.boundingBox.clone().translate(mesh.position);
      if(chunk.inView && occluder?.occluded(chunk,camera.position,box)) {mesh.visible=false;chunk.inView=false;occludedTerrainTiles++;}
      if(chunk.inView) {
        if(mesh.userData.terrainUploaded)mesh.visible=true;
        else {chunk.inView=false;activations.push({chunk,distance});}
      }
    }
    terrainActivations=drainTerrainWork(activations,({chunk})=>{chunk.mesh.visible=true;chunk.inView=true;});
    Object.assign(engineStats,stats());
  }
  function stats() {
    const visible=chunks.filter(c=>c.inView);
    return {terrainBuilds,terrainActivations,occludedTerrainTiles,terrainTilesLoaded:decoded.size,terrainTilesVisible:visible.length,terrainTriangles:visible.reduce((n,c)=>n+c.mesh.geometry.index.count/3,0),terrainPendingFetches:queue.pending.size,terrainBytesFetched:queue.bytes};
  }
  function heightAt(x,z) {
    for(const chunk of chunks) {
      const c=chunk.map.manifest.config,lx=x-chunk.offset[0],lz=z-chunk.offset[2];
      if(lx<chunk.x*c.tile_m || lz<chunk.z*c.tile_m || lx>Math.min((chunk.x+1)*c.tile_m,c.size_m[0]) || lz>Math.min((chunk.z+1)*c.tile_m,c.size_m[1])) continue;
      const data=decoded.get(key(chunk.map,chunk.x,chunk.z));
      return chunk.offset[1]+(data ? sampleTile(data,c,chunk.x,chunk.z,lx,lz) : coarseHeight(chunk.map,lx,lz));
    }
    const first=chunks[0];
    return first ? first.offset[1]+coarseHeight(first.map,x-first.offset[0],z-first.offset[2]) : 0;
  }
  function occluderHeightAt(x,z) {
    let best=NaN;
    for(const layer of layers) {
      const c=layer.map.manifest.config,lx=x-layer.offset[0],lz=z-layer.offset[2];
      if(!(lx>=0 && lz>=0 && lx<=c.size_m[0] && lz<=c.size_m[1])) continue;
      const grid=layer.map.manifest.grid,tx=Math.min(grid[0]-1,Math.floor(lx/c.tile_m)),tz=Math.min(grid[1]-1,Math.floor(lz/c.tile_m));
      const chunk=layer.chunks[tz*grid[0]+tx],data=decoded.get(key(layer.map,tx,tz));
      const h=layer.offset[1]+(data ? sampleTile(data,c,tx,tz,lx,lz) : coarseHeight(layer.map,lx,lz));
      if(chunk.visible && !(h<=best)) best=h;
    }
    return best;
  }
  const api={occluderHeightAt,setOcclusion:value=>{occluder=value;},preload:physicsTiles,update,heightAt,stats,physicsHeightAt:(x,z)=>game.terrain_height(x,z)};
  await initialTiles();
  window.__aiTerrain=api; connectVisuals(); return api;
}
