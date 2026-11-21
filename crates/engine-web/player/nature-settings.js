export function sunDirection(hour) {
  const angle = (((hour % 24 + 24) % 24) - 6) * Math.PI / 12;
  const x = Math.cos(angle), y = Math.sin(angle), z = 0.35, length = Math.hypot(x,y,z);
  return [x/length,y/length,z/length];
}

export function daylightDefaults(hour) {
  if (!Number.isFinite(hour)) return {};
  const sun_direction = sunDirection(hour), elevation = sun_direction[1];
  const day = elevation > 0.3, night = elevation < 0;
  return {sun_direction, sun_color:day?'#fff0cf':night?'#829ed5':'#ffbb70',
    sky:day?'#347db4':night?'#14273f':'#376b97',horizon:day?'#a6c5c8':night?'#34465f':'#d8ac82',
    sun_intensity:Math.max(0,elevation)*3.4};
}

export function foliageSettings(config, level) {
  const density = [0.16,0.45,1.4][level], distance = [24,36,112][level];
  return {density:density*(config?.density ?? 1),trees:config?.trees ?? 1,distance:Math.min(config?.distance ?? distance,distance),tileSize:16};
}

function cellRandom(x, z, salt) {
  let h = Math.imul(x, 0x632be59b) ^ Math.imul(z, 0x85157af5) ^ Math.imul(salt, 0x9e3779b9);
  h = Math.imul(h ^ h >>> 16, 0x45d9f3b);
  return ((h ^ h >>> 16) >>> 0) / 4294967296;
}

export function jungleSettings(level) {
  return {radius:[86,116,150][level], budget:[104,176,286][level], palm:[0,1,1][level]};
}

export function junglePlacements(cx, cz, {radius,budget,palm,sample,exclude=()=>false,water=-Infinity}) {
  const spacing=15, result=[];
  for(let z=Math.floor((cz-radius)/spacing);z<=Math.floor((cz+radius)/spacing);z++) {
    for(let x=Math.floor((cx-radius)/spacing);x<=Math.floor((cx+radius)/spacing);x++) {
      const wx=(x+0.1+0.8*cellRandom(x,z,1))*spacing, wz=(z+0.1+0.8*cellRandom(x,z,2))*spacing;
      const distance=Math.hypot(wx-cx,wz-cz);
      if(distance>radius || exclude(wx,wz)) continue;
      const y=sample(wx,wz);
      if(!Number.isFinite(y) || y<water+1 ||
        !(Math.abs(sample(wx+2,wz)-y)<=1.4 && Math.abs(sample(wx,wz+2)-y)<=1.4)) continue;
      const hue=cellRandom(x,z,3);
      result.push({x:wx,y,z:wz,distance,hue,variant:palm && cellRandom(x,z,4)>.77 ? 1 : 0,
        scale:0.82+cellRandom(x,z,5)*0.88, rotation:cellRandom(x,z,6)*Math.PI*2});
    }
  }
  return result.sort((a,b)=>a.distance-b.distance).slice(0,budget);
}

export function foliagePlacements(tx,tz,{density=1,size=16,water=-Infinity,sample=()=>0,trees=1,exclude=()=>false}={}) {
  const hash = n => { let x = Math.imul(tx ^ 0x9e3779b9,tz ^ 0x85ebca6b) ^ Math.imul(n+1,0x27d4eb2d); x = Math.imul(x ^ x>>>16,0x45d9f3b); return ((x ^ x>>>16)>>>0)/4294967296; };
  const result=[];
  for(let i=0;i<Math.floor(220*density);i++) {
    const x=(tx+hash(i*4))*size,z=(tz+hash(i*4+1))*size,y=sample(x,z);
    if (!Number.isFinite(y) || y < water || exclude(x,z) || !(Math.abs(sample(x+0.5,z)-y)<=0.45) || !(Math.abs(sample(x,z+0.5)-y)<=0.45)) continue;
    result.push({x,y,z,rotation:hash(i*4+2)*Math.PI*2,scale:0.65+hash(i*4+3)*0.8,tree:i%59===0&&hash(i+900)<trees,kind:hash(i+1700),hue:hash(i+2500)});
  }
  return result;
}

