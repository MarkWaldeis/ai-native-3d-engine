// Run against an exported bundle and a fresh --net-serve; needs Playwright and Chrome.
import assert from 'node:assert/strict';
import path from 'node:path';
import {mkdir,writeFile} from 'node:fs/promises';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const url=process.env.GAME_URL || 'http://127.0.0.1:8000/net_arena_ws/?net=ws://127.0.0.1:7790';
const shots=path.resolve(process.env.SHOTS_DIR || 'target/net-shots');
await mkdir(shots,{recursive:true});
const browser=await chromium.launch({channel:'chrome',args:['--use-angle=swiftshader','--enable-unsafe-swiftshader','--disable-background-timer-throttling','--disable-renderer-backgrounding']});
try {
  const pages=[],errors=[];
  for(let i=0;i<2;i++) {
    const context=await browser.newContext({viewport:{width:1100,height:760}});
    const page=await context.newPage();pages.push(page);
    page.on('pageerror',error=>errors.push(error.message));
    await page.goto(url);
    await page.waitForFunction(()=>window.__engineNet?.connected && !document.getElementById('start').disabled,null,{timeout:60000});
    await page.getByRole('button',{name:'Spielen',exact:true}).click();
  }
  for(const page of pages)await page.waitForFunction(()=>window.__engineNet?.players===2);
  const before=await Promise.all(pages.map(page=>page.evaluate(()=>structuredClone(window.__engineNet))));
  const [a,b]=before.map(state=>state.my_entity);
  assert.notEqual(a,b,'connections must own distinct slots');
  const startA=before[0].positions[a],startB=before[1].positions[b],crateBefore=before[0].positions[6];
  const key=startA[0]<0?'KeyD':'KeyA';
  await pages[0].keyboard.down(key);
  await pages[0].waitForFunction(({id,start,crate})=>Math.abs(window.__engineNet.positions[id][0]-start)>3.0 && Math.abs(window.__engineNet.positions[6][0]-crate)>0.25,{id:a,start:startA[0],crate:crateBefore[0]});
  await pages[0].keyboard.up(key);
  await pages[1].waitForFunction(({id,start})=>Math.abs(window.__engineNet.positions[id][0]-start)>2.8,{id:a,start:startA[0]});
  await pages[1].keyboard.down('KeyS');
  await pages[1].waitForFunction(({id,start})=>window.__engineNet.positions[id][2]-start>1,{id:b,start:startB[2]});
  await pages[1].keyboard.up('KeyS');
  await pages[0].waitForFunction(({id,start})=>window.__engineNet.positions[id][2]-start>0.8,{id:b,start:startB[2]});
  for(const page of pages)await page.waitForFunction(()=>window.__engineNet.spawned>=1 && window.__engineNet.removed>=1);
  const after=await Promise.all(pages.map(page=>page.evaluate(()=>structuredClone(window.__engineNet))));
  assert.ok(Math.abs(after[0].positions[6][0]-crateBefore[0])>0.1,'crate must be pushed authoritatively');
  for(const id of [a,b,6]) {
    const delta=Math.hypot(...after[0].positions[id].map((v,i)=>v-after[1].positions[id][i]));
    assert.ok(delta<0.35,`both tabs must see entity ${id} together, delta=${delta}`);
  }
  for(let i=0;i<pages.length;i++) {
    assert.match(await pages[i].locator('#state').innerText(),/Spieler \d+.*2 Spieler verbunden/);
    await pages[i].screenshot({path:path.join(shots,`net-tab-${i+1}.png`)});
  }
  assert.deepEqual(errors,[],'no browser/WASM/render exceptions');
  await writeFile(path.join(shots,'report.json'),JSON.stringify({before,after,errors},null,2));
  console.log(`PASS two exported browser tabs: distinct slots ${a}/${b}, bidirectional movement, pushed crate, replicated spawn/removal, HUD and no errors`);
  await pages[0].close();
  await pages[1].waitForFunction(()=>window.__engineNet.players===1);
  console.log('PASS disconnect removes connected player and frees native slot');
} finally {await browser.close();}
