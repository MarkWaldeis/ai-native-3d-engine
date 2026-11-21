export function compareSample(actual,expected,where='sample') {
  if(typeof expected==='number') {
    const exact=/(?:^|\.)(id|next)$/.test(where);
    if(typeof actual!=='number'||!Number.isFinite(actual)||!Number.isFinite(expected)||(exact?actual!==expected:Math.abs(actual-expected)>1e-5))throw Error(`${where}: numeric parity mismatch: ${actual} != ${expected}`);
  } else if(expected&&typeof expected==='object') {
    if(!actual||typeof actual!=='object'||Array.isArray(actual)!==Array.isArray(expected))throw Error(`${where}: shape mismatch`);
    const keys=Object.keys(expected).sort();if(JSON.stringify(Object.keys(actual).sort())!==JSON.stringify(keys))throw Error(`${where}: keys/length mismatch`);
    for(const key of keys)compareSample(actual[key],expected[key],`${where}.${key}`);
  } else if(actual!==expected)throw Error(`${where}: parity mismatch: ${actual} != ${expected}`);
}

export function validateFixture(fixture) {
  if(fixture?.version!==1||!Array.isArray(fixture.cases)||fixture.cases.length<1||fixture.cases.length>16)throw Error('Expected version 1 with 1..16 workloads');
  const names=new Set();
  for(const c of fixture.cases) {
    if(typeof c?.name!=='string'||!c.name.length||c.name.length>128||names.has(c.name)||!Number.isInteger(c.agents)||c.agents<0||c.agents>128||!Number.isInteger(c.ticks)||c.ticks<1||c.ticks>3600||!Array.isArray(c.scene?.entities)||c.scene.entities.length!==c.agents)throw Error('Invalid/duplicate workload');
    names.add(c.name);
    if(!Array.isArray(c.samples)||c.samples.length<1||c.samples.length>64||c.samples.at(-1)?.tick!==c.ticks)throw Error('Missing/bounded final samples required');
    let previous=0;
    for(const sample of c.samples) {
      if(!sample||!Number.isInteger(sample.tick)||sample.tick<=previous||sample.tick>c.ticks||!Array.isArray(sample.nav_agents)||sample.nav_agents.length!==c.agents)throw Error('Invalid sample sequence or agent count');
      previous=sample.tick;
    }
  }
  return fixture;
}

export function runCases(module,fixture,{maxTickMs=Infinity,now=()=>performance.now()}={}) {
  validateFixture(fixture);
  if(!(maxTickMs>0)||!(Number.isFinite(maxTickMs)||maxTickMs===Infinity))throw Error('Tick budget must be positive');
  return fixture.cases.map(c=>{
    const game=new module.WebGame(),times=[];let sampleIndex=0;
    try {
      game.load(JSON.stringify(c.scene),'{}');
      for(let tick=1;tick<=c.ticks;tick++) {
        const start=now(),snapshot=JSON.parse(game.tick('{}'));times.push(now()-start);
        if(snapshot.tick!==tick)throw Error(`${c.name}: tick mismatch`);
        if(c.samples[sampleIndex]?.tick===tick) {compareSample(snapshot.nav_agents,c.samples[sampleIndex].nav_agents,`${c.name}@${tick}`);sampleIndex++;}
      }
      const first=times[0];times.sort((a,b)=>a-b);const max=times.at(-1);
      if(max>maxTickMs)throw Error(`${c.name}: tick + snapshot ${max.toFixed(3)} ms exceeds budget ${maxTickMs} ms`);
      return {name:c.name,agents:c.agents,ticks:c.ticks,samples:sampleIndex,wasm_tick_and_snapshot_ms:{first,p95:times[Math.floor((times.length-1)*.95)],max}};
    } finally {game.free();}
  });
}
