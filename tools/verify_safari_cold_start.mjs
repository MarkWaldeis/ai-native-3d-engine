import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import http from 'node:http';
import path from 'node:path';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';

const exec = promisify(execFile);
const site = path.resolve(process.argv[2] || 'target/m9-site');
const device = process.argv[3];
assert.ok(device, 'usage: node tools/verify_safari_cold_start.mjs SITE SIMULATOR_UDID [COUNT]');
const count = Number(process.argv[4] || 5);
assert.ok(Number.isInteger(count) && count > 0 && count <= 20);
const game = process.env.GAME || 'sky_garden';
assert.ok(['sky_garden','coin_game'].includes(game));
const startGame = process.env.START_GAME === '1';
const shots = path.resolve(process.env.SHOTS_DIR || 'target/safari-cold-shots');
await fs.mkdir(shots, {recursive:true});
const reports = [];
let pending;
const monitor = `<script>
(() => {
 const id = new URL(location.href).searchParams.get('cold');
 const errors = []; let frames = 0, gl, sent = false;
 const getContext = HTMLCanvasElement.prototype.getContext;
 HTMLCanvasElement.prototype.getContext = function(type,...args) {const result=getContext.call(this,type,...args);if(type==='webgl2')gl=result;return result;};
 const parameter = WebGL2RenderingContext.prototype.getProgramParameter;
 WebGL2RenderingContext.prototype.getProgramParameter = function(program,key) {
  const result=parameter.call(this,program,key);
  if(key===this.LINK_STATUS && !result) errors.push({program:this.getProgramInfoLog(program),shaders:(this.getAttachedShaders(program)||[]).map(shader=>({type:this.getShaderParameter(shader,this.SHADER_TYPE),log:this.getShaderInfoLog(shader)}))});
  return result;
 };
 const raf = requestAnimationFrame;
 window.requestAnimationFrame = callback => raf(time=>{callback(time);frames++;});
 const interval=setInterval(()=>{
  const error=document.getElementById('modal')?.dataset.screen==='error';
  if(sent || (!error && (frames<30 || document.getElementById('start')?.disabled!==false)))return;
  if(!error && ${JSON.stringify(startGame)} && !document.getElementById('modal').hidden) {document.getElementById('start').click();frames=0;return;}
  sent=true;clearInterval(interval);
  fetch('/qa/report',{method:'POST',body:JSON.stringify({id,status:error?'error':'rendered',frames,errors,message:error?document.getElementById('modal-text').textContent:null,userAgent:navigator.userAgent,pixelRatio:devicePixelRatio,quality:document.getElementById('game')?.dataset.quality,cameraMode:document.getElementById('game')?.dataset.cameraMode,started:document.getElementById('modal')?.hidden,buffer:gl?[gl.drawingBufferWidth,gl.drawingBufferHeight]:null,maxTexture:gl?.getParameter(gl.MAX_TEXTURE_SIZE),maxRenderbuffer:gl?.getParameter(gl.MAX_RENDERBUFFER_SIZE),precision:gl?gl.getShaderPrecisionFormat(gl.FRAGMENT_SHADER,gl.HIGH_FLOAT)?.precision:null,contextLost:gl?.isContextLost()})});
 },100);
})();
</script>`;
const server = http.createServer(async(req,res)=>{
 try {
  if(req.url==='/qa/report' && req.method==='POST') {
   const chunks=[];for await(const chunk of req)chunks.push(chunk);
   const report=JSON.parse(Buffer.concat(chunks));
   if(pending?.id===report.id)pending.resolve(report);
   res.end('ok');return;
  }
  let file=path.resolve(site,'.'+new URL(req.url,'http://localhost').pathname);
  if(!file.startsWith(site+path.sep))throw Error('outside site');
  if((await fs.stat(file)).isDirectory())file=path.join(file,'index.html');
  let body=await fs.readFile(file);
  if(file.endsWith('.html'))body=body.toString().replace('<head>','<head>'+monitor);
  res.setHeader('Cache-Control','no-store');
  res.setHeader('Content-Type',({'.html':'text/html','.js':'text/javascript','.wasm':'application/wasm','.json':'application/json','.css':'text/css','.glb':'model/gltf-binary'})[path.extname(file)]||'application/octet-stream');
  res.end(body);
 }catch {res.writeHead(404);res.end('Not found');}
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
try {
 for(let i=1;i<=count;i++) {
  await exec('xcrun',['simctl','terminate',device,'com.apple.mobilesafari']).catch(error=>{if(!/not running|found nothing|ProcessLookup/i.test(error.stderr||''))throw error;});
  let timer;
  const result=new Promise(resolve=>{pending={id:String(i),resolve};timer=setTimeout(()=>resolve({id:String(i),status:'timeout'}),120000);});
  await exec('xcrun',['simctl','openurl',device,`http://127.0.0.1:${server.address().port}/${game}/?cold=${i}`]);
  const report=await result;clearTimeout(timer);pending=null;reports.push(report);
  await exec('xcrun',['simctl','io',device,'screenshot',path.join(shots,`ipad-cold-${i}.png`)]);
  console.log(JSON.stringify(report));
 }
 await fs.writeFile(path.join(shots,'reports.json'),JSON.stringify(reports,null,2));
 console.log(`Safari cold starts: ${reports.filter(r=>r.status==='rendered').length}/${count} rendered; ${reports.filter(r=>r.status==='error').length} graphics errors; ${reports.filter(r=>r.status==='timeout').length} timeouts`);
 process.exitCode=reports.every(r=>r.status==='rendered')?0:1;
}finally {await new Promise(resolve=>server.close(resolve));}
