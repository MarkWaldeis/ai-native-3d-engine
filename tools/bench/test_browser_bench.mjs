import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,writeFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {options,summarize,violations,serve,summary} from './run_browser_bench.mjs';

test('options validate profiles, timing, channel and mobile throttling',()=>{
  const config=options(['--site','fixture']);
  assert.deepEqual(config.profiles,['desktop','mobile']);
  assert.equal(config['mobile-cpu-throttle'],4);
  assert.equal(config.channel,'chromium');
  for(const args of [[],['--site','x','--duration','0'],['--site','x','--warmup','NaN'],['--site','x','--profiles',''],['--site','x','--profiles','desktop,desktop'],['--site','x','--channel','safari'],['--site','x','--scene-path','../outside'],['--site','x','--mobile-cpu-throttle','.5']]) assert.throws(()=>options(args));
  assert.equal(options(['--site','x','--warmup','0','--channel','chrome','--swiftshader'])['swiftshader'],true);
});

test('frame statistics use nearest-rank percentiles and reject empty samples',()=>{
  assert.deepEqual(summarize([40,10,30,20]),{count:4,meanMs:25,p50Ms:20,p95Ms:40,p99Ms:40,maxMs:40});
  assert.throws(()=>summarize([]),/No samples/);
});

test('budget checks fail closed on missing measurements, errors and excessive values',()=>{
  const profile={loadAssetsReadyMs:100,frames:{avgFps:2},memory:{end:{jsHeapUsedSize:1048576}},pageErrors:[],consoleErrors:[]};
  assert.deepEqual(violations(profile),[]);
  assert.deepEqual(violations(profile,{loadAssetsReadyMs:100,minAvgFps:2,maxJsHeapMB:1}),[]);
  assert.equal(violations(profile,{loadAssetsReadyMs:99,minAvgFps:3,maxJsHeapMB:.5}).length,3);
  assert.equal(violations({...profile,error:'timeout',pageErrors:['broken'],consoleErrors:['failed'],frames:null},{minAvgFps:1}).length,4);
  assert.throws(()=>violations(profile,{minAvgFps:'1'}),/Invalid budget/);
});

test('summary includes baseline deltas and missing-profile markers',()=>{
  const profile={cpuThrottleRate:1,loadAssetsReadyMs:200,frames:{avgFps:2},memory:{end:{jsHeapUsedSize:2097152}}};
  const results={browserVersion:'test',config:{channel:'chrome',swiftshader:true,duration:20,warmup:3},scene:{entities:4,colliders:1},profiles:{desktop:profile},violations:[]};
  assert.match(summary(results),/None\./);
  const report=summary(results,{profiles:{desktop:{loadAssetsReadyMs:100,frames:{avgFps:1},memory:{end:{jsHeapUsedSize:1048576}}}}});
  assert.match(report,/Δ ready ms/);
  assert.match(report,/\+100\.00 \| \+1\.00 \| \+1\.00/);
  assert.match(summary(results,{profiles:{}}),/—/);
});

test('static server handles MIME types, missing files and favicon without console errors',async()=>{
  const directory=await mkdtemp(path.join(tmpdir(),'engine-bench-'));
  let server;
  try {
    await writeFile(path.join(directory,'index.html'),'ok');
    await writeFile(path.join(directory,'game.wasm'),Buffer.from([0,97,115,109]));
    await writeFile(path.join(directory,'game.js'),'export {};');
    server=await serve(directory);
    assert.equal(await (await fetch(server.url)).text(),'ok');
    assert.equal((await fetch(server.url+'/game.wasm')).headers.get('content-type'),'application/wasm');
    assert.equal((await fetch(server.url+'/game.js')).headers.get('content-type'),'text/javascript');
    assert.equal((await fetch(server.url+'/missing')).status,404);
    assert.equal((await fetch(server.url+'/favicon.ico')).status,204);
  } finally {await server?.close();await rm(directory,{recursive:true,force:true});}
});
