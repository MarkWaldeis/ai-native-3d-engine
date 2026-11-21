import { chromium, devices } from 'playwright';
import { createServer } from 'node:http';
import { readFile, realpath, stat, mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { parseArgs } from 'node:util';

export function options(args) {
  const { values } = parseArgs({ args, options: {
    site: {type:'string'}, 'scene-path': {type:'string',default:'large_world'},
    profiles: {type:'string',default:'desktop,mobile'}, duration: {type:'string',default:'20'},
    warmup: {type:'string',default:'3'}, out: {type:'string',default:'target/bench/results'},
    channel: {type:'string',default:'chromium'}, swiftshader: {type:'boolean',default:false},
    baseline: {type:'string'}, budgets: {type:'string'},
    timeout: {type:'string',default:'180'}, 'mobile-cpu-throttle': {type:'string',default:'4'},
  }});
  if (!values.site) throw Error('--site is required');
  for (const key of ['duration','warmup','timeout','mobile-cpu-throttle']) {
    values[key] = Number(values[key]);
    if (!Number.isFinite(values[key]) || values[key] < (key === 'warmup' ? 0 : key === 'mobile-cpu-throttle' ? 1 : .001)) throw Error(`Invalid --${key}`);
  }
  if (!['chrome','chromium'].includes(values.channel)) throw Error('Invalid --channel');
  values.profiles = values.profiles.split(',');
  if (!values.profiles.length || new Set(values.profiles).size !== values.profiles.length || values.profiles.some(p=>!['desktop','mobile'].includes(p))) throw Error('Invalid --profiles');
  if (!/^[\w/-]+$/.test(values['scene-path']) || values['scene-path'].split('/').some(p=>!p || p==='..')) throw Error('Invalid --scene-path');
  return values;
}

export function summarize(values) {
  if (!values.length) throw Error('No samples collected');
  const sorted = [...values].sort((a,b)=>a-b);
  const percentile = p => sorted[Math.ceil(p * sorted.length) - 1];
  return {count:values.length, meanMs:values.reduce((a,b)=>a+b,0)/values.length,
    p50Ms:percentile(.5), p95Ms:percentile(.95), p99Ms:percentile(.99), maxMs:sorted.at(-1)};
}

export function violations(profile, budget = {}) {
  const failures = [];
  if (profile.error) failures.push(profile.error);
  if (profile.pageErrors.length) failures.push(`${profile.pageErrors.length} page errors`);
  if (profile.consoleErrors.length) failures.push(`${profile.consoleErrors.length} console errors`);
  for (const [key,value,limit,direction] of [
    ['loadAssetsReadyMs',profile.loadAssetsReadyMs,budget.loadAssetsReadyMs,'max'],
    ['minAvgFps',profile.frames?.avgFps,budget.minAvgFps,'min'],
    ['maxJsHeapMB',profile.memory?.end?.jsHeapUsedSize / 1048576,budget.maxJsHeapMB,'max'],
  ]) {
    if (limit === undefined) continue;
    if (typeof limit !== 'number' || !Number.isFinite(limit) || limit < 0) throw Error(`Invalid budget ${key}`);
    if (!Number.isFinite(value) || (direction==='max' ? value>limit : value<limit)) failures.push(`${key}: ${value ?? 'missing'} (limit ${limit})`);
  }
  return failures;
}

export async function serve(directory) {
  const root = await realpath(directory);
  const mime = {'.js':'text/javascript','.mjs':'text/javascript','.wasm':'application/wasm','.json':'application/json','.html':'text/html','.css':'text/css','.glb':'model/gltf-binary','.png':'image/png'};
  const server = createServer(async (req,res)=>{
    try {
      const pathname = decodeURIComponent(new URL(req.url,'http://localhost').pathname);
      if (pathname==='/favicon.ico') {res.writeHead(204);res.end();return;}
      let file = path.resolve(root, `.${pathname}`);
      if ((await stat(file)).isDirectory()) file = path.join(file,'index.html');
      file = await realpath(file);
      if (!file.startsWith(root + path.sep)) {res.writeHead(403);res.end();return;}
      const body = await readFile(file);
      res.writeHead(200,{'Content-Type':mime[path.extname(file)] || 'application/octet-stream','Content-Length':body.length,'Cache-Control':'no-store'});
      res.end(body);
    } catch {res.writeHead(404);res.end('Not found');}
  });
  await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(0,'127.0.0.1',resolve);});
  return {url:`http://127.0.0.1:${server.address().port}`, close:()=>new Promise(resolve=>server.close(resolve))};
}

