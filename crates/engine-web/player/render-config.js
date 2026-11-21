export const PRESETS = Object.freeze({
  ultra: {pixelRatio:2,cascades:4,shadowSize:2048,bloom:true,ao:true,antialias:'smaa',foliage:1,distance:72,fog:0.006},
  high: {pixelRatio:1.5,cascades:3,shadowSize:1024,bloom:true,ao:false,antialias:'smaa',foliage:0.7,distance:56,fog:0.008},
  medium: {pixelRatio:1.25,cascades:1,shadowSize:1024,bloom:false,ao:false,antialias:'fxaa',foliage:0.4,distance:40,fog:0.01},
  low: {pixelRatio:1,cascades:0,shadowSize:0,bloom:false,ao:false,antialias:null,foliage:0.15,distance:24,fog:0.014},
});
const levels = ['low','medium','high','ultra'];
export function renderConfig(entities, search = '', coarse = globalThis.matchMedia?.('(pointer: coarse)').matches || false) {
  const props = entities.find(e => e.tags?.includes('player'))?.props || {};
  const authored = props.ai_render, graphics = props.ai_graphics;
  const quality = new URLSearchParams(search).get('quality');
  if (authored == null && graphics == null && ![...levels,'auto'].includes(quality)) return null;
  const parse = input => {
    const value = typeof input === 'string' ? JSON.parse(input) : input ?? {};
    if (!value || typeof value !== 'object' || Array.isArray(value)) throw new TypeError('Render configuration must be an object');
    return value;
  };
  const value = {...parse(graphics),...parse(authored)};
  if (value.preset && !levels.includes(value.preset)) throw new RangeError('Invalid render preset');
  let preset = levels.includes(quality) ? quality : value.preset || 'high';
  if (coarse && !quality && levels.indexOf(preset) > 1) preset = 'medium';
  return {sky:true,fog:true,time_of_day:15,shadows:true,foliage:true,water:false,exposure:1,bloom:true,ao:undefined,dof:false,...value,preset,
    auto_quality: quality === 'auto' || (!levels.includes(quality) && value.auto_quality !== false),coarse};
}
export function sunDirection(hour = 15) {
  const angle = ((hour % 24 + 24) % 24 - 6) / 12 * Math.PI;
  const x = Math.cos(angle), y = Math.sin(angle), z = 0.35;
  const length = Math.hypot(x,y,z); return [x/length,y/length,z/length];
}
export function foliagePlacements(tx,tz,{density=1,size=16,water=-Infinity,sample=()=>0}={}) {
  const hash = n => { let x = Math.imul(tx ^ 0x9e3779b9,tz ^ 0x85ebca6b) ^ Math.imul(n+1,0x27d4eb2d); x = Math.imul(x ^ x>>>16,0x45d9f3b); return ((x ^ x>>>16)>>>0)/4294967296; };
  const result=[];
  for(let i=0;i<Math.floor(160*density);i++) {
    const x=(tx+hash(i*4))*size,z=(tz+hash(i*4+1))*size,y=sample(x,z);
    if (!Number.isFinite(y) || y < water || Math.abs(sample(x+0.5,z)-y)>0.45 || Math.abs(sample(x,z+0.5)-y)>0.45) continue;
    result.push({x,y,z,rotation:hash(i*4+2)*Math.PI*2,scale:0.65+hash(i*4+3)*0.8,tree:i%53===0});
  }
  return result;
}
export class QualityGovernor {
  constructor(preset='high',enabled=true,coarse=false) { this.cap=levels.indexOf(preset)*5; this.level=this.cap; this.enabled=enabled; this.target=coarse?30:50; this.slow=0; this.fast=0; this.time=0; this.frames=0; }
  update(dt) {
    if(!this.enabled || !(dt>0)) return false;
    this.time+=dt; this.frames++;
    if(this.time<0.5) return false;
    const fps=this.frames/this.time, duration=this.time; this.time=0; this.frames=0;
    this.slow=fps<this.target?this.slow+duration:0; this.fast=fps>58?this.fast+duration:0;
    const before=this.level;
    if(this.slow>=2) {this.level=Math.max(0,this.level-1);this.slow=0;}
    if(this.fast>=6) {this.level=Math.min(this.cap,this.level+1);this.fast=0;}
    return before!==this.level;
  }
  settings() {
    const preset=levels[Math.ceil(this.level/5)], degradation=Math.ceil(this.level/5)*5-this.level;
    const p={...PRESETS[preset],preset,level:this.level};
    if(degradation>=1) p.pixelRatio=Math.max(1,p.pixelRatio*0.75);
    if(degradation>=2) {p.bloom=false;p.ao=false;p.antialias='fxaa';}
    if(degradation>=3) p.cascades=Math.min(1,p.cascades);
    if(degradation>=4) p.foliage*=0.5;
    return p;
  }
}
