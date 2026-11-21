import assert from 'node:assert/strict';
import path from 'node:path';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const base=process.env.GAME_URL || 'http://127.0.0.1:8783';
const shots=path.resolve(process.env.SHOTS_DIR || 'target/preview-shots');
const browser=await chromium.launch({args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']});
try {
 for(const game of ['sky_garden','coin_game']) for(const [device,width,height] of [['ipad',820,1180],['iphone',390,844]]) {
  const page=await browser.newPage({viewport:{width,height},hasTouch:true,isMobile:true});
  const errors=[];page.on('pageerror',error=>errors.push(error.message));
  await page.addInitScript(()=>{
   let now=1000;const queue=[];
   Object.defineProperty(performance,'now',{value:()=>now});
   window.requestAnimationFrame=callback=>{queue.push(callback);return queue.length;};
   window.advance=count=>{for(let i=0;i<count;i++){now+=1000/60+.00001;const callbacks=queue.splice(0);callbacks.forEach(callback=>callback(now));}};
  });
  await page.goto(`${base}/${game}/`);
  await page.waitForFunction(()=>document.getElementById('start').disabled===false,null,{polling:50});
  await page.evaluate(async()=>{
   const THREE=await import('./vendor/three.module.js');
   const update=THREE.PerspectiveCamera.prototype.updateProjectionMatrix;
   THREE.PerspectiveCamera.prototype.updateProjectionMatrix=function(){window.previewCamera=this;return update.call(this);};
   const {WebGame}=await import('./pkg/engine_web.js'),tick=WebGame.prototype.tick;
   window.recordedInputs=[];
   WebGame.prototype.tick=function(input){const value=tick.call(this,input);window.previewSnapshot=JSON.parse(value);recordedInputs.push(JSON.parse(input));return value;};
  });
  await page.getByRole('button',{name:'Spielen',exact:true}).click();
  await page.evaluate(()=>advance(2));
  assert.equal(await page.locator('#game').getAttribute('data-camera-mode'),'route');
  const projection=await page.evaluate(async()=>{
   const {Vector3}=await import('./vendor/three.module.js');
   const goal=previewSnapshot.transforms.find(entity=>entity.tags.some(tag=>['goal','portal'].includes(tag)));
   return new Vector3(...goal.transform.position).project(previewCamera).toArray();
  });
  assert.ok(Math.abs(projection[0])<.85 && Math.abs(projection[1])<.85 && Math.abs(projection[2])<1,`${game}/${device}: goal must be inside the view with margin: ${projection}`);
  await page.screenshot({path:path.join(shots,`${game}-${device}-route.png`)});
  await page.evaluate(()=>advance(240));
  assert.equal(await page.locator('#game').getAttribute('data-camera-mode'),'follow','preview must expire automatically');
  await page.getByRole('button',{name:'Spiel pausieren',exact:true}).click();
  await page.getByRole('button',{name:'Neustart',exact:true}).click();
  await page.locator('#modal').waitFor({state:'hidden'});
  await page.evaluate(()=>{recordedInputs.length=0;advance(2);});
  assert.equal(await page.locator('#game').getAttribute('data-camera-mode'),'route','restart must show the route again');
  if(device==='iphone')await page.locator('#action').tap();
  else await page.keyboard.down('KeyD');
  await page.evaluate(()=>advance(2));
  assert.equal(await page.locator('#game').getAttribute('data-camera-mode'),'follow','first input must skip preview');
  assert.ok(await page.evaluate(()=>recordedInputs.some(input=>input.action>0 || Math.hypot(input.move_x||0,input.move_z||0)>0)),'skipping must not consume the first gameplay input');
  assert.deepEqual(errors,[]);
  console.log(`PASS ${game}/${device}: goal visibly framed, 3.5s preview expires, restart previews, first ${device==='iphone'?'touch':'keyboard'} input skips without losing input`);
  await page.close();
 }
} finally {await browser.close();}