async function memory(page, cdp) {
  const {metrics} = await cdp.send('Performance.getMetrics');
  const metricsByName = Object.fromEntries(metrics.map(m=>[m.name,m.value]));
  const inPage = await page.evaluate(async()=>{
    const {default:init} = await import('./pkg/engine_web.js');
    const wasm = await init();
    return {performanceMemory:performance.memory ? {usedJSHeapSize:performance.memory.usedJSHeapSize,totalJSHeapSize:performance.memory.totalJSHeapSize,jsHeapSizeLimit:performance.memory.jsHeapSizeLimit}:null,
      wasmMemoryBytes:wasm.memory?.buffer.byteLength ?? null};
  });
  return {jsHeapUsedSize:metricsByName.JSHeapUsedSize,jsHeapTotalSize:metricsByName.JSHeapTotalSize,...inPage};
}

async function runProfile(browser, name, config, url, output) {
  const settings = name==='mobile' ? {...devices['iPhone 13'],viewport:{width:390,height:844},deviceScaleFactor:3} : {viewport:{width:1920,height:1080},deviceScaleFactor:1};
  const context = await browser.newContext(settings);
  const page = await context.newPage();
  const result = {profile:name, viewport:settings.viewport,deviceScaleFactor:settings.deviceScaleFactor,
    isMobile:!!settings.isMobile,hasTouch:!!settings.hasTouch,userAgent:settings.userAgent ?? await page.evaluate(()=>navigator.userAgent),
    cpuThrottleRate:name==='mobile'?config['mobile-cpu-throttle']:1,pageErrors:[],consoleErrors:[]};
  page.on('pageerror',error=>result.pageErrors.push(String(error)));
  page.on('console',message=>{if(message.type()==='error') result.consoleErrors.push(message.text());});
  page.setDefaultTimeout(config.timeout * 1000);
  // Closing the context also bounds operations blocked behind a busy renderer thread.
  const watchdog = setTimeout(()=>context.close().catch(()=>{}), (config.timeout + config.duration + config.warmup + 120)*1000);
  try {
    const cdp = await context.newCDPSession(page);
    await cdp.send('Performance.enable');
    await cdp.send('Emulation.setCPUThrottlingRate',{rate:result.cpuThrottleRate});
    await page.goto(`${url}/${config['scene-path']}/?bench`,{waitUntil:'domcontentloaded'});
    await page.waitForFunction(()=>window.__aiEngineBench?.ready || document.querySelector('#modal')?.dataset.screen==='error',null,{timeout:config.timeout*1000});
    if (!await page.evaluate(()=>window.__aiEngineBench?.ready)) throw Error(await page.locator('#modal-text').innerText());
    result.memory = {ready:await memory(page,cdp)};
    await page.evaluate(async()=>{
      const {WebGame} = await import('./pkg/engine_web.js');
      const original = WebGame.prototype.tick;
      const sample = window.__benchSample = {active:false,ticks:[],frames:[]};
      WebGame.prototype.tick = function(...args) {
        const start = performance.now();
        try {return original.apply(this,args);} finally {if(sample.active) sample.ticks.push(performance.now()-start);}
      };
    });
    const start = await page.evaluate(()=>performance.now());
    if(name==='mobile') await page.locator('#start').tap(); else await page.locator('#start').click();
    await page.waitForFunction(()=>window.__aiEngineBench.marks.firstFrameAfterStart!==undefined);
    result.startToFirstFrameMs = await page.evaluate(start=>__aiEngineBench.marks.firstFrameAfterStart-start,start);
    await page.waitForTimeout(config.warmup*1000);
    const samples = await page.evaluate(duration=>new Promise(resolve=>{
      const sample = window.__benchSample;
      let first,previous;
      function frame(now) {
        if (first===undefined) {first=now;sample.active=true;}
        if(previous!==undefined) sample.frames.push(now-previous);
        previous=now;
        if(now-first<duration*1000) requestAnimationFrame(frame);
        else {sample.active=false;resolve({frames:sample.frames,ticks:sample.ticks,elapsedMs:now-first});}
      }
      requestAnimationFrame(frame);
    }),config.duration);
    result.sampleElapsedMs = samples.elapsedMs;
    result.frames = {...summarize(samples.frames),avgFps:samples.frames.length/(samples.elapsedMs/1000),
      over33_3MsPercent:100*samples.frames.filter(v=>v>33.3).length/samples.frames.length,
      over50MsPercent:100*samples.frames.filter(v=>v>50).length/samples.frames.length};
    result.tick = samples.ticks.length ? summarize(samples.ticks) : null;
    result.memory.end = await memory(page,cdp);
    Object.assign(result,await page.evaluate(()=>{
      const marks = {...__aiEngineBench.marks};
      const navigation = performance.getEntriesByType('navigation')[0];
      const resources = performance.getEntriesByType('resource');
      const sum = entries=>entries.reduce((n,e)=>n+e.transferSize,0);
      const load = Object.fromEntries(['wasmInitialized','sceneFetched','webGameLoaded','assetsReady'].map(k=>[`${k}Ms`,marks[k]-marks.bootStart]));
      const stats=window.__engineStats;
      const terrain=window.__aiTerrain ? {loadedTiles:stats.terrainTilesLoaded,visibleTiles:stats.terrainTilesVisible,triangles:stats.terrainTriangles,bytesFetched:stats.terrainBytesFetched,pendingFetches:stats.terrainPendingFetches} : null;
      return {occlusion:{occludedChunks:stats.occludedChunks,occludedTerrainTiles:stats.occludedTerrainTiles,occlusionTests:stats.occlusionTests},marks,load,loadAssetsReadyMs:load.assetsReadyMs,renderer:__aiEngineBench.stats(),terrain,
        navigation:{ttfbMs:navigation.responseStart-navigation.requestStart,domContentLoadedMs:navigation.domContentLoadedEventEnd,
          transferredBytes:{scene:sum(resources.filter(e=>new URL(e.name).pathname.endsWith('/scene.json'))),wasm:sum(resources.filter(e=>new URL(e.name).pathname.endsWith('.wasm'))),total:sum(resources)+navigation.transferSize}}};
    }));
    result.screenshot = `${name}.png`;
    await page.screenshot({path:path.join(output,result.screenshot),timeout:config.timeout*1000});
  } catch(error) {result.error=String(error);}
  finally {clearTimeout(watchdog);await context.close();}
  return result;
}

