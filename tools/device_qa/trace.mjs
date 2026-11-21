export function digest(value) {
  const canonical=v=>Array.isArray(v)?v.map(canonical):v&&typeof v==='object'?Object.fromEntries(Object.keys(v).sort().map(k=>[k,canonical(v[k])])):v;
  let hash=2166136261;
  for(const byte of new TextEncoder().encode(JSON.stringify(canonical(value))))hash=Math.imul(hash^byte,16777619)>>>0;
  return hash.toString(16).padStart(8,'0');
}

export function summary(snapshot) {
  const state={};
  for(const key of ['score','won','gate_open','Rettungen','_respawns','_checkpoint','_nav_patrol_stop','_nav_patrol_visits']) {
    if(Object.hasOwn(snapshot.state||{},key))state[key]=snapshot.state[key];
  }
  return {tick:snapshot.tick,state,players:(snapshot.transforms||[]).filter(e=>e.tags?.includes('player')).map(e=>({id:e.id,position:e.transform.position})),animators:digest(snapshot.animators||[]),nav_agents:digest(snapshot.nav_agents||[])};
}

export class Trace {
  constructor({maxCalls=80000,maxEvents=256}={}) {
    if(!Number.isInteger(maxCalls)||maxCalls<1||maxCalls>80000||!Number.isInteger(maxEvents)||maxEvents<1||maxEvents>256)throw Error('Invalid trace limits');
    this.maxCalls=maxCalls;this.maxEvents=maxEvents;this.calls=[];this.events=[];this.stopped=false;this.truncated=false;this.firstTick=null;this.last=null;this.started=performance.now();
  }
  event(kind,detail={}) {
    if(this.stopped)return;
    if(this.events.length<this.maxEvents)this.events.push({kind,detail,tick:this.last?.tick??null,at_ms:Math.round(performance.now()-this.started)});
    else this.eventsTruncated=true;
  }
  call(method,args,result) {
    if(this.stopped)return;
    if(this.calls.length===this.maxCalls){this.truncated=true;this.stopped=true;return;}
    const item={method,at_ms:Math.round(performance.now()-this.started)};
    if(method==='load') {
      item.assets={scene:digest(JSON.parse(args[0])),scripts:digest(JSON.parse(args[1]))};
      if(result)this.last=summary(JSON.parse(result));
    } else if(method==='tick') {
      const input=JSON.parse(args[0]);
      if(!input||Array.isArray(input)||typeof input!=='object'||Object.keys(input).length>64||!Object.entries(input).every(([k,v])=>k.length<=64&&typeof v==='number'&&Number.isFinite(v)))throw Error('Invalid logical input');
      item.input=input;
      const snapshot=JSON.parse(result);
      if(!Number.isSafeInteger(snapshot.tick)||snapshot.tick<1)throw Error('Invalid tick');
      if(this.firstTick===null)this.firstTick=snapshot.tick;
      this.last=summary(snapshot);
      if(snapshot.tick%30===0||snapshot.state?.won)item.sample=this.last;
      item.tick=snapshot.tick;
    } else if(method==='set_active_region') {
      if(args.length!==4||!args.every(Number.isFinite))throw Error('Invalid region');
      item.args=[...args];
    } else if(method!=='clear_active_region')throw Error('Unsupported trace method');
    this.calls.push(item);
  }
  report(metadata={}) {
    return {format:'ai-engine-device-qa',version:1,metadata,trace:{calls:structuredClone(this.calls),firstTick:this.firstTick,last:structuredClone(this.last),complete:!this.truncated&&this.firstTick===1,reason:this.truncated?'trace limit or recorder/runtime error':this.firstTick===null?'no ticks':this.firstTick!==1?'recording attached after gameplay started':null},events:structuredClone(this.events),eventsTruncated:!!this.eventsTruncated};
  }
}

export function instrument(WebGame,trace,onFailure=()=>{}) {
  const originals=new Map();
  for(const method of ['load','tick','set_active_region','clear_active_region']) {
    const original=WebGame.prototype[method];
    if(typeof original!=='function')throw Error(`Missing WebGame.${method}`);
    originals.set(method,{original});
  }
  for(const [method,entry] of originals) {
    const {original}=entry;
    const wrapped=function(...args) {
      let result;
      try {result=original.apply(this,args);}catch(error){trace.event('runtime-error',{method});trace.truncated=true;throw error;}
      try {trace.call(method,args,result);}catch(error){trace.truncated=true;trace.stopped=true;try{onFailure(error);}catch{}}
      return result;
    };
    entry.wrapped=wrapped;WebGame.prototype[method]=wrapped;
  }
  return ()=>{for(const [method,{original,wrapped}] of originals)if(WebGame.prototype[method]===wrapped)WebGame.prototype[method]=original;};
}

export function validateReport(report) {
  if(report?.format!=='ai-engine-device-qa'||report.version!==1||!Array.isArray(report.trace?.calls)||report.trace.calls.length>80000)throw Error('Invalid QA report');
  if(!report.trace.complete)throw Error('Incomplete trace cannot prove replay parity');
}
