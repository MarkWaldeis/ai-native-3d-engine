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
  const density = [0.25,0.6,1][level], distance = [24,48,64][level];
  return {density:density*(config?.density ?? 1),distance:Math.min(config?.distance ?? distance,distance),tileSize:16};
}

export function foliagePlacements(tx,tz,{density=1,size=16,water=-Infinity,sample=()=>0}={}) {
  const hash = n => { let x = Math.imul(tx ^ 0x9e3779b9,tz ^ 0x85ebca6b) ^ Math.imul(n+1,0x27d4eb2d); x = Math.imul(x ^ x>>>16,0x45d9f3b); return ((x ^ x>>>16)>>>0)/4294967296; };
  const result=[];
  for(let i=0;i<Math.floor(220*density);i++) {
    const x=(tx+hash(i*4))*size,z=(tz+hash(i*4+1))*size,y=sample(x,z);
    if (!Number.isFinite(y) || y < water || !(Math.abs(sample(x+0.5,z)-y)<=0.45) || !(Math.abs(sample(x,z+0.5)-y)<=0.45)) continue;
    result.push({x,y,z,rotation:hash(i*4+2)*Math.PI*2,scale:0.65+hash(i*4+3)*0.8,tree:i%59===0});
  }
  return result;
}

export function adaptQuality(state, fps, seconds, target = 42) {
  state.slow = fps < target ? state.slow + seconds : 0;
  state.fast = fps > 58 ? state.fast + seconds : 0;
  const before = state.level;
  if (state.slow >= 6 && state.level > 0) {state.level--;state.slow=state.fast=0;}
  if (state.fast >= 12 && state.level < state.startLevel) {state.level++;state.slow=state.fast=0;}
  return state.level !== before;
}