export function summary(results, baseline) {
  const number = value=>Number.isFinite(value)?value.toFixed(2):'—';
  const delta = (value,old)=>Number.isFinite(value)&&Number.isFinite(old)?`${value-old>=0?'+':''}${(value-old).toFixed(2)}`:'—';
  const lines = ['# Browser benchmark','',`Browser: ${results.browserVersion}; channel: ${results.config.channel}; software rendering: ${results.config.swiftshader}.`,
    `Scene: ${results.scene.entities} entities (${results.scene.colliders} colliders). Frame sampling: ${results.config.duration}s after ${results.config.warmup}s warmup.`, '',
    `| Profile | CPU | Ready ms | Avg FPS | p50 / p95 / p99 ms | Max ms | >33.3 / >50 % | Tick avg ms | Heap ready / end MiB |${baseline?' Δ ready ms | Δ FPS | Δ heap MiB |':''}`,
    `|---|---:|---:|---:|---:|---:|---:|---:|---:|${baseline?'---:|---:|---:|':''}`];
  for(const [name,p] of Object.entries(results.profiles)) {
    const old=baseline?.profiles?.[name];
    lines.push(`| ${name} | ${p.cpuThrottleRate}× | ${number(p.loadAssetsReadyMs)} | ${number(p.frames?.avgFps)} | ${[p.frames?.p50Ms,p.frames?.p95Ms,p.frames?.p99Ms].map(number).join(' / ')} | ${number(p.frames?.maxMs)} | ${number(p.frames?.over33_3MsPercent)} / ${number(p.frames?.over50MsPercent)} | ${number(p.tick?.meanMs)} | ${number(p.memory?.ready?.jsHeapUsedSize/1048576)} / ${number(p.memory?.end?.jsHeapUsedSize/1048576)} |${baseline?` ${delta(p.loadAssetsReadyMs,old?.loadAssetsReadyMs)} | ${delta(p.frames?.avgFps,old?.frames?.avgFps)} | ${delta(p.memory?.end?.jsHeapUsedSize/1048576,old?.memory?.end?.jsHeapUsedSize/1048576)} |`:''}`);
  }
  lines.push('', '## Terrain', '', '| Profile | Loaded tiles (decoded LRU) | Visible tiles | Terrain triangles | Fetched bytes | Pending fetches |', '|---|---:|---:|---:|---:|---:|');
  for(const [name,p] of Object.entries(results.profiles)) {
    const t=p.terrain;
    lines.push(`| ${name} | ${number(t?.loadedTiles)} | ${number(t?.visibleTiles)} | ${number(t?.triangles)} | ${number(t?.bytesFetched)} | ${number(t?.pendingFetches)} |`);
  }
  lines.push('', '## Occlusion', '', '| Profile | Occluded chunks | Occluded terrain tiles | Tests/frame |', '|---|---:|---:|---:|');
  for(const [name,p] of Object.entries(results.profiles)) lines.push(`| ${name} | ${number(p.occlusion?.occludedChunks)} | ${number(p.occlusion?.occludedTerrainTiles)} | ${number(p.occlusion?.occlusionTests)} |`);
  lines.push('','Terrain counters are end-of-run snapshots; fetched bytes include manifest/coarse/tile payloads, not HTTP overhead. Missing terrain is shown as —.', '', 'Tick time includes the WASM simulation and snapshot serialization; heap excludes total process memory. Mobile uses Chromium with iPhone emulation, not Safari.', '', '## Violations',...(results.violations.length?results.violations.map(v=>`- ${v}`):['None.']),'');
  return lines.join('\n');
}

