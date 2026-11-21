import {chromium} from 'playwright';
import assert from 'node:assert/strict';
import {mkdir, writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';

const output = resolve('target/terrain_shots');
await mkdir(output, {recursive:true});
const browser = await chromium.launch({channel:'chrome', headless:true, args:['--use-angle=swiftshader','--enable-unsafe-swiftshader','--ignore-gpu-blocklist']});
const results = [];
try {
  for (const [name, site] of [['origin',process.argv[2]],['far',process.argv[3]]]) {
    const page=await browser.newPage({viewport:{width:1280,height:800},deviceScaleFactor:1});
    // Python's static server has no site-wide browser favicon.
    await page.route('**/favicon.ico',route=>route.fulfill({status:204}));
    const errors=[], tiles=[], responses=[];
    page.on('pageerror',error=>errors.push(error.message));
    page.on('console',message=>{if(message.type()==='error') errors.push(message.text());});
    page.on('response',response=>{
      if(response.url().includes('/tiles/')) responses.push(response.body().then(bytes=>tiles.push({url:response.url(),bytes:bytes.length,status:response.status()})));
    });
    await page.goto(`http://127.0.0.1:8000/${site}/?bench&stats=1`);
    await page.waitForFunction(()=>window.__aiEngineBench?.ready,{},{timeout:120000});
    const initial=await page.evaluate(()=>window.__aiEngineBench.stats().simTicks);
    assert.equal(initial,0,'preload must complete before simulation starts');
    await page.locator('#start').click();
    await page.waitForTimeout(3000);
    const state=await page.evaluate(()=>{
      const stats=structuredClone(window.__engineStats),p=stats.playerPosition;
      return {stats,ground:window.__aiTerrain.physicsHeightAt(p[0],p[2]),renderHeight:window.__aiTerrain.heightAt(p[0],p[2]),ticks:window.__aiEngineBench.stats().simTicks};
    });
    assert.ok(Math.abs(state.stats.playerPosition[1]-state.ground-0.9)<0.08,JSON.stringify(state));
    assert.ok(Math.abs(state.renderHeight-state.ground)<0.005);
    await page.waitForFunction(()=>document.getElementById('game').dataset.cameraMode==='follow');
    await page.waitForTimeout(2000);
    const offsets=[];
    for(let i=0;i<8;i++) {
      await page.waitForTimeout(100);
      offsets.push(await page.evaluate(()=>window.__engineStats.cameraPosition.map((v,i)=>v-window.__engineStats.playerPosition[i])));
    }
    const jitter=Math.max(...[0,1,2].map(axis=>Math.max(...offsets.map(p=>p[axis]))-Math.min(...offsets.map(p=>p[axis]))));
    assert.ok(jitter<0.03,`unstable camera-relative position: ${jitter} m`);
    await page.screenshot({path:`${output}/${name}.png`});
    await Promise.all(responses);
    assert.ok(tiles.length>0); assert.ok(tiles.every(t=>t.status===200 && t.bytes<=40000));
    assert.deepEqual(errors,[]);
    results.push({name,...state,cameraRelativeRange:jitter,tileRequests:tiles.length,tileBytes:[...new Set(tiles.map(t=>t.bytes))],errors,screenshot:`${output}/${name}.png`});
    await page.close();
  }
  console.log(JSON.stringify(results,null,2));
  await writeFile(`${output}/results.json`,JSON.stringify(results,null,2));
} finally {await browser.close();}
