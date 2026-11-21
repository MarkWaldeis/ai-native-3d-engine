import {Trace,instrument,digest} from './trace.mjs';
const trace=new Trace(),parentOrigin=location.origin;
const send=(type,value)=>parent.postMessage({type:`device-qa-${type}`,value},parentOrigin);
const safe=value=>String(value).replace(/https?:\/\/[^\s)]+/g,value=>{try{return new URL(value).pathname;}catch{return '[URL]';}}).slice(0,512);
const metadata={started:new Date().toISOString(),path:location.pathname,userAgent:navigator.userAgent.slice(0,512),language:navigator.language,viewport:{width:innerWidth,height:innerHeight,dpr:devicePixelRatio,touchPoints:navigator.maxTouchPoints},physicalDevice:'not_verified',fingerprintAlgorithm:'fnv1a32-canonical-json (diagnostic, not cryptographic)'};
let restore=()=>{},finished=false;
const channel=new MessageChannel();
channel.port1.onmessage=event=>{
  if(event.data?.type!=='device-qa-stop'||finished)return;
  finished=true;
  trace.stopped=true;restore();metadata.finished=new Date().toISOString();
  channel.port1.postMessage({type:'device-qa-report',value:trace.report(metadata)});
};
for(const name of ['resize','orientationchange','pagehide','pageshow'])addEventListener(name,()=>trace.event(name,{width:innerWidth,height:innerHeight}));
document.addEventListener('visibilitychange',()=>trace.event('visibility',{state:document.visibilityState}));
addEventListener('error',event=>trace.event('error',{message:safe(event.message||'asset load failed')}),true);
addEventListener('unhandledrejection',event=>trace.event('unhandledrejection',{message:safe(event.reason?.message||event.reason)}));
try {
  const [{WebGame},scene,scripts]=await Promise.all([import(new URL('pkg/engine_web.js',location.href).href),fetch('scene.json').then(r=>{if(!r.ok)throw Error('scene.json unavailable');return r.json();}),fetch('scripts.json').then(r=>{if(!r.ok)throw Error('scripts.json unavailable');return r.json();})]);
  metadata.assets={scene:digest(scene),scripts:digest(scripts)};
  restore=instrument(WebGame,trace,error=>send('error',safe(error.message)));
  parent.postMessage({type:'device-qa-ready',value:metadata},parentOrigin,[channel.port2]);
} catch(error){send('error',safe(error.message));}
