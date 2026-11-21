import * as T from 'three';
import { Sky } from './vendor/addons/objects/Sky.js';
import { CSM } from './vendor/addons/csm/CSM.js';
import { EffectComposer } from './vendor/addons/postprocessing/EffectComposer.js';
import { RenderPass } from './vendor/addons/postprocessing/RenderPass.js';
import { ShaderPass } from './vendor/addons/postprocessing/ShaderPass.js';
import { UnrealBloomPass } from './vendor/addons/postprocessing/UnrealBloomPass.js';
import { GTAOPass } from './vendor/addons/postprocessing/GTAOPass.js';
import { SMAAPass } from './vendor/addons/postprocessing/SMAAPass.js';
import { BokehPass } from './vendor/addons/postprocessing/BokehPass.js';
import { OutputPass } from './vendor/addons/postprocessing/OutputPass.js';
import { FXAAShader } from './vendor/addons/shaders/FXAAShader.js';
import { mergeGeometries } from './vendor/addons/utils/BufferGeometryUtils.js';
import { renderConfig, sunDirection, foliagePlacements, QualityGovernor } from './render-config.js';
export { renderConfig, sunDirection, foliagePlacements, QualityGovernor, PRESETS } from './render-config.js';

export class Visuals {
  constructor(renderer,scene,camera,config) {
    Object.assign(this,{renderer,scene,camera,config,time:0,tiles:new Map(),materials:new Set(),wind:{value:0},viewer:{value:new T.Vector3()}});
    this.root=new T.Group(); scene.add(this.root);
    this.previous={background:scene.background,fog:scene.fog,environment:scene.environment,tone:renderer.toneMapping,exposure:renderer.toneMappingExposure};
    this.oldLights=[]; scene.traverse(o=>{if(o.isLight){this.oldLights.push([o,o.visible]);o.visible=false;}});
    this.direction=new T.Vector3(...sunDirection(config.time_of_day));
    const daylight=Math.max(0,this.direction.y), horizon=new T.Color().setHSL(0.54,0.25,0.3+daylight*0.42);
    scene.background=horizon;
    this.sky=new Sky(); this.sky.scale.setScalar(4500);
    const uniforms=this.sky.material.uniforms;
    uniforms.turbidity.value=7; uniforms.rayleigh.value=2; uniforms.mieCoefficient.value=0.005; uniforms.mieDirectionalG.value=0.8; uniforms.sunPosition.value.copy(this.direction);
    if(config.sky) this.root.add(this.sky);
    const pmrem=new T.PMREMGenerator(renderer); this.environment=pmrem.fromScene(this.sky,0.04); scene.environment=this.environment.texture; pmrem.dispose();
    this.sun=new T.DirectionalLight(new T.Color().setHSL(0.1,0.25+0.5*(1-daylight),0.9),daylight*3.5);
    this.root.add(this.sun,this.sun.target,new T.HemisphereLight(horizon,0x304226,0.3+daylight*0.7));
    renderer.toneMapping=T.ACESFilmicToneMapping; renderer.toneMappingExposure=config.exposure;
    this.ray=new T.Raycaster(); this.ray.ray.direction.set(0,-1,0);
    this.sample=(x,z)=>{
      this.ray.ray.origin.set(x,10000,z);
      return this.ray.intersectObjects(this.grounds || [],true).find(h=>h.face?.normal.clone().transformDirection(h.object.matrixWorld).y>0.7)?.point.y ?? 0;
    };
    if(config.water) this.createWater();
    this.governor=new QualityGovernor(config.preset,config.auto_quality,config.coarse); this.configure();
    window.__aiEngineVisuals=this;
  }
  setHeightSampler(sample) { if(typeof sample!=='function') throw new TypeError('Height sampler must be a function'); this.sample=sample; this.clearTiles(); }
  createWater() {
    const data=new Uint8Array(64*64*4);
    for(let y=0;y<64;y++) for(let x=0;x<64;x++) {const i=(y*64+x)*4; data[i]=128+Math.sin(x*0.7+y*0.3)*35;data[i+1]=128+Math.cos(y*0.8+x*0.2)*35;data[i+2]=250;data[i+3]=255;}
    this.normal=new T.DataTexture(data,64,64); this.normal.wrapS=this.normal.wrapT=T.RepeatWrapping;this.normal.repeat.set(80,80);this.normal.needsUpdate=true;
    this.water=new T.Mesh(new T.PlaneGeometry(2000,2000),new T.MeshPhysicalMaterial({color:0x176e72,metalness:0.25,roughness:0.18,normalMap:this.normal,normalScale:new T.Vector2(0.35,0.35),transparent:true,opacity:0.88,clearcoat:1}));
    this.water.rotation.x=-Math.PI/2;this.water.position.y=this.config.water.level ?? 0;this.root.add(this.water);
  }
  configure() {
    const p=this.settings=this.governor.settings(),{renderer,scene,config,camera}=this;
    this.disposePost(); this.csm?.remove(); this.csm?.dispose(); this.csm=null; this.materials.clear();
    scene.traverse(o=>{for(const m of (Array.isArray(o.material)?o.material:[o.material])) if(m?.userData.visualCompile)m.onBeforeCompile=m.userData.visualCompile;});
    this.sun.castShadow=config.shadows && p.cascades===1; renderer.shadowMap.enabled=config.shadows && p.cascades>0; renderer.shadowMap.autoUpdate=true;
    if(config.shadows && p.cascades>1) this.csm=new CSM({camera,parent:this.root,cascades:p.cascades,maxFar:Math.min(camera.far,160),shadowMapSize:p.shadowSize,lightDirection:this.direction.clone().negate(),lightIntensity:this.sun.intensity,lightNear:0.1,lightFar:400,shadowBias:-0.00015});
    this.sun.visible=!this.csm;
    this.sun.shadow.mapSize.setScalar(p.shadowSize || 1024);Object.assign(this.sun.shadow.camera,{left:-25,right:25,top:25,bottom:-25,near:0.1,far:180});this.sun.shadow.normalBias=0.035;
    if(config.fog) scene.fog=new T.FogExp2(new T.Color().setHSL(0.54,0.25,0.62),config.fog.density ?? p.fog); else scene.fog=null;
    if(p.antialias) {
      this.composer=new EffectComposer(renderer);this.composer.addPass(new RenderPass(scene,camera));
      if(p.ao && config.ao!==false || config.ao===true && p.preset==='high') {
        const ao=new GTAOPass(scene,camera,innerWidth,innerHeight),render=ao.render.bind(ao);
        ao.render=(...args)=>{const hidden=[this.sky,this.water,...this.tiles.values()].filter(Boolean).map(o=>[o,o.visible]);for(const [o] of hidden)o.visible=false;try{render(...args);}finally{for(const [o,visible] of hidden)o.visible=visible;}};
        this.composer.addPass(ao);
      }
      if(p.bloom && config.bloom!==false) this.composer.addPass(new UnrealBloomPass(new T.Vector2(innerWidth,innerHeight),typeof config.bloom==='number'?config.bloom:0.22,0.5,1));
      if(p.preset==='ultra' && config.dof) this.composer.addPass(new BokehPass(scene,camera,{focus:20,aperture:0.00005,maxblur:0.006}));
      this.composer.addPass(new OutputPass());
      if(p.antialias==='smaa') this.composer.addPass(new SMAAPass(innerWidth,innerHeight));
      else {this.fxaa=new ShaderPass(FXAAShader);this.composer.addPass(this.fxaa);}
    }
    this.clearTiles();this.resize(innerWidth,innerHeight);
  }
  vegetationMaterial(color, tree = false) {
    const material=new T.MeshStandardMaterial({color,roughness:0.9,side:T.DoubleSide,envMapIntensity:0.25});
    material.onBeforeCompile=shader=>{
      shader.uniforms.uTime=this.wind;shader.uniforms.uFade={value:this.settings.distance};shader.uniforms.uNear={value:tree?8:0};shader.uniforms.uViewer=this.viewer;
      shader.vertexShader='uniform float uTime; uniform float uFade; uniform float uNear; uniform vec3 uViewer;\n'+shader.vertexShader;
      shader.vertexShader=shader.vertexShader.replace('#include <begin_vertex>',`#include <begin_vertex>
        vec3 base = (modelMatrix * instanceMatrix * vec4(0.,0.,0.,1.)).xyz;
        float fade = 1. - smoothstep(uFade * 0.7, uFade, distance(base.xz,uViewer.xz));
        fade *= smoothstep(uNear, uNear + 3., distance(base.xz,uViewer.xz));
        transformed *= fade;
        transformed.x += sin(uTime * 1.6 + base.x * 0.3 + base.z * 0.2) * max(position.y,0.) * 0.08;`);
    };
    material.userData.visualCompile=material.onBeforeCompile;
    return material;
  }
  tile(x,z) {
    const placements=foliagePlacements(x,z,{density:this.settings.foliage*(this.config.foliage.density ?? 1),sample:this.sample,water:this.water?.position.y ?? -Infinity});
    const group=new T.Group(), dummy=new T.Object3D();
    const blades=[];
    for(let i=0;i<5;i++) {
      const blade=new T.PlaneGeometry(0.085,0.65+(i%3)*0.13,1,4);blade.translate(0,0.325+(i%3)*0.065,0);
      const positions=blade.attributes.position;
      for(let j=0;j<positions.count;j++){const y=positions.getY(j);positions.setX(j,positions.getX(j)*Math.max(0,1-y)+y*y*0.18);}
      blade.rotateY(i*2.4);blade.translate(Math.sin(i*2.4)*0.15,0,Math.cos(i*2.4)*0.15);blade.computeVertexNormals();blades.push(blade);
    }
    const grass=mergeGeometries(blades);for(const blade of blades)blade.dispose();
    const clusters=[];
    for(let i=0;i<7;i++){const cluster=new T.IcosahedronGeometry(i?1.15:1.8,1);cluster.scale(1,0.6,1);cluster.translate(i?Math.sin(i*2.4)*1.65:0,5+(i%3)*0.4,i?Math.cos(i*2.4)*1.65:0);clusters.push(cluster);}
    const crown=mergeGeometries(clusters);for(const cluster of clusters)cluster.dispose();
    const trunk=new T.CylinderGeometry(0.15,0.3,4,6);trunk.translate(0,2,0);
    for(const [geometry,color,items] of [[grass,0x548835,placements],[crown,0x285d39,placements.filter(p=>p.tree)],[trunk,0x65503a,placements.filter(p=>p.tree)]]) {
      const mesh=new T.InstancedMesh(geometry,this.vegetationMaterial(color,geometry!==grass),items.length);
      mesh.customDepthMaterial=new T.MeshDepthMaterial({depthPacking:T.RGBADepthPacking,side:T.DoubleSide});mesh.customDepthMaterial.onBeforeCompile=mesh.material.onBeforeCompile;
      items.forEach((p,i)=>{dummy.position.set(p.x,p.y,p.z);dummy.rotation.set(0,p.rotation,0);dummy.scale.setScalar(p.scale);dummy.updateMatrix();mesh.setMatrixAt(i,dummy.matrix);mesh.setColorAt(i,new T.Color().setHSL(0.23+(i%7)*0.009,0.35,0.22+(i%5)*0.025));});
      mesh.castShadow=geometry!==grass;mesh.receiveShadow=true;mesh.userData.visual=true;group.add(mesh);
    }
    this.root.add(group);this.tiles.set(`${x},${z}`,group);
  }
  removeTile(key,group) {group.removeFromParent();group.traverse(o=>{this.materials.delete(o.material);this.csm?.shaders.delete(o.material);o.dispose?.();o.geometry?.dispose();o.material?.dispose();o.customDepthMaterial?.dispose();});this.tiles.delete(key);}
  clearTiles() {for(const [key,group] of this.tiles) this.removeTile(key,group);}
  update(elapsed,camera,focusPosition) {
    this.camera=camera;this.time+=elapsed;this.wind.value=this.time;this.viewer.value.copy(camera.position);this.sky.position.copy(camera.position);
    if(this.governor.update(elapsed)) this.configure();
    const target=focusPosition || camera.position;
    this.sun.target.position.copy(target);this.sun.position.copy(target).addScaledVector(this.direction,80);
    this.csm?.update();
    this.scene.traverse(o=>{if(o.isMesh && !this.materials.has(o.material)){if(o!==this.sky && o!==this.water && !o.userData.visual){o.castShadow=true;o.receiveShadow=true;}for(const m of (Array.isArray(o.material)?o.material:[o.material])) if(m?.isMeshStandardMaterial && this.csm){const compile=m.onBeforeCompile;this.csm.setupMaterial(m);const csmCompile=m.onBeforeCompile;m.onBeforeCompile=function(shader,renderer){compile.call(this,shader,renderer);csmCompile.call(this,shader,renderer);};m.needsUpdate=true;}this.materials.add(o.material);}});
    if(this.water) {this.water.position.x=camera.position.x;this.water.position.z=camera.position.z;this.normal.offset.set(this.time*0.007,this.time*0.004);}
    if(this.config.foliage) {
      this.grounds=[];this.scene.updateMatrixWorld(true);this.scene.traverse(o=>{if(o.userData.support)this.grounds.push(o);});
      const cx=Math.floor(camera.position.x/16),cz=Math.floor(camera.position.z/16),r=Math.ceil(this.settings.distance/16);
      for(const [key,group] of this.tiles) {const [x,z]=key.split(',').map(Number);if(Math.hypot(x-cx,z-cz)>r) this.removeTile(key,group);}
      let budget=2;
      for(let d=0;d<=r;d++) for(let x=cx-d;x<=cx+d;x++) for(let z=cz-d;z<=cz+d;z++) if(budget && Math.hypot(x-cx,z-cz)<=r && !this.tiles.has(`${x},${z}`)) {this.tile(x,z);budget--;}
    }
  }
  render(camera) {const reset=this.renderer.info.autoReset;this.renderer.info.autoReset=false;this.renderer.info.reset();if(this.composer) this.composer.render();else this.renderer.render(this.scene,camera);this.renderer.info.autoReset=reset;}
  resize(w,h) {const ratio=Math.min(devicePixelRatio,this.settings.pixelRatio);this.renderer.setPixelRatio(ratio);this.renderer.setSize(w,h,false);this.composer?.setPixelRatio(ratio);this.composer?.setSize(w,h);this.fxaa?.material.uniforms.resolution.value.set(1/(w*ratio),1/(h*ratio));}
  stats() {return {quality:`${this.settings.preset}:${this.governor.level}`,foliageTiles:this.tiles.size};}
  disposePost() {for(const pass of this.composer?.passes || []) pass.dispose?.();this.composer?.dispose();this.composer=null;this.fxaa=null;}
  dispose() {this.disposePost();this.clearTiles();this.csm?.remove();this.csm?.dispose();this.root.removeFromParent();this.root.traverse(o=>{o.geometry?.dispose();o.material?.dispose();});this.normal?.dispose();this.environment.dispose();Object.assign(this.scene,{background:this.previous.background,fog:this.previous.fog,environment:this.previous.environment});this.renderer.toneMapping=this.previous.tone;this.renderer.toneMappingExposure=this.previous.exposure;for(const [o,visible] of this.oldLights)o.visible=visible;if(window.__aiEngineVisuals===this)delete window.__aiEngineVisuals;}
}
