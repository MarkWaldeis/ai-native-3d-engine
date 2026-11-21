import assert from 'node:assert/strict';
import path from 'node:path';

export function installClock() {
  let clock=1000,next=0;const frames=new Map();
  Object.defineProperty(performance,'now',{value:()=>clock});
  window.requestAnimationFrame=callback=>{frames.set(++next,callback);return next;};
  window.cancelAnimationFrame=id=>frames.delete(id);
  window.advanceFrames=count=>{for(let i=0;i<count;i++){clock+=1000/60+.00001;const batch=[...frames.values()];frames.clear();for(const callback of batch)callback(clock);}};
}

export async function verifyLantern(page, shots, mobile=false) {
  const name=mobile?'lantern-iphone-portrait':'lantern-desktop';
  const advance=count=>page.evaluate(n=>{advanceFrames(n);return snapshot;},count);
  let snapshot=await advance(2);
  await page.screenshot({path:path.join(shots,`${name}-start.png`)});
  assert.match(await page.locator('#state').innerText(),/Laternen\s+0\s*\/\s*4/);
  assert.match(await page.locator('#objective-text').innerText(),/Nächste Laterne/);
  await page.getByRole('button',{name:'Spiel pausieren',exact:true}).click();
  const tick=snapshot.tick;snapshot=await advance(30);assert.equal(snapshot.tick,tick);
  await page.getByRole('button',{name:'Neustart',exact:true}).click();
  await page.locator('#modal').waitFor({state:'hidden'});snapshot=await advance(2);
  const cdp=mobile?await page.context().newCDPSession(page):null;
  const pad=await page.locator('#joystick').boundingBox(),button=await page.locator('#action').boundingBox();
  let direction=[0,0],touching=false;
  async function move(x,z) {
    if(x===direction[0]&&z===direction[1])return;
    if(mobile) {
      const point={x:pad.x+pad.width/2+x*pad.width*.45,y:pad.y+pad.height/2+z*pad.width*.45,id:1};
      if(x||z){await cdp.send('Input.dispatchTouchEvent',{type:touching?'touchMove':'touchStart',touchPoints:[point]});touching=true;}
      else if(touching){await cdp.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});touching=false;}
    } else {
      for(const key of ['KeyW','KeyA','KeyS','KeyD'])await page.keyboard.up(key);
      if(x)await page.keyboard.down(x>0?'KeyD':'KeyA');
      if(z)await page.keyboard.down(z>0?'KeyS':'KeyW');
    }
    direction=[x,z];
  }
  const position=()=>snapshot.transforms.find(e=>e.tags.includes('player')).transform.position;
  async function walk(x,z) {
    for(let i=0;i<1200;i++) {
      const p=position(),dx=x-p[0],dz=z-p[2];
      if(Math.abs(dx)<.09&&Math.abs(dz)<.09){await move(0,0);snapshot=await advance(1);return;}
      if(Math.abs(dx)>.09)await move(Math.sign(dx),0);else await move(0,Math.sign(dz));
      snapshot=await advance(1);
    }
    throw new Error(`${name}: blocked walking to ${x},${z}: ${JSON.stringify(position())}`);
  }
  for(const [index,[x,z]] of [[-4.5,-3],[-.5,-3],[-.5,3],[2.5,3]].entries()) {
    await walk(x,z);
    assert.equal(snapshot.state.score,index,'approach without action must not light a lantern');
    if(mobile)await cdp.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{x:button.x+button.width/2,y:button.y+button.height/2,id:2}]});
    else await page.keyboard.down('Space');
    snapshot=await advance(2);
    if(mobile)await cdp.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});else await page.keyboard.up('Space');
    snapshot=await advance(1);
    assert.equal(snapshot.state.score,index+1);
    assert.equal(snapshot.state.gate_open,index===3);
    assert.equal(snapshot.transforms.some(e=>e.tags.includes('gate_bar')),index!==3);
    const lamps=snapshot.transforms.filter(e=>e.tags.includes('lit_lantern'));
    assert.equal(lamps.length,index+1);
    const lamp=lamps.find(e=>Math.hypot(e.transform.position[0]-x,e.transform.position[2]-z)<1);
    assert.ok(lamp&&Math.hypot(lamp.transform.position[0]-x,lamp.transform.position[2]-z)<1,'lit bulb stays at its fixture');
    if(index===0)await page.screenshot({path:path.join(shots,`${name}-lit.png`)});
  }
  await walk(2.5,-2);await walk(4.8,-2);
  await page.screenshot({path:path.join(shots,`${name}-open-gate.png`)});
  await move(0,-1);
  for(let i=0;i<160&&!snapshot.state.won;i++)snapshot=await advance(1);
  await move(0,0);
  assert.equal(snapshot.state.won,true);assert.equal(snapshot.state.score,4);assert.equal(snapshot.state._respawns,0);
  assert.equal(await page.locator('#modal-title').innerText(),'Gewonnen!');
  await page.screenshot({path:path.join(shots,`${name}-win.png`)});
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
  await page.getByRole('button',{name:'Nochmal spielen',exact:true}).click();
  await page.locator('#modal').waitFor({state:'hidden'});snapshot=await advance(2);
  assert.equal(snapshot.state.score,0);assert.equal(snapshot.state.won,false);assert.equal(snapshot.state.gate_open,false);
  assert.equal(snapshot.transforms.filter(e=>e.tags.includes('lantern')).length,4);
  assert.equal(snapshot.transforms.some(e=>e.tags.includes('gate_bar')),true);
  console.log(`PASS ${name}: ${mobile?'real touch joystick + Anzünden':'keyboard + Space'}, 4 lights at fixtures, gate closed until fourth action, touch exit win, pause-Neustart, win restart, no overflow`);
}

export async function verifyLanternTouch(browser,base,shots,errors) {
  const page=await browser.newPage({viewport:{width:390,height:844},isMobile:true,hasTouch:true,deviceScaleFactor:2});
  page.setDefaultTimeout(120000);
  page.on('pageerror',error=>errors.push(`lantern touch: ${error.message}`));
  page.on('response',response=>{if(response.status()>=400)errors.push(`${response.status()} ${response.url()}`);});
  await page.route('**/*',route=>new URL(route.request().url()).origin===base?route.continue():route.abort());
  await page.addInitScript(installClock);
  try {
    await page.goto(`${base}/lantern_maze/`);
    await page.waitForFunction(()=>document.getElementById('start')?.disabled===false,null,{polling:50});
    await page.evaluate(async()=>{
      const {WebGame}=await import('./pkg/engine_web.js');
      const transforms=new Map();
      for(const method of ['load','tick']){const original=WebGame.prototype[method];WebGame.prototype[method]=function(...args){
        const result=original.apply(this,args),snapshot=JSON.parse(result);
        if(method==='load')transforms.clear();
        for(const entity of snapshot.transforms)transforms.set(entity.id,entity);
        for(const id of snapshot.removed)transforms.delete(id);
        window.snapshot={...snapshot,transforms:[...transforms.values()]};
        return result;
      };}
    });
    await page.getByRole('button',{name:'Spielen',exact:true}).click();
    await verifyLantern(page,shots,true);
  } catch(error) {await page.screenshot({path:path.join(shots,'lantern-touch-failure.png')}).catch(()=>{});throw error;}
  finally {await page.close();}
}