export async function main(args) {
  const config=options(args), output=path.resolve(config.out);
  const baseline=config.baseline?JSON.parse(await readFile(config.baseline,'utf8')):null;
  if(baseline && baseline.schemaVersion!==1) throw Error('Unsupported baseline schemaVersion');
  const budgets=config.budgets?JSON.parse(await readFile(config.budgets,'utf8')):{};
  const scene=JSON.parse(await readFile(path.join(config.site,config['scene-path'],'scene.json'),'utf8'));
  await mkdir(output,{recursive:true});
  let gitSha=null;
  try {gitSha=execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8',stdio:['ignore','pipe','ignore']}).trim();} catch {}
  const flags=config.swiftshader?['--use-angle=swiftshader','--enable-unsafe-swiftshader','--ignore-gpu-blocklist']:[];
  const results={schemaVersion:1,timestamp:new Date().toISOString(),gitSha,config,flags,
    scene:{name:scene.name,entities:scene.entities.length,colliders:scene.entities.filter(e=>e.collider).length,terrain:scene.entities.filter(e=>e.props?.ai_terrain).length,kinds:scene.entities.reduce((counts,e)=>(counts[e.kind]=(counts[e.kind]||0)+1,counts),{})},profiles:{},violations:[]};
  const server=await serve(config.site);
  let browser;
  try {
    browser=await chromium.launch({...(config.channel==='chrome'?{channel:'chrome'}:{}),headless:true,args:flags});
    results.browserVersion=browser.version();
    for(const name of config.profiles) {
      console.log(`Running ${name}…`);
      const profile=await runProfile(browser,name,config,server.url,output);
      profile.violations=violations(profile,budgets[name]);
      if(baseline) {
        const old=baseline.profiles?.[name];
        profile.baselineDelta={loadAssetsReadyMs:old&&Number.isFinite(profile.loadAssetsReadyMs)?profile.loadAssetsReadyMs-old.loadAssetsReadyMs:null,
          avgFps:old?.frames&&profile.frames?profile.frames.avgFps-old.frames.avgFps:null,
          jsHeapUsedMB:old?.memory?.end&&profile.memory?.end?(profile.memory.end.jsHeapUsedSize-old.memory.end.jsHeapUsedSize)/1048576:null};
      }
      results.profiles[name]=profile;
      results.violations.push(...profile.violations.map(v=>`${name}: ${v}`));
    }
  } catch(error) {results.violations.push(String(error));}
  finally {await browser?.close();await server.close();}
  await writeFile(path.join(output,'results.json'),JSON.stringify(results,null,2)+'\n');
  const markdown=summary(results,baseline);
  await writeFile(path.join(output,'summary.md'),markdown);
  console.log(markdown);
  return results.violations.length?1:0;
}

if(process.argv[1] && path.resolve(process.argv[1])===fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).then(code=>{process.exitCode=code;}).catch(error=>{console.error(error);process.exitCode=1;});
}
