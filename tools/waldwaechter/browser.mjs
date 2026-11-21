import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import http from 'node:http';
import path from 'node:path';
import {installClock} from '../lantern_maze/browser.mjs';
import {compare} from './parity.mjs';

const {chromium,webkit,devices}=await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const site=path.resolve(process.argv[2] || 'target/showcase-site');
const native=JSON.parse(await fs.readFile(process.argv[3] || 'target/forest-native.json'));
const traces=JSON.parse(await fs.readFile(new URL('./traces.json',import.meta.url)));
assert.deepEqual(Object.keys(native).sort(),traces.map(t=>t.name).sort());
const shots=path.resolve(process.env.SHOTS_DIR || 'target/web-release-shots/waldwaechter');
await fs.mkdir(shots,{recursive:true});
const server=http.createServer(async(req,res)=>{
  try {
    let file=path.resolve(site,'.'+decodeURIComponent(new URL(req.url,'http://localhost').pathname));
    if(!file.startsWith(site+path.sep))throw Error('outside root');
    if((await fs.stat(file)).isDirectory())file=path.join(file,'index.html');
    res.setHeader('Content-Type',({'.wasm':'application/wasm','.js':'text/javascript','.json':'application/json','.html':'text/html','.css':'text/css','.svg':'image/svg+xml'})[path.extname(file)] || 'application/octet-stream');
    res.end(await fs.readFile(file));
  }catch{res.writeHead(404);res.end('Not found');}
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const base=`http://127.0.0.1:${server.address().port}`;
try {
  for(const engine of ['chromium','webkit']) {
    const browser=await (engine==='chromium'?chromium.launch({args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']}):webkit.launch()).catch(error=>{throw Error(`${engine} required; install pinned Playwright browsers; no silent skip: ${error.message}`);});
    try {
      for(const profile of engine==='chromium'?['desktop','iPhone 13']:['iPhone 13','iPad Pro 11']) {
        const name=`${engine}-${profile.replaceAll(' ','-')}`,errors=[];
        const context=await browser.newContext(profile==='desktop'?{viewport:{width:1100,height:800}}:{...devices[profile],deviceScaleFactor:1});
        const page=await context.newPage();page.setDefaultTimeout(120000);
        page.on('pageerror',e=>errors.push(e.message));
        page.on('response',r=>{if(r.status()>=400)errors.push(`${r.status()} ${r.url()}`);});
        await page.route('**/*',route=>new URL(route.request().url()).origin===base?route.continue():route.abort());
        await page.addInitScript(installClock);
        try {
          await page.goto(`${base}/waldwaechter/`);
          await page.waitForFunction(()=>document.getElementById('start')?.disabled===false,null,{polling:50});
          const actual=await page.evaluate(async traces=>{
            const {WebGame}=await import('./pkg/engine_web.js');
            const [scene,scripts]=await Promise.all(['scene.json','scripts.json'].map(p=>fetch(p).then(r=>r.json())));
            const summaries={};
            for(const trace of traces) {
              const game=new WebGame(),inputs={},transforms=new Map(),samples=[];let win_tick=null;
              function sample(s) {
                assertRuntime(s);
                for(const e of s.transforms)transforms.set(e.id,e);
                for(const id of s.removed)transforms.delete(id);
                if(s.state.won && win_tick===null)win_tick=s.tick;
                if(s.tick%30===0 || s.tick===trace.ticks) {
                  const actors=[...transforms.values()].filter(e=>e.tags.some(t=>t==='player'||t==='guard')).map(e=>({id:e.id,position:e.transform.position}));
                  const state=Object.fromEntries(['score','won','Erwischt','_checkpoint','_shield','_patrol_visits'].map(k=>[k,s.state[k]]));
                  samples.push({tick:s.tick,state,actors,animators:s.animators,nav_agents:s.nav_agents});
                }
              }
              function assertRuntime(s){if(s.failed)throw Error(`runtime failed at ${s.tick}`);}
              sample(JSON.parse(game.load(JSON.stringify(scene),JSON.stringify(scripts))));
              for(let tick=0;tick<trace.ticks;tick++) {
                for(const event of trace.inputs)if(event.tick===tick)inputs[event.action]=event.value;
                sample(JSON.parse(game.tick(JSON.stringify(inputs))));
              }
              game.free();summaries[trace.name]={win_tick,samples};
            }
            const transforms=new Map();
            for(const method of ['load','tick']) {
              const original=WebGame.prototype[method];
              WebGame.prototype[method]=function(...args){
                const result=original.apply(this,args),s=JSON.parse(result);
                if(method==='load')transforms.clear();
                for(const e of s.transforms)transforms.set(e.id,e);
                for(const id of s.removed)transforms.delete(id);
                window.snapshot={...s,transforms:[...transforms.values()]};return result;
              };
            }
            return summaries;
          },traces);
          compare(actual,native,name);
          console.log(`PASS forest parity ${name}: win, caught, checkpoint; actor positions, animation and full nav routes (${traces.reduce((n,t)=>n+Math.floor(t.ticks/30)+1+Number(t.ticks%30!==0),0)} samples)`);
          const start=page.getByRole('button',{name:'Spielen',exact:true});
          if(profile==='desktop')await start.click();else await start.tap();
          await page.locator('#modal').waitFor({state:'hidden'});
          const advance=async n=>{
            let result;
            for(let left=n;left>0;left-=10)result=await page.evaluate(count=>{
              advanceFrames(count);
              document.getElementById('game').getContext('webgl2')?.finish();
              return snapshot;
            },Math.min(left,10));
            return result;
          };
          let snapshot=await advance(2);
          assert.equal(snapshot.state.score,0);assert.equal(snapshot.state.won,false);
          await page.waitForFunction(()=>{
            advanceFrames(1);
            const s=window.__engineStats;
            return s.assetsLoaded>0 && s.assetsQueued===0 && s.assetsLoading===0 && s.textureUploadsPending===0;
          },null,{polling:50});
          snapshot=await advance(2);
          await page.screenshot({path:path.join(shots,`${name}-start.png`)});
          await page.getByRole('button',{name:'Spiel pausieren',exact:true}).click();
          const tick=snapshot.tick;assert.equal((await advance(10)).tick,tick);
          await page.getByRole('button',{name:'Neustart',exact:true}).click();
          await page.locator('#modal').waitFor({state:'hidden'});await advance(2);
          const mobile=profile!=='desktop';
          let cdp;
          if(mobile) {
            const pad=await page.locator('#joystick').boundingBox();
            const x=pad.x+pad.width/2,y=pad.y+pad.height/2;
            if(engine==='chromium') {
              cdp=await context.newCDPSession(page);
              await cdp.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{x,y,id:1}]});
              await cdp.send('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{x:x+pad.width*.46,y,id:1}]});
            } else {
              await page.mouse.move(x,y);await page.mouse.down();await page.mouse.move(x+pad.width*.46,y);
            }
          }else await page.keyboard.down('KeyD');
          snapshot=await advance(650);
          if(cdp)await cdp.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
          else if(mobile)await page.mouse.up();else await page.keyboard.up('KeyD');
          assert.equal(snapshot.state.score,5);assert.equal(snapshot.state.won,true);assert.equal(snapshot.state.Erwischt,0);
          assert.equal(await page.locator('#modal-title').innerText(),'Gewonnen!');
          assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
          await page.screenshot({path:path.join(shots,`${name}-win.png`)});
          await page.getByRole('button',{name:'Nochmal spielen',exact:true}).click();
          await page.locator('#modal').waitFor({state:'hidden'});snapshot=await advance(2);
          assert.equal(snapshot.state.score,0);assert.equal(snapshot.state.won,false);
          assert.equal(snapshot.transforms.filter(e=>e.tags.includes('relic')).length,5);
          assert.deepEqual(errors,[]);
          console.log(`PASS forest UI ${name}: ${!mobile?'keyboard':cdp?'real CDP touch joystick':'touch activation + trusted mouse-pointer joystick'}, victory, pause/restart, 5 relics restored; no page/HTTP errors`);
        }catch(error){await page.screenshot({path:path.join(shots,`${name}-FAIL.png`)}).catch(()=>{});throw error;}
        finally{await context.close();}
      }
    }finally{await browser.close();}
  }
}finally{await new Promise(resolve=>server.close(resolve));}