export function foliageExclusion(entities, jungle=false) {
  const cells=new Map(),size=32;
  for(const e of entities) {
    const t=e.transform;
    if(!Array.isArray(t?.position)||t.position.length!==3||!t.position.every(Number.isFinite))continue;
    const road=e.kind==='box'&&e.tags?.includes('road')&&Array.isArray(e.size)&&e.size.length===3&&e.size.every(Number.isFinite);
    const tree=jungle&&e.tags?.includes('tree');
    const meshBounds=e.props?.web_mesh_bounds;
    const bounds=jungle&&e.tags?.some(tag=>tag==='building'||tag==='bridge'||tag==='cover')&&
      Array.isArray(meshBounds)&&meshBounds.length===2&&meshBounds.every(v=>Array.isArray(v)&&v.length===3&&v.every(Number.isFinite))?meshBounds:null;
    if(!road&&!tree&&!bounds)continue;
    const angle=(t.rotation_deg?.[1]||0)*Math.PI/180,c=Math.cos(angle),s=Math.sin(angle);
    const extent=i=>tree?5:road?e.size[i]/2:Math.max(Math.abs(bounds[0][i]),Math.abs(bounds[1][i]));
    const margin=jungle ? 3 : 0.6;
    const area={x:t.position[0],z:t.position[2],hx:extent(0)*Math.abs(t.scale?.[0]??1)+margin,hz:extent(2)*Math.abs(t.scale?.[2]??1)+margin,c,s};
    const rx=Math.abs(c)*area.hx+Math.abs(s)*area.hz,rz=Math.abs(s)*area.hx+Math.abs(c)*area.hz;
    for(let x=Math.floor((area.x-rx)/size);x<=Math.floor((area.x+rx)/size);x++)for(let z=Math.floor((area.z-rz)/size);z<=Math.floor((area.z+rz)/size);z++) {
      const key=`${x},${z}`;if(!cells.has(key))cells.set(key,[]);cells.get(key).push(area);
    }
  }
  return (x,z)=>(cells.get(`${Math.floor(x/size)},${Math.floor(z/size)}`)||[]).some(r=>{
    const dx=x-r.x,dz=z-r.z;return Math.abs(dx*r.c-dz*r.s)<=r.hx&&Math.abs(dx*r.s+dz*r.c)<=r.hz;
  });
}

export function adaptQuality(state, fps, seconds, target = 42) {
  state.slow = fps < target ? state.slow + seconds : 0;
  state.fast = fps > 58 ? state.fast + seconds : 0;
  const before = state.level;
  if (state.slow >= 6 && state.level > 0) {state.level--;state.slow=state.fast=0;}
  if (state.fast >= 12 && state.level < state.startLevel) {state.level++;state.slow=state.fast=0;}
  return state.level !== before;
}

export function waterDepth(ground, level) {
  return Number.isFinite(ground) ? Math.max(0, Math.min(64, level - ground)) : 0;
}

export function bakeWaterDepth(sample, level, x, z, span, resolution, data = new Uint8Array(resolution * resolution * 4)) {
  for (let j = 0; j < resolution; j++) for (let i = 0; i < resolution; i++) {
    const depth = waterDepth(sample(x + (i / (resolution - 1) - 0.5) * span, z + (j / (resolution - 1) - 0.5) * span), level);
    const value = Math.round(depth / 64 * 65535), offset = (j * resolution + i) * 4;
    data[offset] = value >>> 8; data[offset + 1] = value & 255; data[offset + 3] = 255;
  }
  return data;
}

export function replacesWater(entity, graphics) {
  return !!graphics?.water && entity.kind === 'plane' && entity.tags?.includes('water');
}
