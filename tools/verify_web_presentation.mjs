import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const {chromium,devices}=await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const base=process.env.GAME_URL || 'http://127.0.0.1:8769';
const browser=await chromium.launch({executablePath:process.env.CHROME_PATH || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']});
const errors=[];
async function open(game,options={},fault) {
 const page=await browser.newPage(options);page.on('pageerror',error=>errors.push(error.message));
 if(fault)await page.addInitScript(fault=>{
  const prototype=WebGL2RenderingContext.prototype,parameter=prototype.getProgramParameter;
  prototype.getProgramInfoLog=prototype.getShaderInfoLog=()=>null;
  if(fault==='shader')prototype.getProgramParameter=function(program,key){return key===this.LINK_STATUS?false:parameter.call(this,program,key);};
 },fault);
 await page.goto(`${base}/${game}/`);
 return page;
}
async function start(page) {
 await page.getByRole('button',{name:'Spielen',exact:true}).click();
 await page.evaluate(async()=>{
  const {WebGame}=await import('./pkg/engine_web.js'),tick=WebGame.prototype.tick,load=WebGame.prototype.load;
  window.loads=0;
  WebGame.prototype.tick=function(input){const value=tick.call(this,input);window.snapshot=JSON.parse(value);return value;};
  WebGame.prototype.load=function(...args){window.loads++;const value=load.apply(this,args);window.snapshot=JSON.parse(value);return value;};
 });
 await page.waitForFunction(()=>window.snapshot?.tick>3);
}
try {
 for(const game of ['sky_garden','coin_game']) {
  for(const [name,options] of [['landscape',{...devices['iPhone 13 landscape'],deviceScaleFactor:1}],['ipad',{...devices['iPad (gen 7)'],deviceScaleFactor:1}]]) {
   const page=await open(game,options);await start(page);
   assert.match(await page.locator('#objective-text').innerText(),game==='sky_garden'?/Nächster Kristall.*m/:/Nächste Münze.*m/);
   assert.equal(await page.locator('#objective').isVisible(),true);
   if(game==='coin_game') {
    const angle=await page.locator('#objective-arrow').evaluate(element=>Number(element.style.transform.match(/rotate\((.*)rad\)/)[1]));
    assert.ok(angle>Math.PI/2&&angle<2.7,'coin direction must point diagonally from the player, not from the viewport centre');
   }
   if(name==='landscape') {
    const label=await page.locator('#joystick>span').boundingBox(),help=await page.locator('#help').boundingBox();
    assert.ok(label.y+label.height<=help.y || label.x+label.width<=help.x || help.x+help.width<=label.x,'joystick label overlaps help');
   }
   if(game==='coin_game'&&name==='landscape') {
    const cdp=await page.context().newCDPSession(page),pad=await page.locator('#joystick').boundingBox(),length=Math.hypot(7,9);
    const touch=(x,z)=>({x:pad.x+pad.width/2+x*pad.width*.33,y:pad.y+pad.height/2+z*pad.width*.33,id:1});
    await cdp.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[touch(9/length,7/length)]});
    await page.waitForFunction(()=>snapshot.state.score===1);
    await cdp.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
    await cdp.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[touch(-7/length,9/length)]});
    await page.waitForFunction(()=>snapshot.state._respawns===1);
    await cdp.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
    assert.equal(await page.evaluate(()=>snapshot.state.score),1);
    const position=await page.evaluate(()=>snapshot.transforms.find(e=>e.tags.includes('player')).transform.position);
    assert.ok(Math.hypot(position[0]+3,position[2])<.3,'ball must return to authored spawn');
    console.log('PASS coin_game: real touch off-platform respawn; collected score preserved');
   }
   await page.screenshot({path:path.join(root,`docs/polish_${game}_${name}_guidance.png`)});
   await page.getByRole('button',{name:'Spiel pausieren',exact:true}).click();
   await page.getByRole('button',{name:'Neustart',exact:true}).waitFor();
   if(name==='ipad')await page.screenshot({path:path.join(root,`docs/polish_${game}_pause.png`)});
   await page.getByRole('button',{name:'Neustart',exact:true}).click();
   await page.waitForFunction(()=>loads===1&&snapshot.state.score===0&&snapshot.tick>0);
   assert.equal(await page.locator('#modal').isHidden(),true);
   console.log(`PASS ${game} ${name}: objective direction, separate control labels, pause Neustart resets/resumes`);
   await page.close();
  }
 }
 const normal=await open('sky_garden',{viewport:{width:900,height:700}},'null');await start(normal);
 assert.equal(await normal.locator('#modal').isHidden(),true);
 await normal.screenshot({path:path.join(root,'docs/polish_null_shader_logs.png')});
 console.log('PASS null program/shader info logs: first render and gameplay continue');
 await normal.evaluate(async()=>{
  const [{GameEffects},THREE]=await Promise.all([import('./effects.js'),import('three')]);
  const renderer={shadowMap:{},domElement:document.createElement('canvas'),setPixelRatio(){},setSize(){}};
  const effects=new GameEffects(renderer,new THREE.Scene()),camera=new THREE.PerspectiveCamera();
  effects.level=0;renderer.shadowMap.needsUpdate=false;
  effects.update(1/60,new Map(),camera,false);
  if(renderer.shadowMap.needsUpdate)throw Error('low quality refreshed too soon');
  effects.update(1/60,new Map(),camera,false);
  if(!renderer.shadowMap.needsUpdate)throw Error('low quality shadow refresh missing');
  renderer.shadowMap.needsUpdate=false;effects.level=1;effects.update(1/60,new Map(),camera,false);
  if(!renderer.shadowMap.needsUpdate)throw Error('normal quality shadow refresh missing');
  const coin=new THREE.Group();coin.userData.tags=['coin'];coin.position.y=.55;
  coin.rotation.x=Math.PI/2;coin.scale.set(2,.5,3);coin.add(new THREE.Group());
  effects.update(1/60,new Map([[1,coin]]),camera,false);
  const bobbed=coin.children[0].getWorldPosition(new THREE.Vector3());
  if(Math.abs(bobbed.z)>1e-6||Math.abs(bobbed.y-.55)<.001)throw Error('rotated coin did not bob vertically');
  for(let i=0;i<8;i++)effects.burst(new THREE.Vector3(),true);
  if(effects.particles.length!==96)throw Error('particle budget exceeded');
  effects.reset();if(effects.points.geometry.drawRange.count!==0)throw Error('particle reset failed');
 });
 console.log('PASS adaptive shadow cadence, rotated collectible bob, bounded particles and empty-scene/reset effects');
 await normal.evaluate(()=>document.getElementById('game').getContext('webgl2').getExtension('WEBGL_lose_context').loseContext());
 await normal.getByRole('button',{name:'Seite neu laden',exact:true}).waitFor();
 assert.match(await normal.locator('#modal-text').innerText(),/Grafikverbindung.*neu/);
 const tick=await normal.evaluate(()=>snapshot.tick);await normal.waitForTimeout(150);
 assert.equal(await normal.evaluate(()=>snapshot.tick),tick);
 await normal.screenshot({path:path.join(root,'docs/polish_context_error.png')});
 await normal.getByRole('button',{name:'Seite neu laden',exact:true}).click();
 await normal.getByRole('button',{name:'Spielen',exact:true}).waitFor();
 console.log('PASS actual WebGL context loss: German error, stopped ticks, reload recovers');await normal.close();
 const broken=await open('sky_garden',{},'shader');
 await broken.getByRole('button',{name:'Seite neu laden',exact:true}).waitFor();
 assert.match(await broken.locator('#modal-text').innerText(),/Grafik.*neu/);
 console.log('PASS forced shader link failure: German error and enabled reload action');await broken.close();
 assert.deepEqual(errors,[]);
 console.log('PASS no unhandled browser exceptions');
} finally {await browser.close();}
