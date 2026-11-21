import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import http from 'node:http';
import {execFileSync} from 'node:child_process';
import {installClock} from './lantern_maze/browser.mjs';
const {chromium,webkit,devices}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
if(process.env.BROWSER_ENGINE&&!['chromium','webkit'].includes(process.env.BROWSER_ENGINE))throw Error('BROWSER_ENGINE must be chromium or webkit');
const site=path.resolve(process.argv[2]||'target/m9-site'),shots=path.resolve(process.env.SHOTS_DIR||'target/device-qa-shots');
await fs.mkdir(shots,{recursive:true});
const server=http.createServer(async(req,res)=>{
  try {
    let file=path.resolve(site,'.'+decodeURIComponent(new URL(req.url,'http://localhost').pathname));
    if(!file.startsWith(site+path.sep))throw Error('Outside site');
    if((await fs.stat(file)).isDirectory())file=path.join(file,'index.html');
    res.setHeader('Content-Type',({'.html':'text/html','.js':'text/javascript','.mjs':'text/javascript','.css':'text/css','.json':'application/json','.wasm':'application/wasm','.svg':'image/svg+xml'})[path.extname(file)]||'application/octet-stream');res.end(await fs.readFile(file));
  }catch{res.writeHead(404);res.end();}
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const base=`http://127.0.0.1:${server.address().port}`;
try {
  for(const [engine,device] of [['chromium','iPhone 13'],['webkit','iPhone 13'],['webkit','iPad Pro 11']]) {
    if(process.env.BROWSER_ENGINE&&engine!==process.env.BROWSER_ENGINE)continue;
    const browser=await (engine==='webkit'?webkit.launch():chromium.launch({args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']}));
    try {
      const page=await browser.newPage({...devices[device],deviceScaleFactor:1}),errors=[];
      page.setDefaultTimeout(120000);page.on('pageerror',e=>errors.push(e.message));page.on('console',message=>{if(message.type()==='error')errors.push(message.text());});
      await page.addInitScript(installClock);
      await page.goto(`${base}/device-qa/?game=../coin_game/`);
      await page.waitForFunction(()=>document.getElementById('status').textContent.includes('Recorder bereit'),null,{timeout:30000,polling:50}).catch(async error=>{console.error(await page.locator('#status').innerText(),errors,page.frames().map(f=>f.url()));throw error;});
      await page.locator('#toggle').click();
      const frame=page.frames().find(f=>f.parentFrame()===page.mainFrame());assert.ok(frame);
      assert.deepEqual(await frame.evaluate(()=>[innerWidth,innerHeight]),await page.evaluate(()=>[innerWidth,innerHeight]),'QA keeps the full game viewport');
      await frame.locator('#start:not([disabled])').waitFor({timeout:30000}).catch(async error=>{console.error(await frame.locator('body').innerText(),errors);throw error;});await frame.locator('#start').tap();
      await frame.locator('#modal').waitFor({state:'hidden'});
      await frame.evaluate(()=>{document.body.tabIndex=-1;document.body.focus();});
      await frame.evaluate(()=>advanceFrames(2));await page.keyboard.down('KeyD');await frame.evaluate(()=>advanceFrames(240));await page.keyboard.up('KeyD');
      assert.equal(await frame.locator('#modal-title').innerText(),'Gewonnen!','QA instrumentation must not break keyboard victory');
      await frame.getByRole('button',{name:'Nochmal spielen',exact:true}).tap();await frame.locator('#modal').waitFor({state:'hidden'});
      await frame.evaluate(()=>advanceFrames(2));
      const name=`${engine}-${device.replaceAll(' ','-')}`;
      await page.screenshot({path:path.join(shots,`${name}-game.png`)});
      await page.locator('#toggle').click();
      await page.locator('#stop').click();await page.locator('#download').waitFor({state:'visible',timeout:30000}).catch(async error=>{console.error({status:await page.locator('#status').innerText(),errors});throw error;});
      await page.locator('summary').click();await page.locator('#firstTap').selectOption('pass');await page.locator('#notes').fill('Automatisierter Browser-Test, kein physisches Gerät.');await page.locator('#notes').blur();
      const report=await page.locator('#download').evaluate(async a=>JSON.parse(await (await fetch(a.href)).text()));
      assert.equal(report.trace.complete,true);assert.equal(report.owner.physicalDeviceAttested,false);assert.equal(report.owner.checklist.firstTap,'pass');assert.equal(report.owner.checklist.touchWin,'not_checked');assert.ok(report.trace.calls.some(c=>c.method==='load'),'restart must be recorded');assert.ok(report.trace.calls.some(c=>c.sample?.state.won));assert.equal(report.trace.last.state.won,false);
      const file=path.join(shots,`${name}.json`);await fs.writeFile(file,JSON.stringify(report));
      console.log(execFileSync(process.execPath,['tools/device_qa/replay.mjs',file,path.join(site,'coin_game')],{encoding:'utf8'}).trim());
      await page.locator('#panel').evaluate(e=>e.scrollTop=0);await page.screenshot({path:path.join(shots,`${name}.png`),fullPage:true});
      await page.evaluate(()=>Object.defineProperty(navigator,'canShare',{value:()=>false,configurable:true}));
      await page.locator('#share').click();await page.waitForFunction(()=>document.getElementById('status').textContent.includes('nicht verfügbar'),null,{polling:50});
      for(const game of ['sky_garden','lantern_maze']) {
        await page.goto(`${base}/device-qa/?game=../${game}/`);await page.waitForFunction(()=>document.getElementById('status').textContent.includes('Recorder bereit'),null,{polling:50});
        await page.locator('#toggle').click();
        const child=page.frames().find(f=>f.parentFrame()===page.mainFrame());await child.locator('#start:not([disabled])').waitFor();await child.locator('#start').tap();await child.locator('#modal').waitFor({state:'hidden'});
        await child.evaluate(()=>advanceFrames(31));await page.locator('#toggle').click();await page.locator('#stop').click();await page.locator('#download').waitFor({state:'visible'});
        const smoke=await page.locator('#download').evaluate(async a=>JSON.parse(await (await fetch(a.href)).text()));assert.equal(smoke.trace.complete,true);
        const saved=path.join(shots,`${name}-${game}.json`);await fs.writeFile(saved,JSON.stringify(smoke));
        console.log(execFileSync(process.execPath,['tools/device_qa/replay.mjs',saved,path.join(site,game)],{encoding:'utf8'}).trim());
      }
      await page.goto(`${base}/device-qa/?game=../coin_game/`);await page.waitForFunction(()=>document.getElementById('status').textContent.includes('Recorder bereit'),null,{polling:50});await page.locator('#stop').click();await page.locator('#download').waitFor({state:'visible'});
      const empty=await page.locator('#download').evaluate(async a=>JSON.parse(await (await fetch(a.href)).text()));assert.equal(empty.trace.complete,false);assert.equal(empty.trace.reason,'no ticks');
      await page.goto(`${base}/device-qa/?game=https://invalid.example/`);assert.match(await page.locator('#status').innerText(),/derselben Origin/);assert.equal(await page.locator('iframe').getAttribute('src'),null);
      assert.deepEqual(errors,[]);console.log(`PASS device QA ${engine}/${device}: first tap, win/restart trace, owner checklist, JSON export/replay, share fallback, foreign URL refused; no page errors`);
      await page.close();
    }finally{await browser.close();}
  }
}finally{await new Promise(resolve=>server.close(resolve));}
