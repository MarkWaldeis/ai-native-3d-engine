import {readFile} from 'node:fs/promises';
import {pathToFileURL} from 'node:url';
import path from 'node:path';
import assert from 'node:assert/strict';
import {digest,summary,validateReport} from './trace.mjs';
const [reportPath,gameDir]=process.argv.slice(2);
if(!reportPath||!gameDir)throw Error('Usage: node tools/device_qa/replay.mjs REPORT.json EXPORTED_GAME_DIR');
const report=JSON.parse(await readFile(reportPath,'utf8'));validateReport(report);
const root=path.resolve(gameDir),scene=await readFile(path.join(root,'scene.json'),'utf8'),scripts=await readFile(path.join(root,'scripts.json'),'utf8');
const assets={scene:digest(JSON.parse(scene)),scripts:digest(JSON.parse(scripts))};
assert.deepEqual(assets,report.metadata.assets,'Use the exact scene/scripts export recorded on device');
const module=await import(pathToFileURL(path.join(root,'pkg/engine_web.js')).href);
module.initSync({module:new WebAssembly.Module(await readFile(path.join(root,'pkg/engine_web_bg.wasm')))});
const runtime=new module.WebGame();runtime.load(scene,scripts);let ticks=0,last;
try {
  for(const call of report.trace.calls) {
    if(call.method==='load'){assert.deepEqual(call.assets,assets);last=summary(JSON.parse(runtime.load(scene,scripts)));}
    else if(call.method==='set_active_region'){assert.equal(call.args.length,4);assert.ok(call.args.every(Number.isFinite));runtime.set_active_region(...call.args);}
    else if(call.method==='clear_active_region')runtime.clear_active_region();
    else if(call.method==='tick') {
      const snapshot=JSON.parse(runtime.tick(JSON.stringify(call.input)));assert.equal(snapshot.tick,call.tick);
      last=summary(snapshot);if(call.sample)assert.deepEqual(last,call.sample,`Replay mismatch at tick ${call.tick}`);ticks++;
    }else throw Error('Unknown trace method');
  }
  assert.deepEqual(last,report.trace.last);
  console.log(`PASS device QA WASM replay: ${ticks} ticks, recorded gameplay/animator/navigation samples match`);
} finally{runtime.free();}
