import fs from 'node:fs/promises';
import path from 'node:path';
import http from 'node:http';
import {validateFixture} from './nav_scaling.mjs';

const [fixturePath,gameDir,budgetArg,...extra]=process.argv.slice(2);
if(!fixturePath||!gameDir||extra.length)throw Error('Usage: node tools/verify_nav_scaling.mjs FIXTURE.json EXPORTED_GAME_DIR [MAX_WASM_TICK_MS]');
const maxTickMs=budgetArg===undefined?100:Number(budgetArg);
if(maxTickMs!==undefined&&(!Number.isFinite(maxTickMs)||maxTickMs<=0))throw Error('Tick budget must be finite and positive');
const fixture=validateFixture(JSON.parse(await fs.readFile(fixturePath,'utf8')));
const files=new Map([
  ['/pkg/engine_web.js',['text/javascript',await fs.readFile(path.join(gameDir,'pkg/engine_web.js'))]],
  ['/pkg/engine_web_bg.wasm',['application/wasm',await fs.readFile(path.join(gameDir,'pkg/engine_web_bg.wasm'))]],
  ['/nav_scaling.mjs',['text/javascript',await fs.readFile(new URL('./nav_scaling.mjs',import.meta.url))]],
  ['/',['text/html',Buffer.from('<!doctype html><title>Navigation QA</title><link rel="icon" href="data:,">')]],
]);
const {chromium,webkit,devices}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const server=http.createServer((req,res)=>{const file=files.get(req.url);if(!file){res.writeHead(404);res.end();return;}res.setHeader('Content-Type',file[0]);res.end(file[1]);});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
try {
  for(const [engine,device] of [['chromium',null],['webkit','iPhone 13'],['webkit','iPad Pro 11']]) {
    const browser=await (engine==='webkit'?webkit:chromium).launch();
    try {
      const page=await browser.newPage(device?devices[device]:{}),errors=[];page.on('pageerror',e=>errors.push(e.message));
      await page.goto(`http://127.0.0.1:${server.address().port}/`);
      const results=await page.evaluate(async({fixture,maxTickMs})=>{
        const module=await import('/pkg/engine_web.js'),{runCases}=await import('/nav_scaling.mjs');
        module.initSync({module:new WebAssembly.Module(await (await fetch('/pkg/engine_web_bg.wasm')).arrayBuffer())});
        return runCases(module,fixture,maxTickMs===undefined?{}:{maxTickMs});
      },{fixture,maxTickMs});
      if(errors.length)throw Error(errors.join('\n'));
      console.log(JSON.stringify({engine,device:device||'desktop',budget_ms:maxTickMs??null,results}));
      console.log(`PASS navigation scaling ${engine}/${device||'desktop'}: ${results.length} workloads; positions, targets, statuses, full routes and cursors match native (1e-5); no page errors`);
    } finally {await browser.close();}
  }
} finally {await new Promise(resolve=>server.close(resolve));}
