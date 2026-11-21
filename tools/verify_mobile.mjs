import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const {chromium, devices} = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const shots = path.resolve(process.env.SHOTS_DIR || path.join(root,'docs'));
fs.mkdirSync(shots,{recursive:true});
const base = process.env.GAME_URL || 'http://127.0.0.1:8769';
const browser = await chromium.launch({executablePath:process.env.CHROME_PATH || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']});
const errors = [], traces = {};
try {
 for (const [name, options, game] of [
  ['desktop',{viewport:{width:1280,height:800}},'sky_garden'],
  ['iphone_portrait',{...devices['iPhone 13'],deviceScaleFactor:2},'sky_garden'],
  ['iphone_landscape',{...devices['iPhone 13 landscape'],deviceScaleFactor:2},'sky_garden'],
  ['coin_iphone',{...devices['iPhone 13'],deviceScaleFactor:2},'coin_game'],
 ]) {
  const page = await browser.newPage(options);
  page.on('pageerror', error => errors.push(error.message));
  await page.route('**/*', route => new URL(route.request().url()).origin === new URL(base).origin ? route.continue() : route.abort());
  await page.goto(`${base}/${game}/`); await page.getByRole('button',{name:'Spielen',exact:true}).waitFor();
  const trace = traces[name] = [];
  await page.evaluate(async () => {
   const {WebGame} = await import('./pkg/engine_web.js'); const tick = WebGame.prototype.tick;
   window.inputTrace = [];
   WebGame.prototype.tick = function(inputs) {
    const value = tick.call(this, inputs); window.snapshot = JSON.parse(value);
    inputTrace.push({tick:snapshot.tick, input:JSON.parse(inputs), p:snapshot.transforms.find(e=>e.tags.includes('player')).transform.position, state:snapshot.state});
    if (inputTrace.length > 1000) inputTrace.shift();
    return value;
   };
  });
  await page.getByRole('button',{name:'Spielen',exact:true}).click(); await page.waitForFunction(()=>window.snapshot?.tick>6);
  const cdp = await page.context().newCDPSession(page), mobile = name !== 'desktop';
  const pad = await page.locator('#joystick').boundingBox(), button = await page.locator('#action').boundingBox();
  const joy = (x,z) => ({x:pad.x+pad.width/2+x*pad.width*.33,y:pad.y+pad.height/2+z*pad.width*.33,id:1});
  const action = {x:button.x+button.width/2,y:button.y+button.height/2,id:2};
  let touching = false;
  async function stop() { if(mobile){if(touching)await cdp.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});touching=false;}else{await page.keyboard.up('KeyD');await page.keyboard.up('KeyS');} }
  async function move(x,z) {
   await stop();
   if(mobile){await cdp.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[joy(x,z)]});touching=true;}
   else {if(x)await page.keyboard.down('KeyD');if(z)await page.keyboard.down('KeyS');}
  }
  async function jump() {
   trace.push({requested:await page.evaluate(()=>({tick:snapshot.tick,p:snapshot.transforms.find(e=>e.tags.includes('player')).transform.position,state:snapshot.state}))});
   if(mobile) await cdp.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[joy(1,0),action]}); else await page.keyboard.down('Space');
   const tick=await page.evaluate(()=>snapshot.tick);await page.waitForFunction(t=>snapshot.tick>=t+2,tick);
   if(mobile) await cdp.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[action]});else await page.keyboard.up('Space');
   trace.push({released:await page.evaluate(()=>({tick:snapshot.tick,p:snapshot.transforms.find(e=>e.tags.includes('player')).transform.position,state:snapshot.state}))});
  }
  async function screenshot(suffix='') {await page.screenshot({path:path.join(shots,`mobile_${name}${suffix}.png`)});}
  try {
   await screenshot();
   if(game === 'sky_garden') {
    await move(0,1);const tick=await page.evaluate(()=>snapshot.tick);await page.waitForFunction(t=>snapshot.tick>=t+65,tick);await stop();
    await page.waitForFunction(()=>snapshot.state.Rettungen===1&&snapshot.state._coyote>0,null,{timeout:20000});
    await screenshot('_respawn');await move(1,0);
    for(const [i,x] of [.55,4.05,7.60].entries()) {
     await page.waitForFunction(x=>snapshot.transforms.find(e=>e.tags.includes('player')).transform.position[0]>x&&snapshot.state._coyote>0,x,{timeout:20000});
     await jump();if(i===0)await screenshot('_jump');
    }
   } else {
    const length=Math.hypot(7,9);await move(9/length,7/length);
   }
   await page.waitForFunction(()=>snapshot.state.won===true,null,{timeout:20000});await stop();
   const state=await page.evaluate(()=>snapshot.state);
   if(state.score!==3 || (game==='sky_garden' && state['Sprünge']!==3))throw new Error(JSON.stringify(state));
   if(await page.locator('#modal-title').innerText()!=='Gewonnen!')throw new Error('win overlay missing');
   await screenshot('_win');
   if(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth))throw new Error('page overflow');
   await page.getByRole('button',{name:'Nochmal spielen'}).click();await page.waitForFunction(()=>snapshot.state.score===0&&!snapshot.state.won);
   console.log(`PASS ${name}: ${mobile?'real touch joystick + action':'keyboard'}, ${game==='sky_garden'?'3 jumps, fall/respawn, ':''}score=3, Gewonnen!, restart, no overflow`);
  } catch(error) {
   console.error(name,JSON.stringify(await page.evaluate(()=>snapshot)));
   await screenshot('_failure');throw error;
  } finally {
   const ticks=await page.evaluate(()=>inputTrace);
   trace.push({actionTicks:ticks.filter(t=>t.input.action>0),last:ticks.at(-1)});
   fs.writeFileSync(path.join(shots,'mobile_input_trace_after.json'),JSON.stringify(traces,null,2)+'\n');
   await page.close();
  }
 }
 if(errors.length)throw new Error(errors.join('\n'));
 console.log('PASS no browser exceptions; external requests blocked');
} finally {await browser.close();}
