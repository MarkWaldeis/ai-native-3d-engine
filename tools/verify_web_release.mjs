import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import http from 'node:http';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
const {chromium} = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const site = path.resolve(process.argv[2] || 'target/m9-site');
const native = JSON.parse(await fs.readFile(process.argv[3] || 'target/web-native.json', 'utf8'));
const traces = JSON.parse(await fs.readFile(path.join(repo, 'tools/web_traces.json'), 'utf8'));
assert.equal(new Set(traces.map(trace=>trace.name)).size,traces.length,'trace names must be unique');
assert.deepEqual([...new Set(traces.map(trace=>trace.game))].sort(),['coin_game','sky_garden'],'both demos need parity traces');
assert.deepEqual(Object.keys(native).sort(),traces.map(trace=>trace.name).sort(),'native reference must cover exactly these traces');
for(const game of ['sky_garden','coin_game']) {
  assert.ok(traces.some(t=>t.game===game && t.expected.won===true),`${game}: missing victory trace`);
  assert.ok(traces.some(t=>t.game===game && (t.expected.Rettungen>0 || t.expected._respawns>0)),`${game}: missing rescue trace`);
  for(const trace of traces.filter(t=>t.game===game)) for(const key of ['score','won',game==='sky_garden'?'_checkpoint':'_respawns']) assert.ok(trace.keys.includes(key),`${trace.name}: missing parity field ${key}`);
}
const shots = path.resolve(process.env.SHOTS_DIR || 'target/web-release-shots');
await fs.mkdir(shots, {recursive:true});
const server = http.createServer(async (req,res) => {
  try {
    let file = path.resolve(site, '.' + decodeURIComponent(new URL(req.url, 'http://localhost').pathname));
    if (!file.startsWith(site + path.sep)) throw new Error('outside site');
    if ((await fs.stat(file)).isDirectory()) file = path.join(file, 'index.html');
    res.setHeader('Content-Type', ({'.wasm':'application/wasm','.js':'text/javascript','.json':'application/json','.html':'text/html','.css':'text/css','.glb':'model/gltf-binary'})[path.extname(file)] || 'application/octet-stream');
    res.end(await fs.readFile(file));
  } catch { res.writeHead(404); res.end('Not found'); }
});
await new Promise(resolve => server.listen(0,'127.0.0.1',resolve));
const base = `http://127.0.0.1:${server.address().port}`;
let browser;
const errors = [];
try {
  browser = await chromium.launch({...(process.env.CHROME_PATH ? {executablePath:process.env.CHROME_PATH} : {}),args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']});
  for (const game of ['sky_garden','coin_game']) {
    const page = await browser.newPage({viewport:{width:1100,height:800}});
    page.setDefaultTimeout(120000);
    page.on('pageerror', error => errors.push(`${game}: ${error.message}`));
    page.on('response', response => {if (response.status()>=400) errors.push(`${response.status()} ${response.url()}`);});
    await page.route('**/*', route => new URL(route.request().url()).origin===base ? route.continue() : route.abort());
    // Only control frame scheduling: keyboard events still traverse the real UI,
    // camera-relative controls, fixed-step accumulator and exported WASM runtime.
    await page.addInitScript(() => {
      let clock = 1000, next = 0; const frames = new Map();
      Object.defineProperty(performance,'now',{value:()=>clock});
      window.requestAnimationFrame = callback => {frames.set(++next,callback);return next;};
      window.cancelAnimationFrame = id => frames.delete(id);
      window.advanceFrames = count => {for(let i=0;i<count;i++){clock+=1000/60+0.00001;const batch=[...frames.values()];frames.clear();for(const callback of batch)callback(clock);}};
    });
    try {
      await page.goto(`${base}/${game}/`);
      await page.waitForFunction(()=>document.getElementById('start')?.disabled===false,null,{polling:50});
      const parity = await page.evaluate(async traces => {
        const {WebGame} = await import('./pkg/engine_web.js');
        const [scene,scripts] = await Promise.all(['scene.json','scripts.json'].map(file=>fetch(file).then(r=>r.json())));
        const summaries = {};
        for (const trace of traces) {
          const runtime = new WebGame(), inputs = {}, samples = []; let win_tick = null;
          function sample(snapshot) {
            if (snapshot.state.won && win_tick===null) win_tick=snapshot.tick;
            if (snapshot.tick%30===0 || snapshot.tick===trace.ticks) samples.push({tick:snapshot.tick,state:Object.fromEntries(trace.keys.map(key=>[key,snapshot.state[key]??null]))});
          }
          sample(JSON.parse(runtime.load(JSON.stringify(scene),JSON.stringify(scripts))));
          for (let tick=0;tick<trace.ticks;tick++) {
            for (const event of trace.inputs) if(event.tick===tick) inputs[event.action]=event.value;
            sample(JSON.parse(runtime.tick(JSON.stringify(inputs))));
          }
          runtime.free(); summaries[trace.name]={win_tick,checkpoints:samples};
        }
        const tick = WebGame.prototype.tick, load = WebGame.prototype.load;
        WebGame.prototype.tick = function(...args) {const result=tick.apply(this,args);window.snapshot=JSON.parse(result);return result;};
        WebGame.prototype.load = function(...args) {const result=load.apply(this,args);window.snapshot=JSON.parse(result);return result;};
        return summaries;
      },traces.filter(trace=>trace.game===game));
      for(const [name,result] of Object.entries(parity)) {
        assert.deepEqual(result,native[name],`${name}: native simulate / exported WASM mismatch`);
        console.log(`PASS parity ${name}: win_tick=${result.win_tick}; ${result.checkpoints.length} score/checkpoint/rescue samples match native simulate`);
      }
      await page.getByRole('button',{name:'Spielen',exact:true}).click();
      const advance = count => page.evaluate(n=>{advanceFrames(n);return snapshot;},count);
      let snapshot = await advance(2);
      async function until(predicate,label,max=900) {
        for(let i=0;i<max;i++) {if(predicate(snapshot))return;snapshot=await advance(1);}
        throw new Error(`${game}: ${label} timed out after ${max} simulated ticks: ${JSON.stringify(snapshot)}`);
      }
      const position = () => snapshot.transforms.find(e=>e.tags.includes('player')).transform.position;
      await page.keyboard.down('KeyS');
      await until(s=>game==='sky_garden' ? s.state.Rettungen===1 : s.state._respawns===1,'off-platform rescue');
      await page.keyboard.up('KeyS');
      assert.equal(snapshot.state.score,0);
      const rescued = position();
      assert.ok(Math.abs(rescued[0]-(game==='sky_garden'?0:-3))<.1 && Math.abs(rescued[2])<.1 && rescued[1]>0,'rescue must actually return the player to spawn, not only increment a counter');
      await page.getByRole('button',{name:'Spiel pausieren',exact:true}).click();
      const paused = snapshot.tick;
      snapshot = await advance(30); assert.equal(snapshot.tick,paused,'pause must stop ticks');
      await page.getByRole('button',{name:'Neustart',exact:true}).click();
      await page.locator('#modal').waitFor({state:'hidden'});
      snapshot = await advance(2);
      assert.equal(snapshot.state.score,0); assert.equal(snapshot.state.won,false);
      assert.equal(game==='sky_garden' ? snapshot.state.Rettungen : snapshot.state._respawns,0);
      assert.equal(await page.locator('#modal').isHidden(),true);
      if(game==='sky_garden') {
        await page.keyboard.down('KeyD');
        for(const x of [.55,4.05,7.60]) {
          await until(s=>position()[0]>x && s.state._coyote>0,`jump at x=${x}`);
          await page.keyboard.down('Space'); snapshot=await advance(2); await page.keyboard.up('Space');
        }
        await until(s=>s.state.won===true,'keyboard victory');
        await page.keyboard.up('KeyD');
      } else {
        await page.keyboard.down('KeyD'); let diagonal=false;
        for(let i=0;i<900 && !snapshot.state.won;i++) {
          const needDiagonal=position()[2]<.02;
          if(needDiagonal!==diagonal){await page.keyboard[needDiagonal?'down':'up']('KeyS');diagonal=needDiagonal;}
          snapshot=await advance(1);
        }
        await page.keyboard.up('KeyD'); await page.keyboard.up('KeyS');
        assert.equal(snapshot.state.won,true,'camera-relative keyboard victory');
      }
      assert.equal(snapshot.state.score,3);
      assert.equal(await page.locator('#modal-title').innerText(),'Gewonnen!');
      await page.screenshot({path:path.join(shots,`${game}-win.png`)});
      await page.getByRole('button',{name:'Nochmal spielen',exact:true}).click();
      await page.locator('#modal').waitFor({state:'hidden'}); snapshot=await advance(2);
      assert.equal(snapshot.state.score,0); assert.equal(snapshot.state.won,false);
      assert.ok(snapshot.tick>0,'win restart must resume simulation');
      assert.equal(await page.locator('#modal').isHidden(),true,'win restart must not show a graphics error');
      console.log(`PASS player ${game}: actual exported WASM, keyboard win, off-platform rescue, pause freezes ticks, pause-Neustart and win restart`);
    } catch(error) {
      await page.screenshot({path:path.join(shots,`${game}-failure.png`)}).catch(()=>{}); throw error;
    } finally {await page.close();}
  }
  assert.deepEqual(errors,[],'browser exceptions or HTTP failures');
  console.log('PASS release browser: no page errors or failed asset responses; external requests blocked');
} finally {await browser?.close();await new Promise(resolve=>server.close(resolve));}
